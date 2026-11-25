//! Basic CPU control instructions.

use core::arch::asm;

/// Masks maskable interrupts on the current CPU (`cli`).
///
/// This is safe: disabling interrupts cannot break memory safety, it only
/// delays interrupt delivery.
#[inline]
pub fn disable_interrupts() {
    // SAFETY: `cli` only clears RFLAGS.IF. It touches no memory and no stack.
    // We do not claim `preserves_flags` because it changes IF.
    unsafe {
        asm!("cli", options(nomem, nostack));
    }
}

/// Stops the CPU until the next interrupt arrives (`hlt`).
#[inline]
pub fn halt() {
    // SAFETY: `hlt` just idles the core until an interrupt (or NMI/SMI)
    // arrives. It has no memory or flag side effects.
    unsafe {
        asm!("hlt", options(nomem, nostack, preserves_flags));
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

/// Physical base address of the local APIC registers (IA32_APIC_BASE).
pub fn local_apic_base() -> u64 {
    const MSR_APIC_BASE: u32 = 0x1B;
    rdmsr(MSR_APIC_BASE) & 0x000F_FFFF_FFFF_F000
}

/// Whether the CPU supports the NX (execute-disable) bit.
pub fn has_nx() -> bool {
    // CPUID leaf 0x8000_0001, EDX bit 20.
    let r = core::arch::x86_64::__cpuid(0x8000_0001);
    r.edx & (1 << 20) != 0
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
