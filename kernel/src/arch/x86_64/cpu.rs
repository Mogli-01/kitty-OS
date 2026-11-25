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
    unsafe {
        asm!("int3", options(nomem, nostack));
    }
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
