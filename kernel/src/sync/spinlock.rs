//! A test-and-test-and-set spinlock.
//!
//! Phase 1 limitation: the lock does **not** disable interrupts yet. That is
//! fine while there are no interrupt handlers; Phase 4 adds an
//! interrupt-disabling variant (an interrupt handler that takes a lock the
//! interrupted code already holds would deadlock).

use core::cell::UnsafeCell;
use core::hint::spin_loop;
use core::marker::PhantomData;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicBool, Ordering};

/// Mutual exclusion by busy-waiting.
pub struct SpinLock<T> {
    locked: AtomicBool,
    data: UnsafeCell<T>,
}

// SAFETY: the lock guarantees that at most one `SpinLockGuard` (and hence one
// `&mut T`) exists at a time, so sharing `&SpinLock<T>` between CPUs is safe
// as long as `T` itself may be sent to another CPU (`T: Send`).
unsafe impl<T: Send> Sync for SpinLock<T> {}

impl<T> SpinLock<T> {
    /// Creates an unlocked lock.
    pub const fn new(value: T) -> Self {
        Self {
            locked: AtomicBool::new(false),
            data: UnsafeCell::new(value),
        }
    }

    /// Spins until the lock is acquired.
    ///
    /// Test-and-test-and-set: while the lock is taken we only *read* it
    /// (`load`), which keeps the cache line shared between cores instead of
    /// bouncing it with failed atomic writes. `spin_loop` emits `pause`,
    /// which saves power and avoids a memory-order pipeline flush on exit.
    pub fn lock(&self) -> SpinLockGuard<'_, T> {
        loop {
            if let Some(guard) = self.try_lock() {
                return guard;
            }
            while self.locked.load(Ordering::Relaxed) {
                spin_loop();
            }
        }
    }

    /// Acquires the lock only if it is free right now.
    pub fn try_lock(&self) -> Option<SpinLockGuard<'_, T>> {
        // Acquire on success: everything the previous holder wrote before its
        // Release store is visible to us. Relaxed on failure: we learned
        // nothing and touch no protected data.
        let guard = self
            .locked
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .ok()
            .map(|_| SpinLockGuard {
                lock: self,
                _not_send: PhantomData,
            });
        if guard.is_some() {
            // No preemption while any spinlock is held (see `preempt`).
            super::preempt::acquire();
        }
        guard
    }
}

/// Proof that the lock is held. Dropping it releases the lock.
///
/// Auto traits are chosen by hand (review of PR #15):
/// - **not `Send`**: a spinlock is held by the execution context that took
///   it; handing the guard to another thread/CPU (once the scheduler can
///   move tasks) would release the lock from the wrong place. The raw
///   pointer in `_not_send` removes both `Send` and `Sync`.
/// - **`Sync` only if `T: Sync`** (impl below): sharing `&guard` gives `&T`
///   to every thread that sees it. With the automatic impl (`T: Send` was
///   enough) a `T` such as `Cell` could be accessed from two threads at
///   once through `&guard` — a data race in safe code. Same rule as the
///   standard library's `MutexGuard`.
pub struct SpinLockGuard<'a, T> {
    lock: &'a SpinLock<T>,
    _not_send: PhantomData<*mut ()>,
}

// SAFETY: `&SpinLockGuard<T>` only gives out `&T` (through `Deref`), so it
// may be shared between threads exactly when `&T` may, i.e. `T: Sync`.
unsafe impl<T: Sync> Sync for SpinLockGuard<'_, T> {}

impl<T> Deref for SpinLockGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        // SAFETY: the guard exists only while `locked` is true and we are its
        // unique owner, so no `&mut T` can exist elsewhere.
        unsafe { &*self.lock.data.get() }
    }
}

impl<T> DerefMut for SpinLockGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        // SAFETY: as in `deref`; additionally `&mut self` guarantees this is
        // the only reference derived from this guard.
        unsafe { &mut *self.lock.data.get() }
    }
}

impl<T> Drop for SpinLockGuard<'_, T> {
    fn drop(&mut self) {
        // Release: our writes to the data happen-before the next Acquire.
        self.lock.locked.store(false, Ordering::Release);
        super::preempt::release();
    }
}
