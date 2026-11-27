//! Headless QEMU runner shared by boot tests and timings.

use std::io::{self, BufRead, BufReader, Read};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use crate::{BOOT_COMPLETE, Result, TEST_TIMEOUT};

const MAX_SERIAL_BYTES: usize = 1024 * 1024;

pub(crate) struct Run {
    pub status: Option<ExitStatus>,
    pub serial: Vec<u8>,
    pub boot_elapsed: Option<Duration>,
}

pub(crate) fn run(command: &mut Command) -> Result<Run> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    let start = Instant::now();
    let mut child = command
        .spawn()
        .map_err(|e| format!("cannot start QEMU: {e}"))?;
    let reader = child
        .stdout
        .take()
        .ok_or_else(|| "QEMU stdout is unavailable".to_owned())
        .and_then(|stdout| {
            std::thread::Builder::new()
                .name("qemu-serial".into())
                .spawn(move || read_trace(stdout, start))
                .map_err(|e| format!("starting QEMU serial reader: {e}"))
        });
    let reader = match reader {
        Ok(reader) => reader,
        Err(error) => {
            stop(&mut child);
            return Err(error);
        }
    };

    let status = wait_for_exit(&mut child, start);
    if !matches!(status, Ok(Some(_))) {
        stop(&mut child);
    }
    let trace = reader.join().map_err(|_| "QEMU serial reader panicked")?;
    let (serial, boot_elapsed) = trace.map_err(|e| format!("reading QEMU serial output: {e}"))?;
    Ok(Run {
        status: status.map_err(|e| format!("waiting for QEMU: {e}"))?,
        serial,
        boot_elapsed,
    })
}

fn wait_for_exit(child: &mut Child, start: Instant) -> io::Result<Option<ExitStatus>> {
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }
        if start.elapsed() >= TEST_TIMEOUT {
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn stop(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn read_trace(output: impl Read, start: Instant) -> io::Result<(Vec<u8>, Option<Duration>)> {
    let mut output = BufReader::new(output.take(MAX_SERIAL_BYTES as u64 + 1));
    let mut serial = Vec::new();
    let mut boot_elapsed = None;
    loop {
        let offset = serial.len();
        if output.read_until(b'\n', &mut serial)? == 0 {
            return Ok((serial, boot_elapsed));
        }
        if serial.len() > MAX_SERIAL_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "QEMU serial output exceeds 1 MiB",
            ));
        }
        let line = &serial[offset..];
        if boot_elapsed.is_none()
            && line.starts_with(BOOT_COMPLETE.as_bytes())
            && line.ends_with(b"\n")
        {
            boot_elapsed = Some(start.elapsed());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_the_complete_boot_line_and_preserves_the_trace() {
        let serial = b"Hello from kitty-OS\r\n[boot] boot complete, halting\r\n";
        let (trace, ready) = read_trace(&serial[..], Instant::now()).unwrap();
        assert_eq!(trace, serial);
        assert!(ready.is_some());
    }

    #[test]
    fn rejects_a_partial_or_embedded_boot_marker() {
        let bytes = b"error: [boot] boot complete, halting\n[boot] boot complete,";
        let (_, ready) = read_trace(&bytes[..], Instant::now()).unwrap();
        assert!(ready.is_none());
    }
}
