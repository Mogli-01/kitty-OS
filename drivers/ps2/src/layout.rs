//! Portuguese ISO keyboard layout for the boot console.

use super::{InputEvent, KeyAction, KeyCode};

const SHIFT: u8 = 0b0000_0011;
const CTRL: u8 = 0b0000_1100;
const ALT_GR: u8 = 0b0001_0000;
const SUPPRESS_TEXT: u8 = 0b1110_0000; // Left Alt and either Meta key.

/// One complete console input sequence, ready to publish as a unit.
pub struct ConsoleBytes {
    bytes: [u8; 8],
    len: usize,
}

impl ConsoleBytes {
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }

    fn special(bytes: &[u8]) -> Self {
        let mut result = Self {
            bytes: [0; 8],
            len: bytes.len(),
        };
        result.bytes[..bytes.len()].copy_from_slice(bytes);
        result
    }

    fn text(first: char, second: Option<char>) -> Self {
        let mut bytes = [0; 8];
        let mut len = first.encode_utf8(&mut bytes).len();
        if let Some(ch) = second {
            len += ch.encode_utf8(&mut bytes[len..]).len();
        }
        Self { bytes, len }
    }
}

/// Translates physical key events into the text console byte stream. Use a
/// separate instance per destination and deliver loss events to that instance.
#[derive(Default)]
pub struct PortugueseKeyboard {
    held: u8,
    caps_on: bool,
    caps_down: bool,
    accent: Option<Accent>,
}

impl PortugueseKeyboard {
    pub fn push(&mut self, event: InputEvent) -> Option<ConsoleBytes> {
        use KeyCode as K;

        let event = match event {
            InputEvent::Key(event) => event,
            InputEvent::Mouse(_) => return None,
            InputEvent::Lost => {
                *self = Self::default();
                return None;
            }
        };
        let pressed = event.action == KeyAction::Pressed;
        if let Some(bit) = modifier(event.key) {
            if pressed {
                self.held |= bit;
            } else {
                self.held &= !bit;
            }
            return None;
        }
        if event.key == K::CapsLock {
            if pressed && !self.caps_down {
                self.caps_on = !self.caps_on;
            }
            self.caps_down = pressed;
            return None;
        }
        if !pressed || self.held & SUPPRESS_TEXT != 0 {
            return None;
        }
        if self.held & CTRL != 0 {
            if self.held & ALT_GR != 0 {
                return None;
            }
            let letter = ascii_letter(event.key)?;
            self.accent = None;
            return Some(ConsoleBytes::special(&[letter - b'a' + 1]));
        }
        if event.key == K::Escape {
            self.accent = None;
            return None;
        }
        if let Some(bytes) = editing_key(event.key) {
            if event.key == K::Backspace && self.accent.take().is_some() {
                return None;
            }
            self.accent = None;
            return Some(ConsoleBytes::special(bytes));
        }

        let shift = self.held & SHIFT != 0;
        let alt_gr = self.held & ALT_GR != 0;
        let symbol = symbol(event.key, shift, self.caps_on, alt_gr)?;
        match symbol {
            Symbol::Dead(next) => self
                .accent
                .replace(next)
                .map(|previous| ConsoleBytes::text(previous.spacing(), None)),
            Symbol::Text(ch) => Some(match self.accent.take() {
                Some(accent) if ch == ' ' => ConsoleBytes::text(accent.spacing(), None),
                Some(accent) => match accent.apply(ch) {
                    Some(composed) => ConsoleBytes::text(composed, None),
                    None => ConsoleBytes::text(accent.spacing(), Some(ch)),
                },
                None => ConsoleBytes::text(ch, None),
            }),
        }
    }
}

fn modifier(key: KeyCode) -> Option<u8> {
    use KeyCode as K;
    let shift = match key {
        K::LeftShift => 0,
        K::RightShift => 1,
        K::LeftControl => 2,
        K::RightControl => 3,
        K::RightAlt => 4,
        K::LeftAlt => 5,
        K::LeftMeta => 6,
        K::RightMeta => 7,
        _ => return None,
    };
    Some(1 << shift)
}

fn editing_key(key: KeyCode) -> Option<&'static [u8]> {
    use KeyCode as K;
    Some(match key {
        K::Enter | K::KeypadEnter => b"\n",
        K::Tab => b"\t",
        K::Backspace => b"\x7f",
        K::ArrowUp => b"\x1b[A",
        K::ArrowDown => b"\x1b[B",
        K::ArrowRight => b"\x1b[C",
        K::ArrowLeft => b"\x1b[D",
        K::Home => b"\x1b[H",
        K::End => b"\x1b[F",
        K::Delete => b"\x1b[3~",
        _ => return None,
    })
}

fn ascii_letter(key: KeyCode) -> Option<u8> {
    use KeyCode as K;
    Some(match key {
        K::A => b'a',
        K::B => b'b',
        K::C => b'c',
        K::D => b'd',
        K::E => b'e',
        K::F => b'f',
        K::G => b'g',
        K::H => b'h',
        K::I => b'i',
        K::J => b'j',
        K::K => b'k',
        K::L => b'l',
        K::M => b'm',
        K::N => b'n',
        K::O => b'o',
        K::P => b'p',
        K::Q => b'q',
        K::R => b'r',
        K::S => b's',
        K::T => b't',
        K::U => b'u',
        K::V => b'v',
        K::W => b'w',
        K::X => b'x',
        K::Y => b'y',
        K::Z => b'z',
        _ => return None,
    })
}

enum Symbol {
    Text(char),
    Dead(Accent),
}

fn symbol(key: KeyCode, shift: bool, caps: bool, alt_gr: bool) -> Option<Symbol> {
    use KeyCode as K;
    use Symbol::{Dead, Text};

    if alt_gr {
        let ch = match key {
            K::Digit2 => '@',
            K::Digit3 => '£',
            K::Digit4 => '§',
            K::Digit7 => '{',
            K::Digit8 => '[',
            K::Digit9 => ']',
            K::Digit0 => '}',
            K::Minus | K::IsoExtra => '\\',
            K::E => '€',
            K::LeftBracket => return Some(Dead(Accent::Diaeresis)),
            _ => return None,
        };
        return Some(Text(ch));
    }
    if let Some(letter) = ascii_letter(key) {
        let letter = if shift ^ caps {
            letter.to_ascii_uppercase()
        } else {
            letter
        };
        return Some(Text(char::from(letter)));
    }
    let (base, shifted) = match key {
        K::Digit1 => ('1', '!'),
        K::Digit2 => ('2', '"'),
        K::Digit3 => ('3', '#'),
        K::Digit4 => ('4', '$'),
        K::Digit5 => ('5', '%'),
        K::Digit6 => ('6', '&'),
        K::Digit7 => ('7', '/'),
        K::Digit8 => ('8', '('),
        K::Digit9 => ('9', ')'),
        K::Digit0 => ('0', '='),
        K::Grave => ('\\', '|'),
        K::Minus => ('\'', '?'),
        K::Equals => ('«', '»'),
        K::LeftBracket => ('+', '*'),
        K::RightBracket => return Some(Dead(if shift { Accent::Grave } else { Accent::Acute })),
        K::Backslash => {
            return Some(Dead(if shift {
                Accent::Circumflex
            } else {
                Accent::Tilde
            }));
        }
        K::Semicolon => return Some(Text(if shift ^ caps { 'Ç' } else { 'ç' })),
        K::Apostrophe => ('º', 'ª'),
        K::IsoExtra => ('<', '>'),
        K::Comma => (',', ';'),
        K::Period => ('.', ':'),
        K::Slash => ('-', '_'),
        K::Space => (' ', ' '),
        _ => return None,
    };
    Some(Text(if shift { shifted } else { base }))
}

#[derive(Clone, Copy)]
enum Accent {
    Acute,
    Grave,
    Tilde,
    Circumflex,
    Diaeresis,
}

impl Accent {
    fn spacing(self) -> char {
        match self {
            Self::Acute => '´',
            Self::Grave => '`',
            Self::Tilde => '~',
            Self::Circumflex => '^',
            Self::Diaeresis => '¨',
        }
    }

    fn apply(self, ch: char) -> Option<char> {
        let (base, composed) = match self {
            Self::Acute => ("aeiouAEIOU", "áéíóúÁÉÍÓÚ"),
            Self::Grave => ("aeiouAEIOU", "àèìòùÀÈÌÒÙ"),
            Self::Tilde => ("anoANO", "ãñõÃÑÕ"),
            Self::Circumflex => ("aeiouAEIOU", "âêîôûÂÊÎÔÛ"),
            Self::Diaeresis => ("aeiouAEIOU", "äëïöüÄËÏÖÜ"),
        };
        let index = base.bytes().position(|byte| ch == char::from(byte))?;
        composed.chars().nth(index)
    }
}

#[cfg(test)]
mod tests;
