//! Synchronisation primitives, implemented from scratch.
//!
//! `unsafe` is allowed here (see `docs/UNSAFE.md`): a lock is, by definition,
//! the thing that turns shared `UnsafeCell` access into a safe `&mut T`.
//!
//! - [`SpinLock`]: plain spinlock. Only for data that interrupt handlers
//!   never touch.
//! - [`IrqSpinLock`]: also disables interrupts while held. Use it for
//!   anything an interrupt handler may access (all kernel globals today).

pub mod irq_spinlock;
pub mod spinlock;

pub use irq_spinlock::{IrqSpinLock, IrqSpinLockGuard};
pub use spinlock::{SpinLock, SpinLockGuard};
