//! Runs the shell over a byte stream with the console's partial I/O contract.

use core::fmt;

use crate::{SessionEvent, ShellSession};

const PROMPT: &[u8] = b"kitty> ";

/// Byte transport for the shell. A real process will use its CONSOLE handle.
/// A host or test transport may return zero on end of input.
pub trait Console {
    type Error;

    fn read(&mut self, buffer: &mut [u8]) -> Result<usize, Self::Error>;
    fn write(&mut self, bytes: &[u8]) -> Result<usize, Self::Error>;
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
            let event = session.push(byte, &mut output);
            if let Some(error) = output.failure.take() {
                return Err(error);
            }
            match event {
                Ok(SessionEvent::Submitted) => write_all(output.console, PROMPT)?,
                Ok(SessionEvent::Cancelled) => {
                    write_all(output.console, b"^C\n")?;
                    write_all(output.console, PROMPT)?;
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
