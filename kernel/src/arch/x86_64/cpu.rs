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
