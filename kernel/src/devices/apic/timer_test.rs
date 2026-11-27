//! Exercises the local timer interrupt and the core's EOI path in QEMU.

use core::num::NonZeroU32;
use core::sync::atomic::{AtomicU8, Ordering};

use kitty_apic::local::{Error, LocalApic, TimerDivider, TimerMode};

use super::{CONTROLLER, LocalApicRegisters, Vector, irq};

// Both one-shot countdowns complete during the existing QEMU PIT boot test.
const COUNT: NonZeroU32 = NonZeroU32::new(25_000).unwrap();
static VECTOR: AtomicU8 = AtomicU8::new(0);
static DELIVERIES: AtomicU8 = AtomicU8::new(0);

pub(super) fn start() {
    let Ok(vector) = irq::register_local(on_timer) else {
        panic!("could not reserve the local APIC timer test vector");
    };
    VECTOR.store(vector.0, Ordering::Relaxed);
    let started = with_timer(|local| {
        local.configure_timer(vector.0, TimerMode::OneShot, TimerDivider::By1)?;
        local.unmask_timer()?;
        local.start_timer(COUNT)
    });
    assert!(started.is_ok(), "could not start the local APIC timer test");
}

fn on_timer(vector: Vector) {
    if DELIVERIES.fetch_add(1, Ordering::Relaxed) == 0 {
        assert!(
            with_timer(|local| local.start_timer(COUNT)).is_ok(),
            "could not rearm the local APIC timer"
        );
    } else {
        assert!(
            with_timer(LocalApic::stop_timer).is_ok(),
            "could not stop the local APIC timer"
        );
        assert!(
            irq::unregister_local(vector).is_ok(),
            "could not release the local APIC timer vector"
        );
    }
}

pub(super) fn acknowledged(vector: Vector) {
    if vector.0 == VECTOR.load(Ordering::Relaxed) && DELIVERIES.load(Ordering::Relaxed) == 2 {
        VECTOR.store(0, Ordering::Relaxed);
        crate::console::debug_print(format_args!(
            "[apic] two local timer deliveries acknowledged\n"
        ));
    }
}

fn with_timer(
    work: impl FnOnce(&mut LocalApic<LocalApicRegisters>) -> Result<(), Error>,
) -> Result<(), Error> {
    let mut guard = CONTROLLER.0.lock();
    let state = guard.as_mut().ok_or(Error::DeliveryNotEnabled)?;
    work(&mut state.local)
}
