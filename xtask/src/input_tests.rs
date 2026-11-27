//! Exercises both PS/2 devices in a headless QEMU guest.

use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Child, Stdio};
use std::time::{Duration, Instant};

use super::{Opts, PANIC_MESSAGE, Result, TEST_TIMEOUT, build_dir, build_kernel, qemu_command};

const BOOT_READY: &str = "[boot] boot complete, idling with interrupts enabled";
const SERIAL_LIMIT: u64 = 256 * 1024;

pub(super) struct Scenario {
    device: &'static str,
    ready: &'static str,
    prefix: &'static str,
    steps: &'static [(&'static str, &'static [&'static str])],
}

pub(super) const KEYBOARD: Scenario = Scenario {
    device: "keyboard",
    ready: "[kbd] PS/2 keyboard ready on IRQ1",
    prefix: "[kbd-test] ",
    steps: &[
        ("sendkey a", &["A Pressed", "A Released"]),
        ("sendkey up", &["ArrowUp Pressed", "ArrowUp Released"]),
    ],
};

pub(super) const MOUSE: Scenario = Scenario {
    device: "mouse",
    ready: "[mouse] PS/2 mouse ready on IRQ12",
    prefix: "[mouse-test] ",
    steps: &[
        (
            "mouse_move 7 4",
            &["IRQ12 dx=Some(7) dy=Some(4) left=false right=false middle=false"],
        ),
        (
            "mouse_button 1",
            &["IRQ12 dx=Some(0) dy=Some(0) left=true right=false middle=false"],
        ),
        (
            "mouse_button 0",
            &["IRQ12 dx=Some(0) dy=Some(0) left=false right=false middle=false"],
        ),
    ],
};

pub(super) fn run(opts: &Opts, scenario: Scenario) -> Result<()> {
    if opts.expect_panic {
        return Err(format!(
            "test-{} requires a successful boot and cannot use --expect-panic",
            scenario.device
        ));
    }
    let mut features = opts.features.clone();
    features.push("selftest-ps2-input".into());
    let guest_opts = Opts {
        release: opts.release,
        bios: opts.bios,
        features,
        ..Opts::default()
    };
    let kernel = build_kernel(&guest_opts, false)?;
    let mut command = qemu_command(&guest_opts, &kernel)?;
    let name = format!("serial-{}.log", scenario.device);
    let log = build_dir().join(&name);
    File::create(&log).map_err(|e| format!("creating {}: {e}", log.display()))?;
    let mut child = command
        .args([
            "-display",
            "none",
            "-serial",
            &format!("file:build/{name}"),
            "-monitor",
            "stdio",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .map_err(|e| format!("cannot start QEMU: {e}"))?;

    let result = exercise(&mut child, &log, &scenario, &opts.expect);
    // Reap the guest on success and every error, since it idles indefinitely.
    let _ = child.kill();
    let reaped = child.wait().map_err(|e| format!("reaping QEMU: {e}"));
    result.map_err(|e| format!("{e} (log: {})", log.display()))?;
    reaped?;
    println!("xtask: PS/2 {} test passed", scenario.device);
    Ok(())
}

fn exercise(
    child: &mut Child,
    log: &Path,
    scenario: &Scenario,
    boot_expectations: &[String],
) -> Result<()> {
    let boot = wait_for(child, log, "boot completion", |serial| {
        serial.lines().any(|line| line == BOOT_READY)
    })?;
    if !boot.lines().any(|line| line == scenario.ready) {
        return Err(format!("PS/2 {} did not initialize", scenario.device));
    }
    for text in boot_expectations {
        if !boot.contains(text.as_str()) {
            return Err(format!("boot serial output does not contain {text:?}"));
        }
    }
    let mut expected = Vec::new();
    for &(command, reports) in scenario.steps {
        let monitor = child.stdin.as_mut().ok_or("missing QEMU monitor input")?;
        writeln!(monitor, "{command}").map_err(|e| format!("sending {command} to QEMU: {e}"))?;
        expected.extend_from_slice(reports);
        wait_for(child, log, command, |serial| {
            serial.strip_prefix(boot.as_str()).is_some_and(|new| {
                new.lines()
                    .filter_map(|line| line.strip_prefix(scenario.prefix))
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
            return Err("QEMU serial output exceeded the input test limit".into());
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
