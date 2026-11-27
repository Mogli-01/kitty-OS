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

use core::sync::atomic::{AtomicU32, Ordering};

use crate::sched;

static TICK_HZ: AtomicU32 = AtomicU32::new(0);

/// Called by the scheduler-timer driver once its timer runs. `hz` must be
/// in 1..=10_000; other values are ignored. The first call wins.
pub fn set_tick_rate(hz: u32) {
    if (1..=10_000).contains(&hz) {
        let _ = TICK_HZ.compare_exchange(0, hz, Ordering::AcqRel, Ordering::Acquire);
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
    Some(sched::ticks().saturating_mul(1000) / hz)
}

/// Lets other threads run until `ms` milliseconds have passed. Returns
/// `false` at once if there is no timer (callers must not loop forever on
/// it). Must be called from a thread with interrupts enabled.
pub fn sleep_ms(ms: u64) -> bool {
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
