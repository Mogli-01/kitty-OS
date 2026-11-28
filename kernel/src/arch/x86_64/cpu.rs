//! Basic CPU control instructions.

use core::arch::asm;

// A note on asm options for cli/sti/hlt (same lesson as issue #7): none of
// these blocks may use `nomem`. They start or end a region where interrupt
// handlers can (or cannot) run, and handlers read and write Rust memory.
// Without `nomem` each block is a compiler memory barrier, so the compiler
// cannot move memory accesses into or out of a critical section.

/// Masks maskable interrupts on the current CPU (`cli`).
///
/// This is safe: disabling interrupts cannot break memory safety, it only
/// delays interrupt delivery.
#[inline]
pub fn disable_interrupts() {
    // SAFETY: `cli` only clears RFLAGS.IF; no stack use. Not `nomem` (see
    // above) and not `preserves_flags` (it changes IF).
    unsafe {
        asm!("cli", options(nostack));
    }
}

/// Unmasks maskable interrupts on the current CPU (`sti`).
///
/// Safe: every interrupt vector has a handler that saves and restores the
/// full register state, and all kernel state shared with handlers is behind
/// `IrqSpinLock` or atomics. Must only be used after `arch::init` (IDT
/// loaded), which the boot order guarantees.
#[inline]
pub fn enable_interrupts() {
    // SAFETY: see above. Not `nomem`: an interrupt can run right after this
    // instruction and touch memory.
    unsafe {
        asm!("sti", options(nostack));
    }
}

/// Whether maskable interrupts are currently enabled (RFLAGS.IF).
#[inline]
pub fn interrupts_enabled() -> bool {
    let rflags: u64;
    // SAFETY: `pushfq; pop` only copies RFLAGS through the stack slot it
    // pushes itself; it reads no Rust memory.
    unsafe {
        asm!("pushfq", "pop {}", out(reg) rflags, options(nomem, preserves_flags));
    }
    rflags & (1 << 9) != 0
}

/// Stops the CPU until the next interrupt arrives (`hlt`).
#[inline]
pub fn halt() {
    // SAFETY: `hlt` idles the core until an interrupt (or NMI/SMI) arrives.
    // Not `nomem`: the handler that wakes us may change memory we read next.
    unsafe {
        asm!("hlt", options(nostack, preserves_flags));
    }
}

/// Enables interrupts and halts **atomically**: `sti` takes effect only
/// after the next instruction, so no interrupt can slip in between and be
/// "lost" before `hlt` (the classic lost-wakeup race of `sti(); hlt()`).
#[inline]
pub fn enable_interrupts_and_halt() {
    // SAFETY: as for `enable_interrupts` and `halt`.
    unsafe {
        asm!("sti", "hlt", options(nostack));
    }
}

/// Idles forever with interrupts enabled, serving IRQs.
pub fn idle_forever() -> ! {
    loop {
        enable_interrupts_and_halt();
    }
}

/// Reads CR2: the linear address that caused the last page fault.
pub fn read_cr2() -> u64 {
    let value: u64;
    // SAFETY: reading CR2 has no side effects; we are in ring 0.
    unsafe {
        asm!("mov {}, cr2", out(reg) value, options(nomem, nostack, preserves_flags));
    }
    value
}

/// Executes `int3`, raising a breakpoint exception (#BP).
///
/// Used as a boot self-test of the IDT: the handler logs and resumes.
pub fn breakpoint() {
    // SAFETY: `int3` transfers control to the #BP handler, which saves and
    // restores every register and returns to the next instruction. Calling
    // this before the IDT is loaded would triple-fault, so `arch::init`
    // must run first (enforced by the boot order in kmain).
    // No `nostack`: the CPU pushes the exception frame on the current stack.
    // No `nomem`: the handler reads and writes memory (console state and its
    // lock), so the compiler must treat the block as a memory access.
    unsafe {
        asm!("int3");
    }
}

const MSR_EFER: u32 = 0xC000_0080;
const MSR_PAT: u32 = 0x277;
const EFER_NXE: u64 = 1 << 11;
const CR0_WP: u64 = 1 << 16;
const CR4_PGE: u64 = 1 << 7;

fn rdmsr(msr: u32) -> u64 {
    let (lo, hi): (u32, u32);
    // SAFETY: only called with architectural MSRs (EFER, PAT, APIC_BASE) that exist on
    // every x86_64 CPU; reading them has no side effects.
    unsafe {
        asm!("rdmsr", in("ecx") msr, out("eax") lo, out("edx") hi, options(nomem, nostack, preserves_flags));
    }
    (u64::from(hi) << 32) | u64::from(lo)
}

/// # Safety
/// Writing an MSR can change the memory model or the CPU mode; the caller
/// must guarantee the new value keeps the kernel's assumptions valid.
unsafe fn wrmsr(msr: u32, value: u64) {
    // SAFETY: forwarded from the caller.
    unsafe {
        asm!(
            "wrmsr",
            in("ecx") msr,
            in("eax") value as u32,
            in("edx") (value >> 32) as u32,
            options(nostack, preserves_flags),
        );
    }
}

const MSR_APIC_BASE: u32 = 0x1B;

/// Physical base address of the local APIC registers (IA32_APIC_BASE).
pub fn local_apic_base() -> u64 {
    rdmsr(MSR_APIC_BASE) & 0x000F_FFFF_FFFF_F000
}

/// State of the boot CPU's local APIC, from IA32_APIC_BASE.
#[derive(Clone, Copy, Debug)]
pub struct LocalApicState {
    /// Physical base of the xAPIC register page.
    pub base: u64,
    /// Bit 11: the APIC is globally enabled.
    pub enabled: bool,
    /// Bit 10: x2APIC mode (registers via MSRs, not MMIO).
    pub x2apic: bool,
}

/// Reads IA32_APIC_BASE.
pub fn local_apic_state() -> LocalApicState {
    let msr = rdmsr(MSR_APIC_BASE);
    LocalApicState {
        base: msr & 0x000F_FFFF_FFFF_F000,
        enabled: msr & (1 << 11) != 0,
        x2apic: msr & (1 << 10) != 0,
    }
}

/// Whether the CPU supports the NX (execute-disable) bit.
pub fn has_nx() -> bool {
    // CPUID leaf 0x8000_0001, EDX bit 20.
    let r = core::arch::x86_64::__cpuid(0x8000_0001);
    r.edx & (1 << 20) != 0
}

const CR4_UMIP: u64 = 1 << 11;
const CR4_SMEP: u64 = 1 << 20;
const CR4_SMAP: u64 = 1 << 21;

/// CPU protections enabled by [`enable_user_protections`].
#[derive(Clone, Copy, Debug)]
pub struct UserProtections {
    /// Supervisor Mode Execution Prevention: the kernel cannot execute
    /// user pages (stops "ret2user" exploits).
    pub smep: bool,
    /// Supervisor Mode Access Prevention: the kernel cannot read/write user
    /// pages except inside `copy_*_user` (stops kernel bugs from following
    /// user-controlled pointers).
    pub smap: bool,
    /// User-Mode Instruction Prevention: `sgdt`, `sidt`, `sldt`, `smsw` and
    /// `str` fault in ring 3 (they would leak kernel table addresses).
    pub umip: bool,
}

/// Enables SMEP, SMAP and UMIP when the CPU supports them (CPUID leaf 7).
/// Safe: each only *adds* restrictions; the kernel maps no user pages it
/// relies on executing or accessing directly (the W^X audit checks that
/// the kernel half has no user pages, and user memory is only touched by
/// `usercopy`, which opens SMAP for one instruction).
pub fn enable_user_protections() -> UserProtections {
    let leaf7 = core::arch::x86_64::__cpuid_count(7, 0);
    let wanted = UserProtections {
        smep: leaf7.ebx & (1 << 7) != 0,
        smap: leaf7.ebx & (1 << 20) != 0,
        umip: leaf7.ecx & (1 << 2) != 0,
    };
    let mut cr4: u64;
    // SAFETY: reading CR4 has no side effects.
    unsafe { asm!("mov {}, cr4", out(reg) cr4, options(nomem, nostack, preserves_flags)) };
    if wanted.smep {
        cr4 |= CR4_SMEP;
    }
    if wanted.smap {
        cr4 |= CR4_SMAP;
    }
    if wanted.umip {
        cr4 |= CR4_UMIP;
    }
    // SAFETY: only sets bits for features CPUID reports as supported (a
    // reserved bit would #GP). Each one restricts what code may do; none
    // breaks the kernel, which never runs, reads or writes user pages
    // outside `usercopy`. Not `nomem`: accesses must not move across it.
    unsafe { asm!("mov cr4, {}", in(reg) cr4, options(nostack, preserves_flags)) };
    if wanted.smap {
        super::usercopy::note_smap_enabled();
    }
    wanted
}

const MSR_GS_BASE: u32 = 0xC000_0101;
const MSR_STAR: u32 = 0xC000_0081;
const MSR_LSTAR: u32 = 0xC000_0082;
const MSR_FMASK: u32 = 0xC000_0084;
const MSR_SYSENTER_CS: u32 = 0x174;
const EFER_SCE: u64 = 1 << 0;

/// Turns on the `syscall` instruction (Phase 6c, D-063): EFER.SCE, the
/// segments (STAR), the entry point (LSTAR) and the RFLAGS bits cleared on
/// entry (FMASK).
///
/// # Safety
/// `entry` must be the address of the kernel's `syscall` entry routine,
/// and the GDT must have the layout STAR assumes (`gdt.rs`).
pub(crate) unsafe fn enable_syscall(entry: u64, star: u64, fmask: u64) {
    // SAFETY: STAR only names the selectors `syscall`/`sysret` load; the
    // caller guarantees they match the GDT.
    unsafe { wrmsr(MSR_STAR, star) };
    // SAFETY: LSTAR is where `syscall` jumps; the caller passes our entry.
    unsafe { wrmsr(MSR_LSTAR, entry) };
    // SAFETY: FMASK only selects RFLAGS bits to clear on entry.
    unsafe { wrmsr(MSR_FMASK, fmask) };
    // SAFETY: SYSENTER_CS = 0 makes `sysenter` raise #GP (kills the
    // process) instead of entering the kernel through an unprepared path.
    // CSTAR (32-bit `syscall`) stays 0: there is no 32-bit code segment in
    // the GDT, so compatibility mode is unreachable.
    unsafe { wrmsr(MSR_SYSENTER_CS, 0) };
    let efer = rdmsr(MSR_EFER);
    // SAFETY: with STAR/LSTAR/FMASK set, enabling SCE makes `syscall` enter
    // our entry routine instead of raising #UD.
    unsafe { wrmsr(MSR_EFER, efer | EFER_SCE) };
}
const MSR_KERNEL_GS_BASE: u32 = 0xC000_0102;
const CR0_MP: u64 = 1 << 1;
const CR0_EM: u64 = 1 << 2;
const CR0_TS: u64 = 1 << 3;
const CR4_OSFXSR: u64 = 1 << 9;
const CR4_OSXMMEXCPT: u64 = 1 << 10;
const CR0_NE: u64 = 1 << 5;
const CR4_FSGSBASE: u64 = 1 << 16;
const CR4_OSXSAVE: u64 = 1 << 18;

/// Installs the GS bases for the boot CPU (Phase 6b, D-060): the kernel's
/// GS base points at its per-CPU block, and the "other" GS base (what
/// `swapgs` hands to user space) is 0, so user code never sees a kernel
/// pointer in GS. Must run after the last `mov gs, …` (loading a selector
/// resets the base).
///
/// # Safety
/// `kernel_gs` must be the address of a per-CPU block that lives forever.
pub(crate) unsafe fn set_gs_bases(kernel_gs: u64) {
    // SAFETY: both MSRs exist on every x86_64 CPU; writing them only
    // changes what GS-relative accesses resolve to, and the kernel does not
    // use GS before this point (forwarded contract for the block).
    unsafe { wrmsr(MSR_GS_BASE, kernel_gs) };
    // SAFETY: as above; 0 is a valid (canonical) base for user space.
    unsafe { wrmsr(MSR_KERNEL_GS_BASE, 0) };
}

/// Current GS base (for the swapgs self-check: after entering the kernel
/// from ring 3 it must be the kernel's per-CPU block again).
pub fn gs_base() -> u64 {
    rdmsr(MSR_GS_BASE)
}

/// Enables SSE for **user** code (CR4.OSFXSR/OSXMMEXCPT, CR0.MP, no EM/TS).
/// The kernel itself is built soft-float and never touches the FPU/SSE
/// registers; user state is saved and restored per thread with
/// `fxsave`/`fxrstor` (`usermode`), so one process never sees another's
/// registers.
///
/// Explicitly **off** (the firmware or bootloader may have left them on):
/// - CR4.OSXSAVE: AVX state (upper YMM halves) is not covered by `fxsave`,
///   so with it on one process could read another's; AVX faults (#UD);
/// - CR4.FSGSBASE: `wrgsbase` would let user code choose the base that
///   `swapgs` hands to the kernel side.
///
/// CR0.NE on: x87 errors raise #MF (kills the process) instead of the
/// legacy FERR#/IRQ 13 path.
pub fn enable_user_sse() {
    let mut cr0: u64;
    // SAFETY: reading CR0 has no side effects.
    unsafe { asm!("mov {}, cr0", out(reg) cr0, options(nomem, nostack, preserves_flags)) };
    cr0 = (cr0 | CR0_MP | CR0_NE) & !(CR0_EM | CR0_TS);
    // SAFETY: MP/EM/TS only control how x87/SSE instructions behave; the
    // kernel executes none. Not `nomem`: ordered with the CR4 write below.
    unsafe { asm!("mov cr0, {}", in(reg) cr0, options(nostack, preserves_flags)) };
    let mut cr4: u64;
    // SAFETY: reading CR4 has no side effects.
    unsafe { asm!("mov {}, cr4", out(reg) cr4, options(nomem, nostack, preserves_flags)) };
    cr4 = (cr4 | CR4_OSFXSR | CR4_OSXMMEXCPT) & !(CR4_OSXSAVE | CR4_FSGSBASE);
    // SAFETY: FXSR and SSE exceptions are architectural on every x86_64 CPU,
    // so these bits are never reserved; clearing OSXSAVE/FSGSBASE only
    // removes features (the kernel uses neither XSAVE nor *GSBASE).
    unsafe { asm!("mov cr4, {}", in(reg) cr4, options(nostack, preserves_flags)) };
    let check: u64;
    // SAFETY: reading CR4 has no side effects.
    unsafe { asm!("mov {}, cr4", out(reg) check, options(nomem, nostack, preserves_flags)) };
    assert!(
        check & (CR4_OSXSAVE | CR4_FSGSBASE) == 0,
        "cpu: could not disable XSAVE/FSGSBASE for user space"
    );
}

/// Whether the CPU has RDRAND (CPUID.1:ECX bit 30).
pub fn has_rdrand() -> bool {
    core::arch::x86_64::__cpuid(1).ecx & (1 << 30) != 0
}

/// Whether the CPU has RDSEED (CPUID.7.0:EBX bit 18).
pub fn has_rdseed() -> bool {
    core::arch::x86_64::__cpuid_count(7, 0).ebx & (1 << 18) != 0
}

/// One 64-bit value from RDRAND (the CPU's DRBG), or `None` if the CPU
/// has none or it kept failing (carry clear) for 10 tries.
pub fn rdrand64() -> Option<u64> {
    if !has_rdrand() {
        return None;
    }
    for _ in 0..10 {
        let (value, ok): (u64, u8);
        // SAFETY: RDRAND exists (checked above); it only writes the
        // destination register and the flags, read back through `setc`.
        unsafe {
            asm!("rdrand {v}", "setc {ok}", v = out(reg) value, ok = out(reg_byte) ok,
                 options(nomem, nostack));
        }
        if ok != 0 {
            return Some(value);
        }
    }
    None
}

/// One 64-bit value from RDSEED (the CPU's entropy source, meant for
/// seeding), or `None` if absent or exhausted after 100 tries.
pub fn rdseed64() -> Option<u64> {
    if !has_rdseed() {
        return None;
    }
    for _ in 0..100 {
        let (value, ok): (u64, u8);
        // SAFETY: RDSEED exists (checked above); it only writes the
        // destination register and the flags.
        unsafe {
            asm!("rdseed {v}", "setc {ok}", v = out(reg) value, ok = out(reg_byte) ok,
                 options(nomem, nostack));
        }
        if ok != 0 {
            return Some(value);
        }
        core::hint::spin_loop();
    }
    None
}

/// Whether CR4.UMIP is set (`sgdt` & co. fault in ring 3).
pub fn umip_enabled() -> bool {
    let cr4: u64;
    // SAFETY: reading CR4 has no side effects.
    unsafe { asm!("mov {}, cr4", out(reg) cr4, options(nomem, nostack, preserves_flags)) };
    cr4 & CR4_UMIP != 0
}

/// Sets EFER.NXE so page-table entries can forbid instruction fetches.
///
/// Safe: it only *enables* a protection. Pages without the NX bit behave as
/// before. (Without NXE, bit 63 of a PTE is reserved and would fault.)
pub fn enable_nx() {
    let efer = rdmsr(MSR_EFER);
    if efer & EFER_NXE == 0 {
        // SAFETY: setting NXE only enables the NX bit in page tables; the
        // current (Limine) tables are valid with NXE on.
        unsafe { wrmsr(MSR_EFER, efer | EFER_NXE) };
    }
}

/// Sets CR0.WP: ring 0 must respect read-only pages too. Without it, the
/// kernel could write to its own `.text`/`.rodata` (and a kernel write
/// bug could rewrite code).
pub fn enable_write_protect() {
    let mut cr0: u64;
    // SAFETY: reading CR0 has no side effects.
    unsafe { asm!("mov {}, cr0", out(reg) cr0, options(nomem, nostack, preserves_flags)) };
    cr0 |= CR0_WP;
    // SAFETY: only adds the WP bit; it makes no currently valid access of
    // the kernel fail (the kernel never writes to read-only pages).
    unsafe { asm!("mov cr0, {}", in(reg) cr0, options(nostack, preserves_flags)) };
}

/// Sets CR4.PGE so mappings marked global survive CR3 reloads (the kernel
/// half stays in the TLB across process switches: fewer TLB misses).
pub fn enable_global_pages() {
    let mut cr4: u64;
    // SAFETY: reading CR4 has no side effects.
    unsafe { asm!("mov {}, cr4", out(reg) cr4, options(nomem, nostack, preserves_flags)) };
    cr4 |= CR4_PGE;
    // SAFETY: enabling PGE only changes TLB retention of global entries;
    // setting it also flushes the whole TLB.
    unsafe { asm!("mov cr4, {}", in(reg) cr4, options(nostack, preserves_flags)) };
}

/// PAT layout used by the kernel (D-020). PTE bits (PAT, PCD, PWT) select
/// one of 8 entries; we only use indices 0–3:
/// 0 = write-back (WB), 1 = write-combining (WC), 2 = UC-, 3 = uncached (UC).
/// Indices 4–7 repeat 0–3.
pub const PAT_VALUE: u64 = 0x0007_0106_0007_0106;

/// Programs the PAT MSR with [`PAT_VALUE`].
///
/// # Safety
/// Existing mappings that select PAT index 1 change memory type (UC/WT →
/// WC). The caller must switch to page tables built for the new layout
/// right after, before relying on the type of such mappings.
pub unsafe fn program_pat() {
    // SAFETY: WBINVD writes back and invalidates caches so no line with the
    // old memory type survives the change (Intel SDM 11.12.4). It has no
    // effect on program-visible memory contents.
    unsafe { asm!("wbinvd", options(nostack, preserves_flags)) };
    // SAFETY: forwarded from the caller's contract.
    unsafe { wrmsr(MSR_PAT, PAT_VALUE) };
}

/// Physical address of the current top-level page table (CR3).
pub fn read_cr3() -> u64 {
    let cr3: u64;
    // SAFETY: reading CR3 has no side effects.
    unsafe { asm!("mov {}, cr3", out(reg) cr3, options(nomem, nostack, preserves_flags)) };
    cr3 & 0x000F_FFFF_FFFF_F000
}

/// Loads a new top-level page table.
///
/// # Safety
/// `pml4` must be the physical address of a valid PML4 that maps the code
/// being executed, the current stack, and every kernel object in use, with
/// the right permissions.
pub unsafe fn write_cr3(pml4: u64) {
    // SAFETY: forwarded from the caller. Not `nomem`: changing the address
    // space must order all memory accesses around it.
    unsafe { asm!("mov cr3, {}", in(reg) pml4, options(nostack, preserves_flags)) };
}

/// Drops any TLB entry for the page containing `addr`.
///
/// Safe: invalidating a TLB entry can only make the CPU re-read the page
/// tables, never use a stale translation.
pub fn invlpg(addr: u64) {
    // SAFETY: see above. Not `nomem`: it must be ordered after the page
    // table write it follows.
    unsafe { asm!("invlpg [{}]", in(reg) addr, options(nostack, preserves_flags)) };
}

/// Reads the time-stamp counter (CPU cycles since reset, roughly).
pub fn rdtsc() -> u64 {
    // SAFETY: `rdtsc` only reads the time-stamp counter; it touches no
    // memory and is allowed in ring 0.
    unsafe { core::arch::x86_64::_rdtsc() }
}

/// Parks the current CPU forever.
///
/// Interrupts are disabled first, so only an NMI can wake the core; the loop
/// sends it straight back to `hlt`. Using `hlt` instead of a busy loop keeps
/// the host CPU (or the real core) idle.
pub fn halt_forever() -> ! {
    loop {
        disable_interrupts();
        halt();
    }
}
