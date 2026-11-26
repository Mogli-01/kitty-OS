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
use crate::sync::IrqSpinLock;

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
    /// If no other thread is runnable, the CPU idles with interrupts enabled
    /// (`sti; hlt`) instead of spinning, until an interrupt arrives.
    pub fn wait_until(&self, mut ready: impl FnMut() -> bool) {
        loop {
            let were_enabled = cpu::interrupts_enabled();
            cpu::disable_interrupts();
            if ready() {
                if were_enabled {
                    cpu::enable_interrupts();
                }
                return;
            }
            let me = super::current();
            {
                let mut q = self.waiters.lock();
                if !q.contains(&me) {
                    q.push_back(me);
                }
            }
            let switched = super::block_current();
            if !switched {
                // Nobody else can run: sleep until an interrupt (which may
                // be the one that makes `ready` true) and re-check.
                self.remove(me);
                cpu::enable_interrupts_and_halt();
            }
            if !were_enabled {
                cpu::disable_interrupts();
            }
        }
    }

    fn remove(&self, id: ThreadId) {
        self.waiters.lock().retain(|&w| w != id);
    }

    /// Wakes the longest-waiting thread. Returns whether a thread was woken.
    /// Usable from interrupt handlers.
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
    /// interrupt handlers.
    pub fn wake_all(&self) -> usize {
        let waiters = core::mem::take(&mut *self.waiters.lock());
        waiters.into_iter().filter(|&id| super::wake(id)).count()
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
    fn consumer() {
        QUEUE.wait_until(|| VALUE.load(Ordering::Acquire) != 0);
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
        && SEEN.load(Ordering::Acquire) == 0x006b_6974_7479
        && QUEUE.is_empty()
        && !QUEUE.wake_one()
}
