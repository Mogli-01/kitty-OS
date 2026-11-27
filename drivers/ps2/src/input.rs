use kitty_hal::PortBlock;
use kitty_i8042::{Error, I8042, Source};

use crate::mouse::{MouseDecoder, MouseReport};
use crate::{Decoder, KeyEvent};

const EVENT_CAPACITY: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputEvent {
    Key(KeyEvent),
    Mouse(MouseReport),
    /// Clear any held keys and buttons before processing the next event.
    Lost,
}

struct Events {
    slots: [Option<InputEvent>; EVENT_CAPACITY],
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

    fn push(&mut self, event: InputEvent) {
        if self.len == EVENT_CAPACITY {
            self.mark_lost();
        }
        let tail = (self.head + self.len) % EVENT_CAPACITY;
        self.slots[tail] = Some(event);
        self.len += 1;
    }

    fn pop(&mut self) -> Option<InputEvent> {
        if core::mem::take(&mut self.lost) {
            return Some(InputEvent::Lost);
        }
        if self.len == 0 {
            return None;
        }
        let event = self.slots[self.head].take();
        self.head = (self.head + 1) % EVENT_CAPACITY;
        self.len -= 1;
        event
    }
}

/// One i8042 controller with Set 2 and optional standard mouse decoding.
pub struct Ps2Input<P> {
    controller: I8042<P>,
    decoder: Decoder,
    mouse: Option<MouseDecoder>,
    events: Events,
}

impl<P: PortBlock> Ps2Input<P> {
    /// Configures the controller while IRQ1 is masked.
    pub fn initialize(ports: P) -> Result<Self, Error> {
        let mut controller = I8042::new(ports);
        controller.initialize_keyboard()?;
        Ok(Self {
            controller,
            decoder: Decoder::new(),
            mouse: None,
            events: Events::new(),
        })
    }

    /// Enables the mouse during boot, while IRQ1 and IRQ12 are masked.
    pub fn initialize_mouse(&mut self) -> Result<(), Error> {
        self.mouse = None;
        self.controller.initialize_mouse()?;
        self.mouse = Some(MouseDecoder::new());
        Ok(())
    }

    /// Restores keyboard-only mode if registering IRQ12 failed during boot.
    pub fn stop_mouse(&mut self) -> Result<(), Error> {
        self.mouse = None;
        self.controller.stop_mouse()
    }

    /// Stops the controller after the registered interrupt lines are masked.
    pub fn stop(mut self) -> Result<(), Error> {
        let mouse = if self.mouse.is_some() {
            self.controller.stop_mouse()
        } else {
            Ok(())
        };
        let keyboard = self.controller.stop_keyboard();
        mouse.and(keyboard)
    }

    /// Services IRQ1 or IRQ12 by reading at most one byte, without waiting.
    pub fn on_interrupt(&mut self) {
        match self.controller.try_read() {
            Ok(Some(output)) if output.source == Source::Keyboard => {
                if let Some(event) = self.decoder.push(output.value) {
                    self.events.push(InputEvent::Key(event));
                }
            }
            Ok(Some(output)) => {
                if let Some(report) = self
                    .mouse
                    .as_mut()
                    .and_then(|mouse| mouse.feed(output.value))
                {
                    self.events.push(InputEvent::Mouse(report));
                }
            }
            Ok(None) => {}
            Err(Error::CorruptOutput(Source::Auxiliary)) => {
                if let Some(mouse) = self.mouse.as_mut() {
                    mouse.corrupt_byte();
                    self.events.mark_lost();
                }
            }
            Err(Error::CorruptOutput(Source::Keyboard)) => {
                self.decoder.reset();
                self.events.mark_lost();
            }
            Err(_) => {
                self.decoder.reset();
                if let Some(mouse) = self.mouse.as_mut() {
                    mouse.lose_alignment();
                }
                self.events.mark_lost();
            }
        }
    }

    pub fn next_input(&mut self) -> Option<InputEvent> {
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

    struct Script<'a> {
        reads: &'a [(u16, u8)],
        step: Cell<usize>,
    }

    impl PortBlock for Script<'_> {
        fn read8(&self, offset: u16) -> Option<u8> {
            let step = self.step.get();
            let &(expected, value) = self.reads.get(step)?;
            assert_eq!(offset, expected);
            self.step.set(step + 1);
            Some(value)
        }

        fn write8(&self, _: u16, _: u8) -> Option<()> {
            panic!("unexpected port write in IRQ handler")
        }
    }

    fn scripted(reads: &[(u16, u8)], with_mouse: bool) -> Ps2Input<Script<'_>> {
        Ps2Input {
            controller: I8042::new(Script {
                reads,
                step: Cell::new(0),
            }),
            decoder: Decoder::new(),
            mouse: with_mouse.then(MouseDecoder::new),
            events: Events::new(),
        }
    }

    #[test]
    fn preserves_fifo_order_across_wraparound() {
        let mut events = Events::new();
        for _ in 0..EVENT_CAPACITY {
            events.push(InputEvent::Key(MAKE));
        }
        assert_eq!(events.pop(), Some(InputEvent::Key(MAKE)));
        events.push(InputEvent::Key(BREAK));
        for _ in 1..EVENT_CAPACITY {
            assert_eq!(events.pop(), Some(InputEvent::Key(MAKE)));
        }
        assert_eq!(events.pop(), Some(InputEvent::Key(BREAK)));
        assert_eq!(events.pop(), None);
    }

    #[test]
    fn reports_overflow_before_delivering_input_again() {
        let mut events = Events::new();
        for _ in 0..EVENT_CAPACITY {
            events.push(InputEvent::Key(MAKE));
        }
        events.push(InputEvent::Key(BREAK));
        assert_eq!(events.pop(), Some(InputEvent::Lost));
        assert_eq!(events.pop(), Some(InputEvent::Key(BREAK)));
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
        let mut keyboard = scripted(READS, false);
        keyboard.on_interrupt();
        keyboard.on_interrupt();
        assert_eq!(keyboard.next_input(), None);
        keyboard.on_interrupt();
        assert_eq!(
            keyboard.next_input(),
            Some(InputEvent::Key(KeyEvent {
                key: KeyCode::ArrowUp,
                action: KeyAction::Pressed,
            }))
        );
        keyboard.on_interrupt();
        keyboard.on_interrupt();
        keyboard.on_interrupt();
        assert_eq!(keyboard.next_input(), Some(InputEvent::Lost));
        assert_eq!(
            keyboard.next_input(),
            Some(InputEvent::Key(KeyEvent {
                key: KeyCode::Keypad8,
                action: KeyAction::Pressed,
            }))
        );
    }

    #[test]
    fn interleaves_completed_reports_and_skips_the_rest_of_a_corrupt_packet() {
        const READS: &[(u16, u8)] = &[
            (4, 0x21),
            (0, 0x09), // Start mouse packet with a held button.
            (4, 0x01),
            (0, 0x1c), // Interleaved key press.
            (4, 0x21),
            (0, 8),
            (4, 0x21),
            (0, 1), // Complete first mouse packet.
            (4, 0x21),
            (0, 0x09),
            (4, 0xa1),
            (0, 0x89), // Consume and mark a corrupt X byte.
            (4, 0x21),
            (0, 0x09), // The payload looks like a header.
            (4, 0x21),
            (0, 0x08),
            (4, 0x21),
            (0, 2),
            (4, 0x21),
            (0, 3),
        ];
        let mut input = scripted(READS, true);
        for _ in 0..4 {
            input.on_interrupt();
        }
        assert_eq!(input.next_input(), Some(InputEvent::Key(MAKE)));
        let Some(InputEvent::Mouse(first)) = input.next_input() else {
            panic!("missing mouse packet");
        };
        assert!(first.buttons.left);
        assert_eq!((first.delta_x, first.delta_y), (Some(8), Some(-1)));

        for _ in 0..6 {
            input.on_interrupt();
        }
        assert_eq!(input.next_input(), Some(InputEvent::Lost));
        let Some(InputEvent::Mouse(next)) = input.next_input() else {
            panic!("missing next mouse packet");
        };
        assert!(!next.buttons.left);
        assert_eq!((next.delta_x, next.delta_y), (Some(2), Some(-3)));
        assert_eq!(input.next_input(), None);
    }
}
