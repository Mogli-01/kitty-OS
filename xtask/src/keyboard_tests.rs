//! Exercises the PS/2 controller and IRQ1 in a headless QEMU guest.

use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Child, Stdio};
use std::time::{Duration, Instant};

use super::{Opts, PANIC_MESSAGE, Result, TEST_TIMEOUT, build_dir, build_kernel, qemu_command};

const BOOT_READY: &str = "[boot] boot complete, idling with interrupts enabled";
const KEYBOARD_READY: &str = "[kbd] PS/2 keyboard ready on IRQ1";
const TRACE_PREFIX: &str = "[kbd-test] ";
const SERIAL_LIMIT: u64 = 256 * 1024;

pub(super) fn run(opts: &Opts) -> Result<()> {
    if opts.expect_panic || !opts.expect.is_empty() || !opts.features.is_empty() {
        return Err("test-keyboard accepts only --bios and --release".into());
    }
    let opts = Opts {
        release: opts.release,
        bios: opts.bios,
        features: vec!["selftest-ps2-keyboard".into()],
        ..Opts::default()
    };
    let kernel = build_kernel(&opts, false)?;
    let mut command = qemu_command(&opts, &kernel)?;
    let log = build_dir().join("serial-keyboard.log");
    File::create(&log).map_err(|e| format!("creating {}: {e}", log.display()))?;
    let mut child = command
        .args([
            "-display",
            "none",
            "-serial",
            "file:build/serial-keyboard.log",
            "-monitor",
            "stdio",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .map_err(|e| format!("cannot start QEMU: {e}"))?;

    let result = exercise(&mut child, &log);
    // Reap the guest on success and every error, since it idles indefinitely.
    let _ = child.kill();
    let reaped = child.wait().map_err(|e| format!("reaping QEMU: {e}"));
    result.map_err(|e| format!("{e} (log: {})", log.display()))?;
    reaped?;
    println!("xtask: PS/2 keyboard test passed (IRQ1, A and ArrowUp make/break)");
    Ok(())
}

fn exercise(child: &mut Child, log: &Path) -> Result<()> {
    let boot = wait_for(child, log, "boot completion", |serial| {
        serial.lines().any(|line| line == BOOT_READY)
    })?;
    if !boot.lines().any(|line| line == KEYBOARD_READY) {
        return Err("PS/2 keyboard did not initialize".into());
    }
    let mut expected = Vec::new();
    for (key, pressed, released) in [
        ("a", "A Pressed", "A Released"),
        ("up", "ArrowUp Pressed", "ArrowUp Released"),
    ] {
        let monitor = child.stdin.as_mut().ok_or("missing QEMU monitor input")?;
        writeln!(monitor, "sendkey {key}").map_err(|e| format!("sending {key} to QEMU: {e}"))?;
        expected.extend([pressed, released]);
        wait_for(child, log, &format!("{key} make/break"), |serial| {
            serial.strip_prefix(boot.as_str()).is_some_and(|new| {
                new.lines()
                    .filter_map(|line| line.strip_prefix(TRACE_PREFIX))
                    .eq(expected.iter().copied())
            })
        })?;
    }
    Ok(())
}

fn wait_for(
    child: &mut Child,
    log: &Path,
    stage: &str,
    ready: impl Fn(&str) -> bool,
) -> Result<String> {
    let start = Instant::now();
    loop {
        let mut bytes = Vec::new();
        File::open(log)
            .and_then(|file| file.take(SERIAL_LIMIT + 1).read_to_end(&mut bytes))
            .map_err(|e| format!("reading serial output: {e}"))?;
        if bytes.len() as u64 > SERIAL_LIMIT {
            return Err("QEMU serial output exceeded the keyboard test limit".into());
        }
        let serial = String::from_utf8_lossy(&bytes);
        if serial.contains(PANIC_MESSAGE) {
            return Err(format!("kernel panicked while waiting for {stage}"));
        }
        if let Some(status) = child
            .try_wait()
            .map_err(|e| format!("waiting for QEMU: {e}"))?
        {
            return Err(format!(
                "QEMU exited with {status} while waiting for {stage}"
            ));
        }
        if ready(&serial) {
            return Ok(serial.into_owned());
        }
        if start.elapsed() > TEST_TIMEOUT {
            return Err(format!("timed out waiting for {stage}"));
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}
