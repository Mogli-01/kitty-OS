use std::io::{self, Read, StdinLock, StdoutLock, Write};
use std::process::ExitCode;

use kitty_shell::{Console, run_console};

struct HostConsole<'a> {
    input: StdinLock<'a>,
    output: StdoutLock<'a>,
}

impl Console for HostConsole<'_> {
    type Error = io::Error;

    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.input.read(buffer)
    }

    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let count = self.output.write(bytes)?;
        self.output.flush()?;
        Ok(count)
    }
}

fn main() -> ExitCode {
    let mut console = HostConsole {
        input: io::stdin().lock(),
        output: io::stdout().lock(),
    };
    match run_console(&mut console) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("kitty shell: {error:?}");
            ExitCode::FAILURE
        }
    }
}
