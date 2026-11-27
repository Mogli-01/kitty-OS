//! Calibrates the boot CPU's local timer against the masked legacy PIT.

use core::num::NonZeroU32;
use kitty_apic::local::{LocalApic, TimerDivider, TimerMode};
use kitty_hal::PortBlock;
use kitty_pit::{INPUT_HZ, Period, Pit};

use super::{CONTROLLER, LocalApicRegisters, Vector, irq};
use crate::arch::legacy::{self, LegacyDevice};
use crate::devices::pit::TICK_HZ;
use crate::{kprintln, sched};

const REFERENCE: Period = match Period::from_hz(200) {
    Some(period) => period,
    None => panic!("invalid PIT calibration rate"),
};
const DIVIDER: TimerDivider = TimerDivider::By16;
const MAX_ATTEMPTS: usize = 3;
const MAX_POLLS_PER_RELOAD: usize = 65_536;

pub(in crate::devices) fn init() -> bool {
    if CONTROLLER.0.lock().is_none() {
        return false;
    }
    let Ok(vector) = irq::register_local(on_tick) else {
        kprintln!("[lapic] scheduler vector unavailable, using PIT");
        return false;
    };
    let configured = {
        let mut guard = CONTROLLER.0.lock();
        let Some(state) = guard.as_mut() else {
            panic!("boot interrupt controller disappeared");
        };
        let result = configure(&mut state.local, vector);
        if result.is_err() {
            assert!(
                state.local.stop_timer().is_ok(),
                "could not stop the local APIC timer before PIT fallback"
            );
        }
        result
    };
    match configured {
        Ok(count) => kprintln!("[lapic] scheduler timer ready ({TICK_HZ} Hz, count {count})"),
        Err(reason) => {
            assert!(
                irq::unregister_local(vector).is_ok(),
                "could not release LAPIC timer vector"
            );
            kprintln!("[lapic] {reason}, using PIT");
            return false;
        }
    }
    #[cfg(all(feature = "selftest-lapic-scheduler", feature = "qemu-exit"))]
    if let Err(error) = sched::spawn("lapic-probe", || {
        crate::devices::timer_probe::verify_preemption("lapic");
    }) {
        kprintln!("[lapic] probe thread unavailable: {:?}", error);
    }
    true
}

fn on_tick(_: Vector) {
    #[cfg(all(feature = "selftest-lapic-scheduler", feature = "qemu-exit"))]
    crate::devices::timer_probe::count_irq();
    sched::tick();
}

fn configure(
    local: &mut LocalApic<LocalApicRegisters>,
    vector: Vector,
) -> Result<NonZeroU32, &'static str> {
    local
        .configure_timer(vector.0, TimerMode::OneShot, DIVIDER)
        .and_then(|()| local.start_timer(NonZeroU32::MAX))
        .map_err(|_| "could not start masked calibration timer")?;
    let count = calibrate(local)?;
    local
        .configure_timer(vector.0, TimerMode::Periodic, DIVIDER)
        .and_then(|()| local.start_timer(count))
        .map_err(|_| "could not start periodic timer")?;
    local
        .unmask_timer()
        .map_err(|_| "could not unmask periodic timer")?;
    Ok(count)
}

fn calibrate(local: &LocalApic<LocalApicRegisters>) -> Result<NonZeroU32, &'static str> {
    let ports = legacy::claim(LegacyDevice::Pit).ok_or("PIT calibration ports unavailable")?;
    let mut pit = Pit::new(ports);
    let measured = pit
        .start_periodic(REFERENCE)
        .map_err(|_| "could not start PIT reference")
        .and_then(|()| measure(&mut pit, local));
    pit.stop().map_err(|_| "could not stop PIT reference")?;
    measured
}

fn measure(
    pit: &mut Pit<impl PortBlock>,
    local: &LocalApic<LocalApicRegisters>,
) -> Result<NonZeroU32, &'static str> {
    // A host or firmware pause can delay a sample. Retry inconsistent
    // intervals, but fall back immediately if the reference stops moving.
    let mut previous = read_pit(pit)?;
    for _ in 0..MAX_ATTEMPTS {
        let mut samples = [0; 3];
        for sample in &mut samples {
            wait_for_reload(pit, &mut previous)?;
            *sample = local.current_count().map_err(|_| "could not read LAPIC")?;
        }
        if let Some(count) = periodic_count(samples) {
            return Ok(count);
        }
    }
    Err("unstable or out-of-range calibration intervals")
}

fn periodic_count([start, middle, end]: [u32; 3]) -> Option<NonZeroU32> {
    let first = start.saturating_sub(middle);
    let second = middle.saturating_sub(end);
    if first == 0 || second == 0 || first.abs_diff(second) > first.max(second) / 8 {
        return None;
    }
    let numerator = (u64::from(first) + u64::from(second)) * u64::from(INPUT_HZ);
    let denominator = 2 * u64::from(REFERENCE.divisor()) * u64::from(TICK_HZ);
    let count = (numerator + denominator / 2) / denominator;
    u32::try_from(count).ok().and_then(NonZeroU32::new)
}

fn read_pit(pit: &mut Pit<impl PortBlock>) -> Result<u16, &'static str> {
    pit.latch_count()
        .map_err(|_| "could not read PIT reference")
}

fn wait_for_reload(pit: &mut Pit<impl PortBlock>, previous: &mut u16) -> Result<(), &'static str> {
    for _ in 0..MAX_POLLS_PER_RELOAD {
        let current = read_pit(pit)?;
        let reloaded = current > *previous && u32::from(current) <= REFERENCE.divisor();
        *previous = current;
        if reloaded {
            return Ok(());
        }
    }
    Err("PIT calibration timed out")
}
