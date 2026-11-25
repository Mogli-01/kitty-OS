//! A spinlock that also disables interrupts on the current CPU while held.
//!
//! Why: if code holding a normal spinlock is interrupted, and the interrupt
//! handler tries to take the same lock, the handler spins forever — the
//! holder cannot run again until the handler returns. Disabling interrupts
//! *before* taking the lock makes that impossible on the local CPU. Other
//! CPUs (Phase 9) still just spin until the holder releases it.
//!
//! The previous interrupt state is restored on release, so nested
//! `IrqSpinLock`s work: only the outermost guard re-enables interrupts.
//!
//! Use it for any data that interrupt handlers may touch (console, memory
//! allocators, IRQ tables). Keep critical sections short: interrupts are
//! delayed for their whole duration.

use core::mem::ManuallyDrop;
use core::ops::{Deref, DerefMut};

use super::spinlock::{SpinLock, SpinLockGuard};
use crate::arch::x86_64::cpu;

/// Spinlock with interrupts disabled while held.
pub struct IrqSpinLock<T> {
    inner: SpinLock<T>,
}

impl<T> IrqSpinLock<T> {
    /// Creates an unlocked lock.
    pub const fn new(value: T) -> Self {
        Self {
            inner: SpinLock::new(value),
        }
    }

    /// Disables interrupts, then spins until the lock is acquired.
    pub fn lock(&self) -> IrqSpinLockGuard<'_, T> {
        let were_enabled = cpu::interrupts_enabled();
        cpu::disable_interrupts();
        IrqSpinLockGuard {
            guard: ManuallyDrop::new(self.inner.lock()),
            were_enabled,
        }
    }

    /// Like [`lock`](Self::lock) but gives up if the lock is taken.
    /// Interrupts are left as they were if it fails.
    pub fn try_lock(&self) -> Option<IrqSpinLockGuard<'_, T>> {
        let were_enabled = cpu::interrupts_enabled();
        cpu::disable_interrupts();
        match self.inner.try_lock() {
            Some(guard) => Some(IrqSpinLockGuard {
                guard: ManuallyDrop::new(guard),
                were_enabled,
            }),
            None => {
                if were_enabled {
                    cpu::enable_interrupts();
                }
                None
            }
        }
    }
}

/// Proof that the lock is held (with interrupts off).
pub struct IrqSpinLockGuard<'a, T> {
    guard: ManuallyDrop<SpinLockGuard<'a, T>>,
    were_enabled: bool,
}

impl<T> Deref for IrqSpinLockGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.guard
    }
}

impl<T> DerefMut for IrqSpinLockGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.guard
    }
}

impl<T> Drop for IrqSpinLockGuard<'_, T> {
    fn drop(&mut self) {
        // Order matters: release the lock *first*, then re-enable
        // interrupts. The other way round, an interrupt arriving in between
        // could try to take the still-held lock and deadlock.
        // SAFETY: `guard` is dropped exactly once, here, and never used
        // afterwards (we are in the guard's own destructor).
        unsafe { ManuallyDrop::drop(&mut self.guard) };
        if self.were_enabled {
            cpu::enable_interrupts();
        }
    }
}

/// Boot self-test (run with interrupts enabled): interrupts must be off
/// while a guard is held, stay off when a nested guard is released, and
/// come back exactly as they were when the outer guard is released.
pub fn selftest() -> bool {
    let before = cpu::interrupts_enabled();
    let outer = IrqSpinLock::new(0u32);
    let inner = IrqSpinLock::new(());
    let guard = outer.lock();
    let off_inside = !cpu::interrupts_enabled();
    drop(inner.lock());
    let still_off = !cpu::interrupts_enabled();
    let busy = outer.try_lock().is_none() && !cpu::interrupts_enabled();
    drop(guard);
    before && off_inside && still_off && busy && cpu::interrupts_enabled()
}
