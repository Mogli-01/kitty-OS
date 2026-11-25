use crate::arch::irq::{self, Vector};
use crate::arch::legacy::{self, LegacyDevice, LegacyPorts};
use crate::kprintln;
use crate::sync::IrqSpinLock;
use kitty_ps2::Keyboard;

const IRQ_LINE: u8 = 1;
static KEYBOARD: IrqSpinLock<Option<Keyboard<LegacyPorts>>> = IrqSpinLock::new(None);

fn on_irq1(_: Vector) {
    let mut state = KEYBOARD.lock();
    let Some(keyboard) = state.as_mut() else {
        return;
    };
    keyboard.on_interrupt();

    #[cfg(feature = "selftest-ps2-keyboard")]
    {
        let input = keyboard.next_input();
        drop(state);
        if let Some(input) = input {
            use kitty_ps2::KeyboardInput;
            match input {
                KeyboardInput::Key(event) => crate::console::debug_print(format_args!(
                    "[kbd-test] {:?} {:?}\n",
                    event.key, event.action
                )),
                KeyboardInput::Lost => {
                    crate::console::debug_print(format_args!("[kbd-test] input lost\n"))
                }
            }
        }
    }
}

pub(super) fn init() {
    let Some(ports) = legacy::claim(LegacyDevice::I8042) else {
        kprintln!("[kbd] i8042 ports unavailable");
        return;
    };
    let keyboard = match Keyboard::initialize(ports) {
        Ok(keyboard) => keyboard,
        Err(error) => {
            kprintln!("[kbd] i8042 initialization failed: {:?}", error);
            return;
        }
    };
    // The boot CPU still has interrupts disabled. The handler can run only
    // after the device state has been installed and boot enables interrupts.
    match irq::register_isa(IRQ_LINE, on_irq1) {
        Ok(_) => {
            *KEYBOARD.lock() = Some(keyboard);
            kprintln!("[kbd] PS/2 keyboard ready on IRQ1");
        }
        Err(error) => {
            let _ = keyboard.stop();
            kprintln!("[kbd] IRQ1 registration failed: {:?}", error);
        }
    }
}
