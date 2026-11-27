//! Monotonic time from the scheduler tick (issue #87, D-062).
//!
//! The timer driver that drives the scheduler (PIT or local APIC, Codex)
//! says once how fast it ticks with [`set_tick_rate`]; from then on
//! [`monotonic_ms`] is `ticks * 1000 / rate`. Resolution is one tick
//! (10 ms at 100 Hz), enough for animations and timeouts. Before a timer is
//! running there is no time: `None`, never a made-up value.
//!
//! Later (calibrated TSC) this gets nanosecond resolution behind the same
//! functions.

use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use crate::sched;

static TICK_HZ: AtomicU32 = AtomicU32::new(0);
/// Tick count when the rate was registered: time 0.
static ORIGIN: AtomicU64 = AtomicU64::new(0);

/// Called **only by the driver that calls `sched::tick`**, once its timer
/// runs at `hz` (1..=10_000; other values are ignored). The first call
/// wins: a later switch to another timer must keep the same rate.
pub fn set_tick_rate(hz: u32) {
    if (1..=10_000).contains(&hz)
        && TICK_HZ
            .compare_exchange(0, hz, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    {
        ORIGIN.store(sched::ticks(), Ordering::Release);
    }
}

/// Ticks per second, if a timer has registered.
pub fn tick_rate() -> Option<u32> {
    match TICK_HZ.load(Ordering::Acquire) {
        0 => None,
        hz => Some(hz),
    }
}

/// Milliseconds since the scheduler timer started, or `None` without one.
pub fn monotonic_ms() -> Option<u64> {
    let hz = u64::from(tick_rate()?);
    let ticks = sched::ticks().saturating_sub(ORIGIN.load(Ordering::Acquire));
    Some(ticks.saturating_mul(1000) / hz)
}

/// Lets other threads run until `ms` milliseconds have passed. Returns
/// `false` at once if there is no timer (callers must not loop forever on
/// it), or if called with interrupts disabled or a spinlock held (it would
/// re-enable interrupts inside someone's critical section).
pub fn sleep_ms(ms: u64) -> bool {
    if !crate::arch::x86_64::cpu::interrupts_enabled() || !crate::sync::preempt::enabled() {
        return false;
    }
    let Some(start) = monotonic_ms() else {
        return false;
    };
    let deadline = start.saturating_add(ms);
    while monotonic_ms().is_some_and(|now| now < deadline) {
        sched::yield_now();
        crate::arch::x86_64::cpu::enable_interrupts_and_halt();
    }
    true
}
