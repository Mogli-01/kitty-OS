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
//! **Rule: release nested guards in reverse order (LIFO).** Each guard
//! remembers the state it found. `let a = A.lock(); let b = B.lock();
//! drop(a);` would re-enable interrupts while `B` is still held, and an
//! interrupt handler taking `B` would then deadlock. Normal scoping (guards
//! dropped at the end of their block) is always LIFO; only explicit
//! out-of-order `drop` breaks it. A per-CPU nesting counter will replace
//! this rule when per-CPU data exists (Phase 9).
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
///
/// **Not `Send`**: `were_enabled` is the interrupt flag of the CPU that took
/// the lock, and `Drop` restores it on the CPU that runs it. Moving the
/// guard to another CPU would re-enable interrupts on the wrong one (and
/// leave them off on the original). It is `!Send` because the inner
/// `SpinLockGuard` is, and `Sync` only when `T: Sync` (same reason as
/// `SpinLockGuard`); the compile-time assertions below keep it that way.
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

// Compile-time proof of the auto-trait rules above: these functions only
// type-check while the guards are !Send and Sync requires T: Sync. If a
// future change made a guard Send, the `impl` below would conflict with the
// blanket one and the build would fail.
#[allow(dead_code)]
mod auto_traits {
    use super::IrqSpinLockGuard;
    use crate::sync::SpinLockGuard;

    trait AmbiguousIfSend<A> {
        fn some_item() {}
    }
    impl<T: ?Sized> AmbiguousIfSend<()> for T {}
    struct Invalid;
    impl<T: ?Sized + Send> AmbiguousIfSend<Invalid> for T {}

    fn guards_are_not_send() {
        // Fails to compile ("type annotations needed") if the type is Send.
        <SpinLockGuard<'static, u32> as AmbiguousIfSend<_>>::some_item();
        <IrqSpinLockGuard<'static, u32> as AmbiguousIfSend<_>>::some_item();
    }

    trait AmbiguousIfSync<A> {
        fn some_item() {}
    }
    impl<T: ?Sized> AmbiguousIfSync<()> for T {}
    impl<T: ?Sized + Sync> AmbiguousIfSync<Invalid> for T {}

    fn guard_of_cell_is_not_sync() {
        // `Cell` is Send but not Sync: the guard must not be Sync either.
        <SpinLockGuard<'static, core::cell::Cell<u32>> as AmbiguousIfSync<_>>::some_item();
        <IrqSpinLockGuard<'static, core::cell::Cell<u32>> as AmbiguousIfSync<_>>::some_item();
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
