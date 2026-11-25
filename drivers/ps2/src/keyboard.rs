use kitty_hal::PortBlock;
use kitty_i8042::{Error, I8042, Source};

use crate::{Decoder, KeyEvent};

const EVENT_CAPACITY: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyboardInput {
    Key(KeyEvent),
    /// Input was lost. Clear any held keys before processing the next event.
    Lost,
}

struct Events {
    slots: [Option<KeyEvent>; EVENT_CAPACITY],
    head: usize,
    len: usize,
    lost: bool,
}

impl Events {
    const fn new() -> Self {
        Self {
            slots: [None; EVENT_CAPACITY],
            head: 0,
            len: 0,
            lost: false,
        }
    }

    fn mark_lost(&mut self) {
        self.head = 0;
        self.len = 0;
        self.lost = true;
    }

    fn push(&mut self, event: KeyEvent) {
        if self.len == EVENT_CAPACITY {
            self.mark_lost();
        }
        let tail = (self.head + self.len) % EVENT_CAPACITY;
        self.slots[tail] = Some(event);
        self.len += 1;
    }

    fn pop(&mut self) -> Option<KeyboardInput> {
        if core::mem::take(&mut self.lost) {
            return Some(KeyboardInput::Lost);
        }
        if self.len == 0 {
            return None;
        }
        let event = self.slots[self.head].take();
        self.head = (self.head + 1) % EVENT_CAPACITY;
        self.len -= 1;
        event.map(KeyboardInput::Key)
    }
}

/// The i8042 transport, Set 2 decoder and bounded input queue for one keyboard.
pub struct Keyboard<P> {
    controller: I8042<P>,
    decoder: Decoder,
    events: Events,
}

impl<P: PortBlock> Keyboard<P> {
    /// Configures the controller while IRQ1 is masked.
    pub fn initialize(ports: P) -> Result<Self, Error> {
        let mut controller = I8042::new(ports);
        controller.initialize_keyboard()?;
        Ok(Self {
            controller,
            decoder: Decoder::new(),
            events: Events::new(),
        })
    }

    /// Stops the device after its interrupt line has been masked.
    pub fn stop(mut self) -> Result<(), Error> {
        self.controller.stop_keyboard()
    }

    /// Services IRQ1 by reading at most one byte, without waiting or allocating.
    pub fn on_interrupt(&mut self) {
        match self.controller.try_read() {
            Ok(Some(output)) if output.source == Source::Keyboard => {
                if let Some(event) = self.decoder.push(output.value) {
                    self.events.push(event);
                }
            }
            Ok(_) | Err(Error::CorruptOutput(Source::Auxiliary)) => {}
            Err(_) => {
                self.decoder.reset();
                self.events.mark_lost();
            }
        }
    }

    pub fn next_input(&mut self) -> Option<KeyboardInput> {
        self.events.pop()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{KeyAction, KeyCode};
    use core::cell::Cell;

    const MAKE: KeyEvent = KeyEvent {
        key: KeyCode::A,
        action: KeyAction::Pressed,
    };
    const BREAK: KeyEvent = KeyEvent {
        key: KeyCode::A,
        action: KeyAction::Released,
    };

    #[test]
    fn preserves_fifo_order_across_wraparound() {
        let mut events = Events::new();
        for _ in 0..EVENT_CAPACITY {
            events.push(MAKE);
        }
        assert_eq!(events.pop(), Some(KeyboardInput::Key(MAKE)));
        events.push(BREAK);
        for _ in 1..EVENT_CAPACITY {
            assert_eq!(events.pop(), Some(KeyboardInput::Key(MAKE)));
        }
        assert_eq!(events.pop(), Some(KeyboardInput::Key(BREAK)));
        assert_eq!(events.pop(), None);
    }

    #[test]
    fn reports_overflow_before_delivering_input_again() {
        let mut events = Events::new();
        for _ in 0..EVENT_CAPACITY {
            events.push(MAKE);
        }
        events.push(BREAK);
        assert_eq!(events.pop(), Some(KeyboardInput::Lost));
        assert_eq!(events.pop(), Some(KeyboardInput::Key(BREAK)));
        assert_eq!(events.pop(), None);
    }

    #[test]
    fn ignores_auxiliary_bytes_and_resets_after_corrupt_keyboard_input() {
        const READS: &[(u16, u8)] = &[
            (4, 0x01),
            (0, 0xe0), // Start an extended keyboard code.
            (4, 0x21),
            (0, 0x1c), // Auxiliary input must leave it alone.
            (4, 0x01),
            (0, 0x75), // ArrowUp.
            (4, 0x01),
            (0, 0xe0),
            (4, 0x81),
            (0, 0x75), // Parity error discards the prefix.
            (4, 0x01),
            (0, 0x75), // Keypad8 after reset.
        ];
        struct Script(Cell<usize>);
        impl PortBlock for Script {
            fn read8(&self, offset: u16) -> Option<u8> {
                let step = self.0.get();
                let &(expected, value) = READS.get(step)?;
                assert_eq!(offset, expected);
                self.0.set(step + 1);
                Some(value)
            }

            fn write8(&self, _: u16, _: u8) -> Option<()> {
                panic!("unexpected port write in IRQ handler")
            }
        }
        let mut keyboard = Keyboard {
            controller: I8042::new(Script(Cell::new(0))),
            decoder: Decoder::new(),
            events: Events::new(),
        };
        keyboard.on_interrupt();
        keyboard.on_interrupt();
        assert_eq!(keyboard.next_input(), None);
        keyboard.on_interrupt();
        assert_eq!(
            keyboard.next_input(),
            Some(KeyboardInput::Key(KeyEvent {
                key: KeyCode::ArrowUp,
                action: KeyAction::Pressed,
            }))
        );
        keyboard.on_interrupt();
        keyboard.on_interrupt();
        keyboard.on_interrupt();
        assert_eq!(keyboard.next_input(), Some(KeyboardInput::Lost));
        assert_eq!(
            keyboard.next_input(),
            Some(KeyboardInput::Key(KeyEvent {
                key: KeyCode::Keypad8,
                action: KeyAction::Pressed,
            }))
        );
    }
}
