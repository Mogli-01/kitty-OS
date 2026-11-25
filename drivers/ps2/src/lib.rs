//! PS/2 keyboard scan code set 2 decoder for an untranslated byte stream.
//!
//! The controller transport handles command replies and calls [`Decoder::reset`]
//! when bytes are lost or the device is reconfigured. The decoder emits physical
//! keys. A later input layer applies a keyboard layout and attaches timestamps.

#![no_std]
#![forbid(unsafe_code)]

mod keyboard;
mod keys;

pub use keyboard::{Keyboard, KeyboardInput};
pub use keys::KeyCode;

const EXTENDED: u8 = 0xe0;
const PAUSE: u8 = 0xe1;
const BREAK: u8 = 0xf0;
const PAUSE_TAIL: [u8; 7] = [0x14, 0x77, 0xe1, 0xf0, 0x14, 0xf0, 0x77];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyAction {
    /// Make code. Typematic repeats produce additional presses.
    Pressed,
    Released,
    /// A momentary key with no release code, such as unmodified Pause.
    Tapped,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyEvent {
    pub key: KeyCode,
    pub action: KeyAction,
}

#[derive(Clone, Copy, Debug)]
enum State {
    Idle,
    Break,
    Extended,
    ExtendedBreak,
    Pause(u8),
}

/// Decodes one byte at a time without buffering a stream or allocating memory.
#[derive(Debug)]
pub struct Decoder {
    state: State,
}

impl Default for Decoder {
    fn default() -> Self {
        Self::new()
    }
}

impl Decoder {
    pub const fn new() -> Self {
        Self { state: State::Idle }
    }

    /// Discards a partial scan code after a transport error or device reset.
    pub fn reset(&mut self) {
        self.state = State::Idle;
    }

    /// Returns an event only when this byte completes a supported scan code.
    /// Unknown codes and invalid sequences leave no pending key event.
    pub fn push(&mut self, byte: u8) -> Option<KeyEvent> {
        let state = core::mem::replace(&mut self.state, State::Idle);
        let (key, action) = match (state, byte) {
            (State::Pause(index), _) if PAUSE_TAIL.get(usize::from(index)) == Some(&byte) => {
                let next = index + 1;
                if usize::from(next) == PAUSE_TAIL.len() {
                    return Some(KeyEvent {
                        key: KeyCode::Pause,
                        action: KeyAction::Tapped,
                    });
                }
                self.state = State::Pause(next);
                return None;
            }
            (_, PAUSE) => {
                self.state = State::Pause(0);
                return None;
            }
            (_, EXTENDED) => {
                self.state = State::Extended;
                return None;
            }
            (State::Idle, BREAK) => {
                self.state = State::Break;
                return None;
            }
            (State::Extended, BREAK) => {
                self.state = State::ExtendedBreak;
                return None;
            }
            (_, BREAK) | (State::Pause(_), _) => return None,
            (State::Idle, _) => (KeyCode::normal(byte), KeyAction::Pressed),
            (State::Break, _) => (KeyCode::normal(byte), KeyAction::Released),
            (State::Extended, _) => (KeyCode::extended(byte), KeyAction::Pressed),
            (State::ExtendedBreak, _) => (KeyCode::extended(byte), KeyAction::Released),
        };
        key.map(|key| KeyEvent { key, action })
    }
}
