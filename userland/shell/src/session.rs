//! Incremental console input and command execution for the initial shell.

use core::{fmt, mem, str};

use super::{CommandLine, ExecuteError, LineBuffer, ParseError};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionEvent {
    Ignored,
    Edited,
    EditRejected,
    Submitted,
    Cancelled,
}

/// Shell state that stays alive across console reads. The caller owns the
/// display of the prompt and of the line under editing.
#[derive(Default)]
pub struct ShellSession {
    input: InputState,
    line: LineBuffer,
}

impl ShellSession {
    pub fn line(&self) -> &LineBuffer {
        &self.line
    }

    /// Consumes one byte from the console. On submit, sends command output or
    /// a diagnostic to `output`. A write error leaves the line consumed.
    pub fn push(
        &mut self,
        byte: u8,
        output: &mut impl fmt::Write,
    ) -> Result<SessionEvent, fmt::Error> {
        let Some(key) = self.input.push(byte) else {
            return Ok(SessionEvent::Ignored);
        };
        match key {
            Key::Insert(ch) => {
                if self.line.insert(ch).is_err() {
                    return Ok(SessionEvent::EditRejected);
                }
            }
            Key::Left => self.line.move_left(),
            Key::Right => self.line.move_right(),
            Key::Home => self.line.move_home(),
            Key::End => self.line.move_end(),
            Key::Backspace => self.line.backspace(),
            Key::Delete => self.line.delete(),
            Key::Cancel => {
                self.line.clear();
                return Ok(SessionEvent::Cancelled);
            }
            Key::Submit => {
                let command = CommandLine::parse(self.line.as_bytes());
                self.line.clear();
                match command {
                    Ok(command) => match command.execute(output) {
                        Ok(()) => {}
                        Err(ExecuteError::Output(error)) => return Err(error),
                        Err(ExecuteError::UnknownCommand) => {
                            output.write_str("unknown command\n")?
                        }
                        Err(ExecuteError::UnexpectedArguments) => {
                            output.write_str("unexpected arguments\n")?
                        }
                    },
                    Err(ParseError::TooManyWords) => output.write_str("too many words\n")?,
                    Err(ParseError::UnclosedQuote) => output.write_str("unclosed quote\n")?,
                    Err(ParseError::TrailingEscape) => output.write_str("trailing escape\n")?,
                    Err(
                        ParseError::LineTooLong
                        | ParseError::InvalidUtf8
                        | ParseError::ControlCharacter,
                    ) => output.write_str("invalid command line\n")?,
                }
                return Ok(SessionEvent::Submitted);
            }
        }
        Ok(SessionEvent::Edited)
    }
}

enum Key {
    Insert(char),
    Left,
    Right,
    Home,
    End,
    Backspace,
    Delete,
    Cancel,
    Submit,
}

#[derive(Default)]
enum InputState {
    #[default]
    Ready,
    Escape,
    Csi,
    CsiThree,
    IgnoreCsi,
    Utf8 {
        bytes: [u8; 4],
        filled: usize,
        width: usize,
    },
}

impl InputState {
    fn push(&mut self, byte: u8) -> Option<Key> {
        let previous = mem::take(self);
        match byte {
            b'\n' => return Some(Key::Submit),
            b'\t' => return Some(Key::Insert('\t')),
            0x03 => return Some(Key::Cancel),
            0x7f => return Some(Key::Backspace),
            0x1b => {
                *self = Self::Escape;
                return None;
            }
            _ => {}
        }

        match previous {
            Self::Escape if byte == b'[' => {
                *self = Self::Csi;
                None
            }
            Self::Csi => match byte {
                b'D' => Some(Key::Left),
                b'C' => Some(Key::Right),
                b'H' => Some(Key::Home),
                b'F' => Some(Key::End),
                b'3' => {
                    *self = Self::CsiThree;
                    None
                }
                _ => self.skip_csi(byte),
            },
            Self::CsiThree if byte == b'~' => Some(Key::Delete),
            Self::CsiThree | Self::IgnoreCsi => self.skip_csi(byte),
            Self::Utf8 {
                mut bytes,
                filled,
                width,
            } if (0x80..=0xbf).contains(&byte) => {
                bytes[filled] = byte;
                let filled = filled + 1;
                if filled == width {
                    return str::from_utf8(&bytes[..filled])
                        .ok()?
                        .chars()
                        .next()
                        .map(Key::Insert);
                }
                *self = Self::Utf8 {
                    bytes,
                    filled,
                    width,
                };
                None
            }
            // A new leading byte can recover from a lost continuation or ESC.
            _ => self.start_text(byte),
        }
    }

    fn start_text(&mut self, byte: u8) -> Option<Key> {
        let width = match byte {
            0x20..=0x7e => return Some(Key::Insert(char::from(byte))),
            0xc2..=0xdf => 2,
            0xe0..=0xef => 3,
            0xf0..=0xf4 => 4,
            _ => return None,
        };
        *self = Self::Utf8 {
            bytes: [byte, 0, 0, 0],
            filled: 1,
            width,
        };
        None
    }

    fn skip_csi(&mut self, byte: u8) -> Option<Key> {
        match byte {
            0x20..=0x3f => *self = Self::IgnoreCsi,
            0x80..=0xff => return self.start_text(byte),
            _ => {}
        }
        None
    }
}
