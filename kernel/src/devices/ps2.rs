use crate::arch::irq::{self, Vector};
use crate::arch::legacy::{self, LegacyDevice, LegacyPorts};
use crate::kprintln;
use crate::sync::IrqSpinLock;
use kitty_ps2::{ControllerError, Ps2Input};

const KEYBOARD_IRQ: u8 = 1;
const AUXILIARY_IRQ: u8 = 12;
static DEVICES: IrqSpinLock<Option<Ps2Input<LegacyPorts>>> = IrqSpinLock::new(None);

fn on_keyboard_irq(_: Vector) {
    on_interrupt(KEYBOARD_IRQ);
}

fn on_auxiliary_irq(_: Vector) {
    on_interrupt(AUXILIARY_IRQ);
}

fn on_interrupt(_line: u8) {
    let mut state = DEVICES.lock();
    let Some(devices) = state.as_mut() else {
        return;
    };
    // The status byte determines the source because both IRQs share DATA.
    devices.on_interrupt();

    #[cfg(feature = "selftest-ps2-input")]
    {
        let event = devices.next_input();
        drop(state);
        log_test_event(_line, event);
    }
}

#[cfg(feature = "selftest-ps2-input")]
fn log_test_event(line: u8, event: Option<kitty_ps2::InputEvent>) {
    use kitty_ps2::InputEvent;
    match event {
        Some(InputEvent::Key(event)) => crate::console::debug_print(format_args!(
            "[kbd-test] {:?} {:?}\n",
            event.key, event.action
        )),
        Some(InputEvent::Mouse(report)) => crate::console::debug_print(format_args!(
            "[mouse-test] IRQ{line} dx={:?} dy={:?} left={} right={} middle={}\n",
            report.delta_x,
            report.delta_y,
            report.buttons.left,
            report.buttons.right,
            report.buttons.middle
        )),
        Some(InputEvent::Lost) => {
            crate::console::debug_print(format_args!(
                "[kbd-test] input lost\n[mouse-test] input lost\n"
            ));
        }
        None => {}
    }
}

pub(super) fn init() {
    let Some(ports) = legacy::claim(LegacyDevice::I8042) else {
        kprintln!("[kbd] i8042 ports unavailable");
        return;
    };
    let mut devices = match Ps2Input::initialize(ports) {
        Ok(devices) => devices,
        Err(error) => {
            kprintln!("[kbd] i8042 initialization failed: {:?}", error);
            return;
        }
    };
    let mut mouse_ready = match devices.initialize_mouse() {
        Ok(()) => true,
        Err(ControllerError::KeyboardRestoreFailed) => {
            let _ = devices.stop();
            kprintln!("[ps2] controller recovery failed");
            return;
        }
        Err(error) => {
            kprintln!("[mouse] AUX not enabled: {:?}", error);
            false
        }
    };

    // The boot CPU still has interrupts disabled. They are enabled after
    // both handlers have been registered and the device state is installed.
    if let Err(error) = irq::register_isa(KEYBOARD_IRQ, on_keyboard_irq) {
        let _ = devices.stop();
        kprintln!("[kbd] IRQ1 registration failed: {:?}", error);
        return;
    }
    if mouse_ready && let Err(error) = irq::register_isa(AUXILIARY_IRQ, on_auxiliary_irq) {
        kprintln!("[mouse] IRQ12 registration failed: {:?}", error);
        if devices.stop_mouse().is_err() {
            let _ = irq::unregister_isa(KEYBOARD_IRQ);
            let _ = devices.stop();
            kprintln!("[ps2] controller recovery failed");
            return;
        }
        mouse_ready = false;
    }

    *DEVICES.lock() = Some(devices);
    kprintln!("[kbd] PS/2 keyboard ready on IRQ1");
    if mouse_ready {
        kprintln!("[mouse] PS/2 mouse ready on IRQ12");
    }
}
