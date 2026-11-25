//! Synchronisation primitives, implemented from scratch.
//!
//! `unsafe` is allowed here (see `docs/UNSAFE.md`): a lock is, by definition,
//! the thing that turns shared `UnsafeCell` access into a safe `&mut T`.

pub mod spinlock;

pub use spinlock::{SpinLock, SpinLockGuard};
