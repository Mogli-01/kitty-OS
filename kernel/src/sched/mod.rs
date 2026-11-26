//! Kernel threads and a preemptive round-robin scheduler (Phase 5,
//! docs/DECISIONS.md D-044/D-045).
//!
//! - Every thread has its own [`KernelStack`] (guard page below it) and is
//!   described, while not running, only by its saved stack pointer.
//! - [`yield_now`] gives the CPU to the next ready thread (cooperative).
//! - [`tick`] is called by the timer interrupt: it only *requests* a
//!   reschedule. The switch happens at the end of interrupt dispatch, after
//!   the EOI, and never while a plain `SpinLock` is held (`sync::preempt`).
//! - Threads that return from their entry function die; their stacks are
//!   freed by the next scheduling decision, outside the scheduler lock.
//!
//! Single CPU for now: the scheduler lock is released *before* the switch
//! while interrupts stay disabled, which is enough on one CPU. Phase 9
//! (SMP) needs per-CPU run queues and a lock held across the switch.
//!
//! `unsafe` is allowed here (D-044): switching stacks and turning the entry
//! address back into a function pointer cannot be done in safe Rust.

use alloc::boxed::Box;
use alloc::collections::{BTreeMap, VecDeque};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::arch::x86_64::{context, cpu};
use crate::mm::paging::MapError;
use crate::mm::stack::KernelStack;
use crate::sync::{IrqSpinLock, preempt};

/// Identifier of a kernel thread. 0 is the boot thread (it becomes the
/// idle thread).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ThreadId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Running,
    Ready,
    Dead,
}

struct Thread {
    name: &'static str,
    state: State,
    /// Saved RSP while not running.
    rsp: u64,
    /// `None` for the boot thread, which runs on the static main stack.
    /// Never read: it is owned so that dropping the thread frees the stack.
    #[expect(dead_code, reason = "owned only to free the stack on drop")]
    stack: Option<KernelStack>,
}

struct Scheduler {
    /// Boxed so a thread's `rsp` field never moves while a raw pointer to
    /// it is used by the switch.
    threads: BTreeMap<ThreadId, Box<Thread>>,
    ready: VecDeque<ThreadId>,
    current: ThreadId,
    next_id: u64,
    /// Threads that died; freed by a later `schedule` (never the running one).
    /// Boxed on purpose: `schedule` keeps a raw pointer to the dying
    /// thread's `rsp` while moving it here, so the `Thread` must not move.
    #[allow(clippy::vec_box)]
    dead: Vec<Box<Thread>>,
}

static SCHED: IrqSpinLock<Option<Scheduler>> = IrqSpinLock::new(None);
static NEED_RESCHED: AtomicBool = AtomicBool::new(false);
static SWITCHES: AtomicU64 = AtomicU64::new(0);

/// Errors from [`spawn`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpawnError {
    /// [`init`] has not run.
    NotInitialised,
    /// No memory for the stack or the thread.
    OutOfMemory,
}

impl From<MapError> for SpawnError {
    fn from(_: MapError) -> Self {
        SpawnError::OutOfMemory
    }
}

/// Turns the code currently running (the boot thread) into thread 0.
pub fn init() {
    let mut guard = SCHED.lock();
    if guard.is_some() {
        return;
    }
    let mut threads = BTreeMap::new();
    threads.insert(
        ThreadId(0),
        Box::new(Thread {
            name: "boot/idle",
            state: State::Running,
            rsp: 0,
            stack: None,
        }),
    );
    *guard = Some(Scheduler {
        threads,
        ready: VecDeque::new(),
        current: ThreadId(0),
        next_id: 1,
        dead: Vec::new(),
    });
}

/// Creates a kernel thread that runs `entry` and is queued as ready.
/// When `entry` returns, the thread exits.
pub fn spawn(name: &'static str, entry: fn()) -> Result<ThreadId, SpawnError> {
    let frame = context::initial_frame(entry as usize as u64);
    let stack = KernelStack::new(&frame)?;
    let rsp = stack.top() - (context::INITIAL_FRAME_WORDS as u64) * 8;
    let thread = Box::new(Thread {
        name,
        state: State::Ready,
        rsp,
        stack: Some(stack),
    });
    let mut guard = SCHED.lock();
    let s = guard.as_mut().ok_or(SpawnError::NotInitialised)?;
    let id = ThreadId(s.next_id);
    s.next_id += 1;
    s.threads.insert(id, thread);
    s.ready.push_back(id);
    Ok(id)
}

/// Gives the CPU to the next ready thread, if any.
pub fn yield_now() {
    schedule(false);
}

/// Called by the timer interrupt handler (Codex's timer driver): requests a
/// reschedule at the end of the current interrupt. Cheap and lock-free.
pub fn tick() {
    NEED_RESCHED.store(true, Ordering::Release);
}

/// Called by the interrupt core after the handler and the EOI, with
/// interrupts disabled. Switches if a reschedule was requested and no plain
/// spinlock is held (otherwise the request stays pending).
pub(crate) fn preempt_if_requested() {
    if preempt::enabled() && NEED_RESCHED.swap(false, Ordering::AcqRel) {
        schedule(false);
    }
}

/// Number of context switches so far.
pub fn switches() -> u64 {
    SWITCHES.load(Ordering::Relaxed)
}

/// Identifier of the running thread.
pub fn current() -> ThreadId {
    SCHED.lock().as_ref().map_or(ThreadId(0), |s| s.current)
}

/// Number of threads that exist (running, ready or not yet reaped).
pub fn thread_count() -> usize {
    SCHED.lock().as_ref().map_or(0, |s| s.threads.len())
}

/// First Rust code of every new thread (called by the assembly trampoline
/// with the entry address placed by `spawn`).
pub extern "C" fn thread_start(entry: u64) -> ! {
    // A new thread starts inside `schedule`'s critical section, with
    // interrupts disabled; it is now a normal running thread.
    cpu::enable_interrupts();
    // SAFETY: `entry` was produced in `spawn` from a `fn()` (`entry as
    // usize as u64`) and travelled unchanged through the initial stack
    // frame (r12) to here, so it is a valid `fn()` pointer.
    let f: fn() = unsafe { core::mem::transmute::<usize, fn()>(entry as usize) };
    f();
    exit()
}

/// Ends the current thread. Its stack is freed later, by another thread.
pub fn exit() -> ! {
    schedule(true);
    // `schedule(true)` never returns to a dead thread.
    cpu::halt_forever()
}

/// Picks the next thread and switches to it. `dying`: the current thread
/// exits instead of being re-queued.
fn schedule(dying: bool) {
    let were_enabled = cpu::interrupts_enabled();
    // Interrupts stay off from here until the switch is complete. Because
    // they are already off when the lock is taken, dropping the guard below
    // does not re-enable them.
    cpu::disable_interrupts();

    let (save, resume, reap) = {
        let mut guard = SCHED.lock();
        let Some(s) = guard.as_mut() else {
            drop(guard);
            restore(were_enabled);
            return;
        };
        let reap = core::mem::take(&mut s.dead);
        let Some(next) = s.ready.pop_front() else {
            drop(guard);
            drop(reap);
            restore(were_enabled);
            return;
        };
        let cur = s.current;
        s.current = next;

        let save: *mut u64 = if dying {
            match s.threads.remove(&cur) {
                Some(mut t) => {
                    t.state = State::Dead;
                    let ptr: *mut u64 = &raw mut t.rsp;
                    s.dead.push(t); // the Box keeps `rsp` at the same address
                    ptr
                }
                None => core::ptr::null_mut(),
            }
        } else {
            s.ready.push_back(cur);
            match s.threads.get_mut(&cur) {
                Some(t) => {
                    t.state = State::Ready;
                    &raw mut t.rsp
                }
                None => core::ptr::null_mut(),
            }
        };
        let resume = match s.threads.get_mut(&next) {
            Some(t) => {
                t.state = State::Running;
                t.rsp
            }
            None => 0,
        };
        (save, resume, reap)
    };

    // Free stacks of threads that died earlier (never the current one: a
    // thread is only in `reap` after another thread has run).
    drop(reap);

    if save.is_null() || resume == 0 {
        // Inconsistent bookkeeping is a kernel bug; refuse to jump anywhere.
        panic!("sched: corrupted thread table");
    }
    SWITCHES.fetch_add(1, Ordering::Relaxed);
    // SAFETY: interrupts are disabled (single CPU: nothing else touches the
    // scheduler until we resume). `save` points into a live boxed `Thread`
    // (in `threads` or `dead`, neither freed before the next `schedule`,
    // which cannot run before this switch completes). `resume` is the saved
    // RSP of a thread suspended by `switch` or a fresh stack built by
    // `spawn`, whose stack is still mapped, and that thread is not running.
    unsafe { context::switch(save, resume) };

    // Resumed: some other thread switched back to us.
    restore(were_enabled);
}

fn restore(were_enabled: bool) {
    if were_enabled {
        cpu::enable_interrupts();
    }
}

/// Boot self-test of cooperative scheduling: two threads ping-pong with
/// `yield_now` and exit; the boot thread waits for them. Returns the
/// average cost of one switch in TSC cycles, or `None` on failure.
pub fn selftest() -> Option<u64> {
    static ROUNDS_DONE: AtomicU64 = AtomicU64::new(0);
    static EXITED: AtomicU64 = AtomicU64::new(0);
    const ROUNDS: u64 = 1000;
    fn worker() {
        for _ in 0..ROUNDS {
            ROUNDS_DONE.fetch_add(1, Ordering::Relaxed);
            yield_now();
        }
        EXITED.fetch_add(1, Ordering::Release);
    }

    let threads_before = thread_count();
    let switches_before = switches();
    let start = cpu::rdtsc();
    spawn("selftest-a", worker).ok()?;
    spawn("selftest-b", worker).ok()?;
    let mut spins = 0u64;
    while EXITED.load(Ordering::Acquire) < 2 && spins < 10_000_000 {
        yield_now();
        spins += 1;
    }
    let cycles = cpu::rdtsc().saturating_sub(start);
    let switched = switches().saturating_sub(switches_before);
    // One more round so the dead threads are reaped.
    yield_now();
    yield_now();
    let ok = EXITED.load(Ordering::Acquire) == 2
        && ROUNDS_DONE.load(Ordering::Relaxed) == 2 * ROUNDS
        && thread_count() == threads_before
        && current() == ThreadId(0)
        && switched >= 2 * ROUNDS;
    ok.then(|| cycles / switched.max(1))
}

/// Name of the running thread (for logs).
pub fn current_name() -> &'static str {
    SCHED
        .lock()
        .as_ref()
        .and_then(|s| s.threads.get(&s.current).map(|t| t.name))
        .unwrap_or("?")
}
