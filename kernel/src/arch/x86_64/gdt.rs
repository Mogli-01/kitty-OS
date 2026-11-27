//! Global Descriptor Table and Task State Segment.
//!
//! In 64-bit mode segmentation is almost gone: bases and limits are ignored
//! for CS/DS/ES/SS. What still matters:
//! - **CS** selects the privilege level (ring 0 vs ring 3) and long mode (L bit).
//! - **The TSS** tells the CPU which stack to switch to on an interrupt:
//!   `RSP0` when coming from ring 3 (Phase 6) and the **IST** stacks, which
//!   are used for specific vectors *no matter what* the current stack is.
//!
//! IST is the reason this phase exists security-wise: if the kernel stack
//! overflows (or RSP is corrupted), a fault cannot push its frame and the
//! CPU raises a double fault. Without IST the double fault would try the same
//! broken stack and the machine triple-faults (resets) with no diagnostics.
//! With a dedicated, known-good IST stack we always get a report.
//!
//! Layout (order matters for `syscall`/`sysret` in Phase 6: SYSRET loads
//! SS = STAR[63:48] + 8 and CS = STAR[63:48] + 16, so user data must come
//! right before user code):
//!
//! | index | selector | descriptor            |
//! |-------|----------|-----------------------|
//! | 0     | 0x00     | null (required)       |
//! | 1     | 0x08     | kernel code, 64-bit   |
//! | 2     | 0x10     | kernel data           |
//! | 3     | 0x18 \|3 | user data (Phase 6)   |
//! | 4     | 0x20 \|3 | user code, 64-bit     |
//! | 5–6   | 0x28     | TSS (16-byte system descriptor) |

use core::arch::asm;
use core::cell::UnsafeCell;
use core::mem::size_of;
use core::sync::atomic::{AtomicBool, Ordering};

use super::stacks;

/// Kernel code segment selector.
pub const KERNEL_CS: u16 = 0x08;
/// Kernel data/stack segment selector.
pub const KERNEL_DS: u16 = 0x10;
/// User data/stack segment selector (RPL 3).
pub const USER_DS: u16 = 0x18 | 3;
/// User code segment selector (RPL 3).
pub const USER_CS: u16 = 0x20 | 3;
/// TSS selector.
pub const TSS_SEL: u16 = 0x28;

/// IST slot (1-based, as the IDT encodes it) used by each critical vector.
pub const IST_DOUBLE_FAULT: u8 = 1;
/// IST slot for NMI: an NMI can arrive at *any* instruction, including the
/// few where RSP is not valid yet (e.g. right after `syscall` in Phase 6).
pub const IST_NMI: u8 = 2;
/// IST slot for machine check, same reasoning as NMI.
pub const IST_MACHINE_CHECK: u8 = 3;

// Descriptor encodings. Access byte: P (present), DPL, S (code/data), type.
// Flags nibble: G (4 KiB granularity), L (64-bit code) or DB (32-bit data).
//
// The "accessed" bit (bit 40) is set in advance on purpose: the CPU writes
// it into the GDT the first time a segment is loaded. Pre-setting it means
// the CPU never needs to write the GDT, so the GDT page can be mapped
// read-only (Phase 3) without loads of SS/DS faulting.
const KERNEL_CODE: u64 = 0x00AF_9B00_0000_FFFF; // P, DPL0, exec/read, A, L=1
const KERNEL_DATA: u64 = 0x00CF_9300_0000_FFFF; // P, DPL0, read/write, A
const USER_DATA: u64 = 0x00CF_F300_0000_FFFF; // P, DPL3, read/write, A
const USER_CODE: u64 = 0x00AF_FB00_0000_FFFF; // P, DPL3, exec/read, A, L=1

/// 64-bit Task State Segment (Intel SDM Vol. 3, 8.7).
#[repr(C, packed(4))]
struct Tss {
    _reserved0: u32,
    /// Stack pointers loaded on a privilege change to ring 0/1/2.
    rsp: [u64; 3],
    _reserved1: u64,
    /// Interrupt Stack Table: IST1..IST7.
    ist: [u64; 7],
    _reserved2: u64,
    _reserved3: u16,
    /// Offset of the I/O permission bitmap. Pointing it at the end of the
    /// TSS means "no bitmap": with IOPL=0, ring 3 can never use `in`/`out`.
    iomap_base: u16,
}

const TSS_SIZE: usize = size_of::<Tss>();
const _: () = assert!(TSS_SIZE == 104);

/// The GDT gets a 4 KiB page of its own so that the page can be mapped
/// read-only once it is loaded (after `ltr` has set the TSS busy bit, the
/// CPU never writes it again).
#[repr(C, align(4096))]
struct GdtPage([u64; 7]);

/// Memory the CPU reads (and, for the TSS busy bit, *writes*) behind the
/// compiler's back. `UnsafeCell` tells Rust it may change, so it is never
/// treated as immutable data.
struct CpuTables {
    gdt: UnsafeCell<GdtPage>,
    tss: UnsafeCell<Tss>,
}

// SAFETY: the GDT is written exactly once, by the single caller that wins
// the `INITIALIZED` flag in `init`, before the CPU is told about it. The
// TSS is written by `init` and afterwards only its RSP0 field, by
// `set_kernel_stack`, on one CPU with interrupts disabled.
unsafe impl Sync for CpuTables {}

static TABLES: CpuTables = CpuTables {
    gdt: UnsafeCell::new(GdtPage([0; 7])),
    tss: UnsafeCell::new(Tss {
        _reserved0: 0,
        rsp: [0; 3],
        _reserved1: 0,
        ist: [0; 7],
        _reserved2: 0,
        _reserved3: 0,
        iomap_base: TSS_SIZE as u16,
    }),
};

static INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Per-CPU block that the kernel's GS base points at (Phase 6b, D-060).
/// The `syscall` entry (6c) uses it through GS: it saves the user RSP in
/// slot [`PERCPU_USER_RSP`] and loads the kernel stack from slot
/// [`PERCPU_KERNEL_RSP`], because `syscall` does not switch stacks.
#[repr(C, align(64))]
pub struct PerCpu {
    slots: [u64; 8],
}

/// Byte offset (from GS) of the scratch slot for the user RSP.
pub const PERCPU_USER_RSP: usize = 0;
/// Byte offset (from GS) of the current thread's kernel stack top.
pub const PERCPU_KERNEL_RSP: usize = 8;

/// The block, written only by `set_kernel_stack` (IF=0) and by the
/// `syscall` entry (IF=0, before the stack switch).
struct PerCpuCell(UnsafeCell<PerCpu>);

// SAFETY: one CPU; every access happens with interrupts disabled (the
// scheduler's `set_kernel_stack`, and the `syscall` entry, which runs with
// IF cleared by FMASK), so there are never two concurrent accesses.
unsafe impl Sync for PerCpuCell {}

/// The boot CPU's block (Phase 9 / SMP: one per CPU).
static PERCPU: PerCpuCell = PerCpuCell(UnsafeCell::new(PerCpu { slots: [0; 8] }));

/// Address the kernel GS base must hold while the kernel runs.
pub fn percpu_address() -> u64 {
    PERCPU.0.get().addr() as u64
}

/// Operand of `lgdt`/`lidt`: 16-bit limit followed by a 64-bit base.
#[repr(C, packed)]
pub(super) struct DescriptorTablePointer {
    pub limit: u16,
    pub base: u64,
}

/// Address of the (page-aligned) GDT page, to be mapped read-only.
pub fn table_page() -> u64 {
    TABLES.gdt.get().addr() as u64
}

/// Builds the 16-byte TSS system descriptor (type 0x9 = available 64-bit TSS).
fn tss_descriptor(base: u64) -> (u64, u64) {
    let limit = (TSS_SIZE - 1) as u64;
    let low = (limit & 0xFFFF)
        | ((base & 0xFF_FFFF) << 16)
        | (0x89 << 40) // P=1, DPL=0, type=0b1001
        | (((limit >> 16) & 0xF) << 48)
        | (((base >> 24) & 0xFF) << 56);
    let high = base >> 32;
    (low, high)
}

/// Installs the kernel GDT and TSS on the current CPU and reloads all
/// segment registers. Only the first call does anything.
///
/// Phase 9 (SMP) will need one GDT/TSS per CPU.
pub fn init() {
    if INITIALIZED.swap(true, Ordering::AcqRel) {
        return;
    }

    let tss_ptr = TABLES.tss.get();
    let gdt_ptr = TABLES.gdt.get().cast::<[u64; 7]>();

    // IST stacks live in `stacks.rs`, each with an unmapped guard page.
    let mut ist = [0u64; 7];
    for (slot, top) in ist.iter_mut().take(stacks::IST_COUNT).enumerate() {
        *top = stacks::ist_top(slot).unwrap_or(0);
    }

    // SAFETY: we won the INITIALIZED flag, so no other code accesses the TSS
    // and the CPU does not know about it yet (no `ltr` executed). The
    // pointer comes from a live static, so it is valid and aligned.
    unsafe {
        (*tss_ptr).ist = ist;
    }

    let (tss_low, tss_high) = tss_descriptor(tss_ptr.addr() as u64);
    let gdt = [
        0,
        KERNEL_CODE,
        KERNEL_DATA,
        USER_DATA,
        USER_CODE,
        tss_low,
        tss_high,
    ];
    // SAFETY: same exclusivity argument as for the TSS: the GDT is not
    // loaded yet and only this call can reach this line.
    unsafe {
        gdt_ptr.write(gdt);
    }

    let pointer = DescriptorTablePointer {
        limit: (size_of::<[u64; 7]>() - 1) as u16,
        base: gdt_ptr.addr() as u64,
    };

    // SAFETY: `pointer` describes a valid, 'static GDT whose kernel code and
    // data descriptors match what the kernel already runs with (ring 0,
    // 64-bit), so reloading CS/SS/DS cannot change privilege or mode.
    // The far return (`retfq`) is the only way to reload CS in long mode:
    // it pops RIP then CS. The TSS descriptor is valid and not busy.
    unsafe {
        asm!(
            "lgdt [{ptr}]",
            "push {cs}",
            "lea {tmp}, [rip + 2f]",
            "push {tmp}",
            "retfq",
            "2:",
            "mov ds, {ds:x}",
            "mov es, {ds:x}",
            "mov ss, {ds:x}",
            "mov fs, {ds:x}",
            "mov gs, {ds:x}",
            "ltr {tss:x}",
            ptr = in(reg) &pointer,
            cs = in(reg) u64::from(KERNEL_CS),
            ds = in(reg) u64::from(KERNEL_DS),
            tss = in(reg) u64::from(TSS_SEL),
            // `out`, not `lateout`: tmp is written while inputs are still live.
            tmp = out(reg) _,
            options(preserves_flags),
        );
    }

    // SAFETY: `PERCPU` is a 'static block; loading GS above reset the GS
    // base to 0, and nothing in the kernel uses GS before this.
    unsafe { super::cpu::set_gs_bases(percpu_address()) };
}

/// Sets `TSS.RSP0`, the stack the CPU switches to when an interrupt or
/// exception arrives while ring 3 runs. The scheduler calls it on every
/// switch with the top of the next thread's kernel stack, so a trap from
/// user space always lands on the stack of the thread that was running.
///
/// # Safety
/// Interrupts must be disabled, and `top` must be the top of the kernel
/// stack of the thread about to run (or the bottom of a stack whose guard
/// page is below it, so that a stray trap from ring 3 double-faults): the
/// next trap from ring 3 writes its frame there.
pub(crate) unsafe fn set_kernel_stack(top: u64) {
    assert!(
        !super::cpu::interrupts_enabled(),
        "gdt: RSP0 changed with interrupts enabled"
    );
    if !INITIALIZED.load(Ordering::Acquire) {
        return;
    }
    let tss = TABLES.tss.get();
    // SAFETY: `tss` points to the live, initialised TSS static; taking a raw
    // pointer to one of its fields creates no reference.
    let rsp0 = unsafe { &raw mut (*tss).rsp[0] };
    // SAFETY: on a single CPU with interrupts disabled nothing else reads or
    // writes the TSS now (the CPU only reads RSP0 when a trap from ring 3 is
    // delivered, which cannot happen with IF=0). The field is only 4-byte
    // aligned in the packed struct, hence the unaligned write.
    unsafe { rsp0.write_unaligned(top) };
    let slot = PERCPU
        .0
        .get()
        .cast::<u64>()
        .wrapping_add(PERCPU_KERNEL_RSP / 8);
    // SAFETY: `slot` is inside the live per-CPU block; with IF=0 on one CPU
    // the `syscall` entry (the only other user) cannot run concurrently.
    unsafe { slot.write_volatile(top) };
}
