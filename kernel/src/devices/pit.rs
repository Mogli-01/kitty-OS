//! Connects the legacy timer to the scheduler's interrupt epilogue.

use kitty_pit::{Period, Pit};

use crate::arch::irq::{self, Vector};
use crate::arch::legacy::{self, LegacyDevice, LegacyPorts};
use crate::sync::IrqSpinLock;
use crate::{kprintln, sched};

const IRQ_LINE: u8 = 0;
pub(super) const TICK_HZ: u32 = 100;
const PERIOD: Period = match Period::from_hz(TICK_HZ) {
    Some(period) => period,
    None => panic!("invalid scheduler timer rate"),
};

// Retain the exclusive port grant for the lifetime of the timer.
static TIMER: IrqSpinLock<Option<Pit<LegacyPorts>>> = IrqSpinLock::new(None);

fn on_irq0(_: Vector) {
    #[cfg(all(feature = "selftest-pit-driver", feature = "qemu-exit"))]
    super::timer_probe::count_irq();
    sched::tick();
}

pub(super) fn init() {
    let Some(ports) = legacy::claim(LegacyDevice::Pit) else {
        kprintln!("[pit] timer ports unavailable");
        return;
    };
    let mut pit = Pit::new(ports);
    if let Err(error) = pit.start_periodic(PERIOD) {
        let _ = pit.stop();
        kprintln!("[pit] timer programming failed: {:?}", error);
        return;
    }
    *TIMER.lock() = Some(pit);
    if let Err(error) = irq::register_isa(IRQ_LINE, on_irq0) {
        let pit = TIMER.lock().take();
        if let Some(mut pit) = pit {
            let _ = pit.stop();
        }
        kprintln!("[pit] IRQ0 registration failed: {:?}", error);
        return;
    }
    kprintln!(
        "[pit] scheduler timer ready on IRQ0 ({} Hz requested)",
        TICK_HZ
    );

    #[cfg(all(feature = "selftest-pit-driver", feature = "qemu-exit"))]
    if let Err(error) = sched::spawn("pit-probe", || super::timer_probe::verify_preemption("pit")) {
        kprintln!("[pit] probe thread unavailable: {:?}", error);
    }
}
