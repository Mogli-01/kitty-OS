//! Phase 9a: bringing up the other CPUs (application processors, APs).
//!
//! Limine starts every CPU in 64-bit mode and parks each AP in a loop that
//! waits for its `goto_address`. We never send INIT/SIPI ourselves. For each
//! AP the boot CPU (BSP):
//! 1. gives it a CPU index (1..), a guarded kernel stack and three guarded
//!    IST stacks (double fault, NMI, machine check), written into its TSS;
//! 2. writes `goto_address` = [`ap_entry`] (with the index as the argument).
//!
//! The AP then, **still on Limine's page tables** (where its GDT page is
//! writable, which `ltr` needs for the TSS busy bit):
//! 3. loads its own GDT/TSS and GS base;
//! 4. turns on NX (refusing a CPU without it), programs the PAT like the
//!    BSP and turns global pages off, so the CR3 load below flushes every
//!    TLB entry Limine's tables left;
//! 5. switches to the kernel's page tables and its own stack in one asm
//!    block, and continues in [`ap_main`]:
//! 6. loads the shared IDT **only now**: its IST stacks exist only in the
//!    kernel's tables, so an NMI delivered earlier through our IDT would
//!    fault on an unmapped stack and triple-fault. Until here the AP still
//!    has Limine's IDT (a short window with interrupts off);
//! 7. CR0.WP, global pages, **exactly** the BSP's SMEP/SMAP/UMIP (bits
//!    the firmware left on are cleared, and CR4 is read back), user SSE
//!    settings; checks that its APIC ID is the one Limine reported;
//! 8. counts itself online and parks (`cli; hlt`).
//!
//! APs the kernel does not start (past `MAX_CPUS`, IDs xAPIC cannot
//! address, no memory for stacks) are sent to [`ap_park`], so none is
//! left reading its `goto_address` forever: a stray kernel write there
//! would otherwise start a CPU on Limine's tables, with none of our
//! protections. An AP that reports after the boot CPU stopped waiting
//! parks instead of going online (`CLOSED`).
//!
//! An AP that lacks a protection the BSP uses, or whose APIC ID does not
//! match, is **refused**: it parks without going online. In Phase 9a APs
//! run no threads; the scheduler, IPIs and TLB shootdown come in 9b.
//! Until then a parked AP touches no memory but its own stack, so stale
//! TLB entries on it cannot be used.

use core::arch::asm;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};

use limine::mp::MpInfo;

use super::cpu::{self, UserProtections};
use super::{boot, gdt, idt, stacks};
use crate::mm::stack::KernelStack;

/// What the boot CPU found and started (no addresses: printable).
#[derive(Clone, Copy, Debug, Default)]
pub struct SmpReport {
    /// CPUs the bootloader reported (including the boot CPU).
    pub reported: usize,
    /// CPUs running the kernel (including the boot CPU).
    pub online: usize,
    /// APs that started but refused to go online (missing protection or
    /// wrong APIC ID).
    pub refused: usize,
    /// APs not started: past `MAX_CPUS`, an x2APIC-only ID, or no memory
    /// for their stacks.
    pub skipped: usize,
    /// APs started that did not report back in time.
    pub silent: usize,
    /// The boot CPU's APIC ID did not match the bootloader's: no AP started.
    pub bsp_mismatch: bool,
}

/// Per-AP data the boot CPU writes before starting it.
struct ApSlot {
    stack_top: AtomicU64,
    lapic_id: AtomicU32,
}

impl ApSlot {
    const fn empty() -> Self {
        Self {
            stack_top: AtomicU64::new(0),
            lapic_id: AtomicU32::new(0),
        }
    }
}

static SLOTS: [ApSlot; gdt::MAX_CPUS] = [const { ApSlot::empty() }; gdt::MAX_CPUS];
/// Physical address of the kernel PML4 every AP switches to.
static KERNEL_ROOT: AtomicU64 = AtomicU64::new(0);
static WANT_SMEP: AtomicBool = AtomicBool::new(false);
static WANT_SMAP: AtomicBool = AtomicBool::new(false);
static WANT_UMIP: AtomicBool = AtomicBool::new(false);
static ONLINE: AtomicUsize = AtomicUsize::new(0);
static REFUSED: AtomicUsize = AtomicUsize::new(0);
static STARTED: AtomicBool = AtomicBool::new(false);
/// Set when the boot CPU stops waiting: a late AP parks instead.
static CLOSED: AtomicBool = AtomicBool::new(false);

/// How long the boot CPU waits for the APs, in TSC cycles (a few seconds
/// at any realistic clock; QEMU answers in milliseconds).
const WAIT_CYCLES: u64 = 10_000_000_000;

/// Starts every AP the bootloader reported (at most `MAX_CPUS - 1`) and
/// waits until each one is online or refused. Runs once, on the boot CPU,
/// after the VMM, the heap and the boot CPU's protections are set up;
/// `prot` are the protections the boot CPU runs with. Later calls return
/// an empty report.
pub fn start_aps(prot: UserProtections) -> SmpReport {
    let mut report = SmpReport {
        reported: 1,
        online: 1,
        ..SmpReport::default()
    };
    if STARTED.swap(true, Ordering::AcqRel) {
        return report;
    }
    let Some(mp) = boot::mp_response() else {
        return report; // no MP information: run on the boot CPU only
    };
    let cpus = mp.cpus();
    report.reported = cpus.len().max(1);
    if cpu::initial_apic_id() != mp.bsp_lapic_id {
        // The bootloader's view of who we are is wrong: starting the
        // others could start this CPU twice. Stay single-CPU.
        report.bsp_mismatch = true;
        return report;
    }

    KERNEL_ROOT.store(cpu::read_cr3(), Ordering::Relaxed);
    WANT_SMEP.store(prot.smep, Ordering::Relaxed);
    WANT_SMAP.store(prot.smap, Ordering::Relaxed);
    WANT_UMIP.store(prot.umip, Ordering::Relaxed);

    let mut next = 1;
    let mut started = 0;
    for info in cpus.iter().filter(|c| c.lapic_id != mp.bsp_lapic_id) {
        // Flags 0 asked for xAPIC mode: an ID that does not fit 8 bits
        // (or the broadcast ID 0xFF) cannot be addressed by it.
        if next >= gdt::MAX_CPUS || info.lapic_id >= 0xFF {
            info.bootstrap(ap_park, 0);
            report.skipped += 1;
            continue;
        }
        let Some(tops) = alloc_stacks() else {
            info.bootstrap(ap_park, 0);
            report.skipped += 1;
            continue;
        };
        // SAFETY: CPU `next` has not been started (each index is used
        // once), and the IST tops are guarded stacks leaked for good.
        if !unsafe { gdt::set_ist(next, tops.ist) } {
            info.bootstrap(ap_park, 0);
            report.skipped += 1;
            continue;
        }
        SLOTS[next].stack_top.store(tops.main, Ordering::Relaxed);
        // Test only: give the first AP a wrong expected APIC ID, so it must
        // refuse to go online (checks the refusal path).
        let expected = if cfg!(feature = "selftest-smp-refuse") && next == 1 {
            info.lapic_id ^ 0x40
        } else {
            info.lapic_id
        };
        SLOTS[next].lapic_id.store(expected, Ordering::Relaxed);
        // `bootstrap` publishes with Release ordering: everything written
        // above (slots, TSS, globals) is visible to the AP when it starts.
        info.bootstrap(ap_entry, next as u64);
        next += 1;
        started += 1;
    }

    let start = cpu::rdtsc();
    let done = || ONLINE.load(Ordering::Acquire) + REFUSED.load(Ordering::Acquire);
    while done() < started && cpu::rdtsc().wrapping_sub(start) < WAIT_CYCLES {
        core::hint::spin_loop();
    }
    // From here the count is final: an AP that has not counted itself yet
    // parks. (An AP between its CLOSED check and its increment can still
    // land; `online_cpus` reads the counter, so it stays consistent.)
    CLOSED.store(true, Ordering::Release);
    let online = ONLINE.load(Ordering::Acquire);
    let refused = REFUSED.load(Ordering::Acquire);
    report.online = 1 + online;
    report.refused = refused;
    report.silent = started.saturating_sub(online + refused);
    report
}

/// Number of CPUs running the kernel, the boot CPU included.
pub fn online_cpus() -> usize {
    1 + ONLINE.load(Ordering::Acquire)
}

struct ApStacks {
    main: u64,
    ist: [u64; stacks::IST_COUNT],
}

/// One kernel stack and the IST stacks for an AP, each with an unmapped
/// guard page below it. APs never stop, so the stacks are leaked.
fn alloc_stacks() -> Option<ApStacks> {
    let main = KernelStack::new(&[]).ok()?;
    let mut ist_stacks = [const { None }; stacks::IST_COUNT];
    for slot in ist_stacks.iter_mut() {
        *slot = Some(KernelStack::new(&[]).ok()?);
    }
    let mut ist = [0u64; stacks::IST_COUNT];
    for (top, stack) in ist.iter_mut().zip(ist_stacks.iter()) {
        *top = stack.as_ref().map_or(0, KernelStack::top);
    }
    let tops = ApStacks {
        main: main.top(),
        ist,
    };
    core::mem::forget(main);
    core::mem::forget(ist_stacks);
    Some(tops)
}

/// Where APs the kernel does not use go: interrupts off, `hlt` forever,
/// no memory touched (Limine's stack is fine for that).
///
/// # Safety
/// Only Limine calls it, on an AP the kernel decided not to start.
unsafe extern "C" fn ap_park(_info: &MpInfo) -> ! {
    cpu::halt_forever();
}

/// First code an AP runs, on Limine's page tables and stack, with
/// interrupts disabled.
///
/// # Safety
/// Only Limine calls it, once per AP, after [`start_aps`] filled in the
/// AP's slot and TSS and published them with `bootstrap`.
unsafe extern "C" fn ap_entry(info: &MpInfo) -> ! {
    let index = usize::try_from(info.extra_argument()).unwrap_or(0);
    let Some(slot) = SLOTS.get(index).filter(|_| index != 0) else {
        cpu::halt_forever();
    };
    let top = slot.stack_top.load(Ordering::Acquire);
    let root = KERNEL_ROOT.load(Ordering::Acquire);
    if top == 0 || root == 0 {
        cpu::halt_forever();
    }
    if CLOSED.load(Ordering::Acquire) || !cpu::has_nx() {
        // Too late (the boot CPU already reported), or this CPU cannot
        // honour the NX bits of the kernel's tables.
        REFUSED.fetch_add(1, Ordering::Release);
        cpu::halt_forever();
    }
    // SAFETY: this CPU was assigned `index` (only it got that argument),
    // interrupts are off, and the boot CPU set up GDT/TSS `index` before
    // starting it. We are on Limine's tables, where the GDT page is
    // writable for the busy bit `ltr` sets.
    unsafe { gdt::load_on_ap(index) };
    cpu::enable_nx();
    // SAFETY: the kernel tables (loaded right below) are built for this
    // PAT layout; nothing runs in between that relies on a memory type.
    unsafe { cpu::program_pat() };
    // With PGE off, the CR3 load below flushes every TLB entry, global
    // ones included: nothing from Limine's tables (where .text and the GDT
    // are writable through its HHDM) survives into ours.
    cpu::disable_global_pages();
    // SAFETY: `root` is the kernel PML4 the boot CPU runs on: it maps this
    // code (kernel text), the HHDM (where Limine's stack and `info` live)
    // and `top`, the top of a mapped, guarded kernel stack that only this
    // CPU uses. NXE is on, so NX bits are honoured. Switching CR3 and RSP
    // in one block means no Rust code runs between the two; the call
    // pushes the return address onto the new 16-byte-aligned stack, as
    // the System V ABI expects, and `ap_main` never returns.
    unsafe {
        asm!(
            "mov cr3, {root}",
            "mov rsp, {top}",
            "xor ebp, ebp",
            "call {main}",
            "ud2",
            root = in(reg) root,
            top = in(reg) top,
            main = sym ap_main,
            in("rdi") index,
            options(noreturn),
        );
    }
}

/// The AP's code on the kernel's tables and its own stack.
extern "C" fn ap_main(index: usize) -> ! {
    // The IST stacks of this CPU are mapped only in the kernel's tables.
    idt::load();
    cpu::enable_write_protect();
    cpu::enable_global_pages();
    let want = UserProtections {
        smep: WANT_SMEP.load(Ordering::Relaxed),
        smap: WANT_SMAP.load(Ordering::Relaxed),
        umip: WANT_UMIP.load(Ordering::Relaxed),
    };
    let expected = SLOTS
        .get(index)
        .map_or(u32::MAX, |s| s.lapic_id.load(Ordering::Relaxed));
    if cpu::initial_apic_id() != expected || !cpu::apply_user_protections(want) {
        REFUSED.fetch_add(1, Ordering::Release);
        cpu::halt_forever();
    }
    cpu::enable_user_sse();
    ONLINE.fetch_add(1, Ordering::Release);
    cpu::halt_forever();
}
