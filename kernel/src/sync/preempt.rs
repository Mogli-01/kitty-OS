//! Preemption guard (Phase 5).
//!
//! Preempting a thread while it holds a plain `SpinLock` would be a
//! deadlock on one CPU: the next thread could spin forever on that lock
//! while its owner is not running. Every held `SpinLock` increments this
//! counter, and the scheduler only preempts when it is zero. (An
//! `IrqSpinLock` disables interrupts, so the timer cannot preempt its
//! holder in the first place.)
//!
//! One global counter is correct while there is one CPU; Phase 9 (SMP)
//! makes it per-CPU.

use core::sync::atomic::{AtomicU32, Ordering};

static HELD: AtomicU32 = AtomicU32::new(0);

pub(super) fn acquire() {
    HELD.fetch_add(1, Ordering::Acquire);
}

pub(super) fn release() {
    HELD.fetch_sub(1, Ordering::Release);
}

/// Whether the current context may be preempted (no plain spinlock held).
pub fn enabled() -> bool {
    HELD.load(Ordering::Acquire) == 0
}
