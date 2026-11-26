//! Wait queues: block a thread until a condition holds (Phase 5, GUI-PREP;
//! docs/DECISIONS.md D-046). The basis for IPC channels, blocking reads
//! from drivers and input events.
//!
//! ```text
//!  waiter:   queue.wait_until(|| data_ready())     // sleeps, no CPU used
//!  waker:    publish data; queue.wake_one()        // from a thread or an IRQ
//! ```
//!
//! **No lost wake-ups.** The waiter checks the condition, registers itself
//! and blocks with interrupts disabled. On one CPU nothing can run between
//! those steps, so a waker either runs before the check (the waiter sees
//! the condition true and does not sleep) or after the registration (the
//! waiter is woken). The condition is always re-checked after waking, so a
//! spurious wake-up is harmless. Phase 9 (SMP) will need the condition
//! check and registration under the queue lock instead.
//!
//! Safe Rust only (this file adds no `unsafe`).

use alloc::collections::VecDeque;

use super::ThreadId;
use crate::arch::x86_64::cpu;
use crate::sync::{IrqSpinLock, preempt};

/// A queue of threads waiting for something.
pub struct WaitQueue {
    waiters: IrqSpinLock<VecDeque<ThreadId>>,
}

impl Default for WaitQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl WaitQueue {
    /// An empty queue (usable in a `static`).
    pub const fn new() -> Self {
        Self {
            waiters: IrqSpinLock::new(VecDeque::new()),
        }
    }

    /// Blocks the current thread until `ready()` returns true. `ready` must
    /// be cheap and must not block or take a plain `SpinLock`; it may read
    /// atomics or take short `IrqSpinLock`s.
    ///
    /// Must be called from thread context with interrupts **enabled** and no
    /// lock held — never from an interrupt handler. Violations panic on
    /// entry, every time (not only when the condition happens to be false).
    /// Always returns with interrupts enabled.
    pub fn wait_until(&self, mut ready: impl FnMut() -> bool) {
        assert!(
            cpu::interrupts_enabled() && preempt::enabled(),
            "WaitQueue::wait_until: needs interrupts enabled and no lock held"
        );
        loop {
            cpu::disable_interrupts();
            if ready() {
                cpu::enable_interrupts();
                return;
            }
            let me = super::current();
            {
                let mut q = self.waiters.lock();
                if !q.contains(&me) {
                    q.push_back(me);
                }
            }
            if super::block_current() {
                // Woken. The switch does not carry RFLAGS, so interrupts are
                // still off here: turn them back on (we required them on).
                cpu::enable_interrupts();
            } else {
                // Only before the idle thread exists: nobody else can run.
                // Sleep until an interrupt (it may make `ready` true).
                self.remove(me);
                cpu::enable_interrupts_and_halt();
            }
        }
    }

    fn remove(&self, id: ThreadId) {
        self.waiters.lock().retain(|&w| w != id);
    }

    /// Wakes the longest-waiting thread. Returns whether a thread was woken.
    /// Usable from interrupt handlers (no allocation: `pop_front` keeps the
    /// buffer, and the scheduler reserved room in its ready queue).
    pub fn wake_one(&self) -> bool {
        loop {
            let Some(id) = self.waiters.lock().pop_front() else {
                return false;
            };
            // Skip stale entries (threads that already stopped waiting).
            if super::wake(id) {
                return true;
            }
        }
    }

    /// Wakes every waiting thread. Returns how many were woken. Usable from
    /// interrupt handlers: entries are popped one at a time, so the queue
    /// keeps its buffer (nothing is freed or allocated here), and the queue
    /// lock is released before each `wake` (lock order: queue, then SCHED,
    /// never nested).
    pub fn wake_all(&self) -> usize {
        let mut woken = 0;
        while let Some(id) = self.waiters.lock().pop_front() {
            if super::wake(id) {
                woken += 1;
            }
        }
        woken
    }

    /// Number of threads registered on the queue.
    pub fn len(&self) -> usize {
        self.waiters.lock().len()
    }

    /// Whether no thread is registered.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Boot self-test: a consumer blocks on a queue (and really is Blocked,
/// not spinning), a producer publishes and wakes it, the consumer sees the
/// value and exits. Returns whether everything happened in that order.
pub fn selftest() -> bool {
    use core::sync::atomic::{AtomicU64, Ordering};
    static QUEUE: WaitQueue = WaitQueue::new();
    static VALUE: AtomicU64 = AtomicU64::new(0);
    static SEEN: AtomicU64 = AtomicU64::new(0);
    static IF_AFTER_WAKE: AtomicU64 = AtomicU64::new(0);
    fn consumer() {
        QUEUE.wait_until(|| VALUE.load(Ordering::Acquire) != 0);
        // A woken thread must have interrupts enabled again (review bug).
        IF_AFTER_WAKE.store(u64::from(cpu::interrupts_enabled()), Ordering::Release);
        SEEN.store(VALUE.load(Ordering::Acquire), Ordering::Release);
    }

    let Ok(id) = super::spawn("waiter", consumer) else {
        return false;
    };
    // Let the consumer run until it blocks.
    for _ in 0..10 {
        super::yield_now();
    }
    let blocked = super::is_blocked(id) && QUEUE.len() == 1;
    // While blocked it must not be scheduled: yields do not wake it.
    for _ in 0..10 {
        super::yield_now();
    }
    let still_blocked = super::is_blocked(id) && SEEN.load(Ordering::Acquire) == 0;

    VALUE.store(0x006b_6974_7479, Ordering::Release); // "kitty"
    let woke = QUEUE.wake_one();
    let mut spins = 0;
    while SEEN.load(Ordering::Acquire) == 0 && spins < 1000 {
        super::yield_now();
        spins += 1;
    }
    super::yield_now(); // reap the consumer
    blocked
        && still_blocked
        && woke
        && IF_AFTER_WAKE.load(Ordering::Acquire) == 1
        && cpu::interrupts_enabled()
        && SEEN.load(Ordering::Acquire) == 0x006b_6974_7479
        && QUEUE.is_empty()
        && !QUEUE.wake_one()
}
