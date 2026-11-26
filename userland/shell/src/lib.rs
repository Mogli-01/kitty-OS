//! Command parsing and built-ins for the initial user-space shell.

#![no_std]
#![forbid(unsafe_code)]

use core::{fmt, str};

mod editor;
pub use editor::{EditError, LineBuffer};

pub const MAX_LINE_BYTES: usize = 1024;
pub const MAX_WORDS: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParseError {
    LineTooLong,
    InvalidUtf8,
    ControlCharacter,
    TooManyWords,
    UnclosedQuote,
    TrailingEscape,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecuteError {
    UnknownCommand,
    UnexpectedArguments,
    Output(fmt::Error),
}

/// Parsed words stored as UTF-8 spans in a bounded buffer.
pub struct CommandLine {
    bytes: [u8; MAX_LINE_BYTES],
    spans: [(usize, usize); MAX_WORDS],
    count: usize,
}

impl CommandLine {
    /// Splits on ASCII spaces and tabs outside quotes. Single quotes are
    /// literal. A backslash outside single quotes quotes the next character.
    pub fn parse(input: &[u8]) -> Result<Self, ParseError> {
        if input.len() > MAX_LINE_BYTES {
            return Err(ParseError::LineTooLong);
        }
        let line = str::from_utf8(input).map_err(|_| ParseError::InvalidUtf8)?;
        if line.chars().any(disallowed_control) {
            return Err(ParseError::ControlCharacter);
        }
        let mut parsed = Self {
            bytes: [0; MAX_LINE_BYTES],
            spans: [(0, 0); MAX_WORDS],
            count: 0,
        };
        let mut chars = line.chars();
        let mut quote = None;
        let mut start = None;
        let mut end = 0;
        while let Some(ch) = chars.next() {
            let literal = match (quote, ch) {
                (None, ' ' | '\t') => {
                    parsed.finish_word(start.take(), end)?;
                    continue;
                }
                (None, '\'' | '"') => {
                    start.get_or_insert(end);
                    quote = Some(ch);
                    continue;
                }
                (Some(open), close) if open == close => {
                    quote = None;
                    continue;
                }
                (None | Some('"'), '\\') => chars.next().ok_or(ParseError::TrailingEscape)?,
                _ => ch,
            };
            start.get_or_insert(end);
            // Each stored character consumes at least as many input bytes
            // as it writes. Quoting and escaping only remove delimiters.
            let finish = end + literal.len_utf8();
            literal.encode_utf8(&mut parsed.bytes[end..finish]);
            end = finish;
        }
        if quote.is_some() {
            return Err(ParseError::UnclosedQuote);
        }
        parsed.finish_word(start, end)?;
        Ok(parsed)
    }

    fn finish_word(&mut self, start: Option<usize>, end: usize) -> Result<(), ParseError> {
        if let Some(start) = start {
            let span = self
                .spans
                .get_mut(self.count)
                .ok_or(ParseError::TooManyWords)?;
            *span = (start, end);
            self.count += 1;
        }
        Ok(())
    }

    /// Iterates over the command name and its arguments in order.
    pub fn words(&self) -> impl Iterator<Item = &str> {
        self.spans[..self.count]
            .iter()
            .filter_map(|&(start, end)| str::from_utf8(&self.bytes[start..end]).ok())
    }

    /// Executes an in-process built-in. Output is supplied by the caller.
    pub fn execute(&self, output: &mut impl fmt::Write) -> Result<(), ExecuteError> {
        let mut words = self.words();
        match words.next() {
            None => Ok(()),
            Some("help") if words.next().is_none() => output
                .write_str("help       list commands\necho TEXT  print text\n")
                .map_err(ExecuteError::Output),
            Some("help") => Err(ExecuteError::UnexpectedArguments),
            Some("echo") => {
                if let Some(first) = words.next() {
                    output.write_str(first).map_err(ExecuteError::Output)?;
                    for word in words {
                        output.write_char(' ').map_err(ExecuteError::Output)?;
                        output.write_str(word).map_err(ExecuteError::Output)?;
                    }
                }
                output.write_char('\n').map_err(ExecuteError::Output)
            }
            Some(_) => Err(ExecuteError::UnknownCommand),
        }
    }
}

fn disallowed_control(ch: char) -> bool {
    ch.is_control() && ch != '\t'
}

#[cfg(test)]
mod tests;
