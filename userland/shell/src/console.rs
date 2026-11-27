//! Runs the shell over a byte stream with the console's partial I/O contract.

use core::fmt;

use crate::{CommandLine, ExecuteError, LineBuffer, SessionEvent, ShellSession};

const PROMPT: &[u8] = b"kitty> ";

/// Byte transport for the shell. A real process will use its CONSOLE handle.
/// A host or test transport may return zero on end of input.
pub trait Console {
    type Error;

    fn read(&mut self, buffer: &mut [u8]) -> Result<usize, Self::Error>;
    fn write(&mut self, bytes: &[u8]) -> Result<usize, Self::Error>;

    /// Whether this transport expects the shell to echo its own input.
    fn echo_input(&self) -> bool {
        false
    }

    /// Optional file service. An unavailable service returns None.
    fn cat(&mut self, _path: &str) -> Option<Result<(), Self::Error>> {
        None
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum ConsoleError<E> {
    Read(E),
    Write(E),
    InvalidReadCount,
    InvalidWriteCount,
    WriteZero,
}

/// Runs an editing session until EOF or an I/O error. The display owns
/// live echo and cursor placement. The shell emits prompts and command output.
pub fn run_console<C: Console>(console: &mut C) -> Result<(), ConsoleError<C::Error>> {
    let mut session = ShellSession::default();
    let mut input = [0u8; 128];
    write_all(console, PROMPT)?;
    let mut drawn = 0;
    loop {
        let count = console.read(&mut input).map_err(ConsoleError::Read)?;
        let bytes = input.get(..count).ok_or(ConsoleError::InvalidReadCount)?;
        if bytes.is_empty() {
            return Ok(());
        }
        let mut output = ConsoleOutput {
            console,
            failure: None,
        };
        for &byte in bytes {
            if byte == b'\n' && output.console.echo_input() {
                write_all(output.console, b"\n")?;
            }
            let event = session.push_with(byte, &mut output, ConsoleOutput::execute);
            if let Some(error) = output.failure.take() {
                return Err(error);
            }
            match event {
                Ok(SessionEvent::Submitted) => {
                    drawn = 0;
                    write_all(output.console, PROMPT)?;
                }
                Ok(SessionEvent::Cancelled) => {
                    drawn = 0;
                    write_all(output.console, b"^C\n")?;
                    write_all(output.console, PROMPT)?;
                }
                Ok(SessionEvent::Edited) if output.console.echo_input() => {
                    redraw(output.console, session.line(), &mut drawn)?;
                }
                // ShellSession propagates fmt::Error only from the writer.
                _ => {}
            }
        }
    }
}

struct ConsoleOutput<'a, C: Console> {
    console: &'a mut C,
    failure: Option<ConsoleError<C::Error>>,
}

impl<C: Console> ConsoleOutput<'_, C> {
    fn execute(command: &CommandLine, output: &mut Self) -> Result<(), ExecuteError> {
        let mut words = command.words();
        if words.next() != Some("cat") {
            return command.execute(output);
        }
        let path = words.next().ok_or(ExecuteError::UnexpectedArguments)?;
        if words.next().is_some() {
            return Err(ExecuteError::UnexpectedArguments);
        }
        match output.console.cat(path) {
            None => Err(ExecuteError::UnknownCommand),
            Some(Ok(())) => Ok(()),
            Some(Err(error)) => {
                output.failure = Some(ConsoleError::Write(error));
                Err(ExecuteError::Output(fmt::Error))
            }
        }
    }
}

fn redraw<C: Console>(
    console: &mut C,
    line: &LineBuffer,
    drawn: &mut usize,
) -> Result<(), ConsoleError<C::Error>> {
    const WIDTH: usize = 32;
    let text = core::str::from_utf8(line.as_bytes()).unwrap_or_default();
    let cursor = line.cursor();
    let prefix = text.get(..cursor).unwrap_or_default();
    let start = prefix
        .char_indices()
        .rev()
        .nth(WIDTH - 1)
        .map_or(0, |(offset, _)| offset);
    let view = &text[start..];
    let end = view
        .char_indices()
        .nth(WIDTH)
        .map_or(view.len(), |(offset, _)| offset);
    let view = &view[..end];
    let cursor = cursor.saturating_sub(start).min(end);
    let width = view.chars().count();
    write_all(console, b"\r")?;
    write_all(console, PROMPT)?;
    for ch in view.chars() {
        let mut bytes = [0; 4];
        write_all(
            console,
            if ch == '\t' {
                b" "
            } else {
                ch.encode_utf8(&mut bytes).as_bytes()
            },
        )?;
    }
    let spaces = [b' '; WIDTH];
    write_all(console, &spaces[..drawn.saturating_sub(width)])?;
    write_all(console, b"\r")?;
    write_all(console, PROMPT)?;
    for ch in view[..cursor].chars() {
        let mut bytes = [0; 4];
        write_all(
            console,
            if ch == '\t' {
                b" "
            } else {
                ch.encode_utf8(&mut bytes).as_bytes()
            },
        )?;
    }
    *drawn = width;
    Ok(())
}

impl<C: Console> fmt::Write for ConsoleOutput<'_, C> {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        write_all(self.console, text.as_bytes()).map_err(|error| {
            self.failure = Some(error);
            fmt::Error
        })
    }
}

fn write_all<C: Console>(console: &mut C, mut bytes: &[u8]) -> Result<(), ConsoleError<C::Error>> {
    while !bytes.is_empty() {
        let written = console.write(bytes).map_err(ConsoleError::Write)?;
        if written == 0 {
            return Err(ConsoleError::WriteZero);
        }
        bytes = bytes
            .get(written..)
            .ok_or(ConsoleError::InvalidWriteCount)?;
    }
    Ok(())
}
