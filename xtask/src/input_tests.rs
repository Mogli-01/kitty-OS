//! Exercises both PS/2 devices in a headless QEMU guest.

use std::convert::Infallible;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Child, Stdio};
use std::time::{Duration, Instant};

use kitty_shell::{Console, run_console};

use super::{Opts, PANIC_MESSAGE, Result, TEST_TIMEOUT, build_dir, build_kernel, qemu_command};

const BOOT_READY: &str = "[boot] boot complete, idling with interrupts enabled";
const SERIAL_LIMIT: u64 = 256 * 1024;

pub(super) struct Scenario {
    device: &'static str,
    ready: &'static str,
    prefix: &'static str,
    check_shell: bool,
    steps: &'static [(&'static str, &'static [&'static str])],
}

pub(super) const KEYBOARD: Scenario = Scenario {
    device: "keyboard",
    ready: "[kbd] PS/2 keyboard ready on IRQ1",
    prefix: "[kbd-test] ",
    check_shell: true,
    steps: &[
        ("sendkey a", &["A Pressed", "text [61]", "A Released"]),
        (
            "sendkey up",
            &["ArrowUp Pressed", "text [1b, 5b, 41]", "ArrowUp Released"],
        ),
        (
            "sendkey bracket_right",
            &["RightBracket Pressed", "RightBracket Released"],
        ),
        ("sendkey e", &["E Pressed", "text [c3, a9]", "E Released"]),
        (
            "sendkey ctrl-c",
            &[
                "LeftControl Pressed",
                "C Pressed",
                "text [03]",
                "C Released",
                "LeftControl Released",
            ],
        ),
        ("sendkey h", &["H Pressed", "text [68]", "H Released"]),
        ("sendkey e", &["E Pressed", "text [65]", "E Released"]),
        ("sendkey l", &["L Pressed", "text [6c]", "L Released"]),
        ("sendkey p", &["P Pressed", "text [70]", "P Released"]),
        (
            "sendkey ret",
            &["Enter Pressed", "text [0a]", "Enter Released"],
        ),
        ("sendkey e", &["E Pressed", "text [65]", "E Released"]),
        ("sendkey c", &["C Pressed", "text [63]", "C Released"]),
        ("sendkey o", &["O Pressed", "text [6f]", "O Released"]),
        (
            "sendkey left",
            &[
                "ArrowLeft Pressed",
                "text [1b, 5b, 44]",
                "ArrowLeft Released",
            ],
        ),
        ("sendkey h", &["H Pressed", "text [68]", "H Released"]),
        (
            "sendkey right",
            &[
                "ArrowRight Pressed",
                "text [1b, 5b, 43]",
                "ArrowRight Released",
            ],
        ),
        (
            "sendkey spc",
            &["Space Pressed", "text [20]", "Space Released"],
        ),
        ("sendkey x", &["X Pressed", "text [78]", "X Released"]),
        (
            "sendkey backspace",
            &["Backspace Pressed", "text [7f]", "Backspace Released"],
        ),
        ("sendkey c", &["C Pressed", "text [63]", "C Released"]),
        ("sendkey a", &["A Pressed", "text [61]", "A Released"]),
        ("sendkey f", &["F Pressed", "text [66]", "F Released"]),
        (
            "sendkey bracket_right",
            &["RightBracket Pressed", "RightBracket Released"],
        ),
        ("sendkey e", &["E Pressed", "text [c3, a9]", "E Released"]),
        (
            "sendkey ret",
            &["Enter Pressed", "text [0a]", "Enter Released"],
        ),
    ],
};

pub(super) const MOUSE: Scenario = Scenario {
    device: "mouse",
    ready: "[mouse] PS/2 mouse ready on IRQ12",
    prefix: "[mouse-test] ",
    check_shell: false,
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
    let mut serial = String::new();
    for &(command, reports) in scenario.steps {
        let monitor = child.stdin.as_mut().ok_or("missing QEMU monitor input")?;
        writeln!(monitor, "{command}").map_err(|e| format!("sending {command} to QEMU: {e}"))?;
        expected.extend_from_slice(reports);
        serial = wait_for(child, log, command, |serial| {
            serial.strip_prefix(boot.as_str()).is_some_and(|new| {
                new.lines()
                    .filter_map(|line| line.strip_prefix(scenario.prefix))
                    .eq(expected.iter().copied())
            })
        })?;
    }
    if scenario.check_shell {
        let input = serial
            .strip_prefix(&boot)
            .ok_or("lost serial boot boundary")?;
        verify_shell_input(input)?;
        println!("xtask: host shell accepted PS/2 help, edited echo and cancellation");
    }
    Ok(())
}

fn verify_shell_input(serial: &str) -> Result<()> {
    let mut bytes = Vec::new();
    for line in serial.lines() {
        let Some(text) = line.strip_prefix("[kbd-test] text [") else {
            continue;
        };
        let text = text
            .strip_suffix(']')
            .ok_or("invalid keyboard byte report")?;
        for byte in text.split(", ") {
            bytes.push(u8::from_str_radix(byte, 16).map_err(|_| "invalid keyboard byte")?);
        }
    }
    let mut console = RecordedConsole {
        input: &bytes,
        output: Vec::new(),
    };
    run_console(&mut console).map_err(|error| format!("host shell failed: {error:?}"))?;
    let expected =
        "kitty> ^C\nkitty> help       list commands\necho TEXT  print text\nkitty> café\nkitty> ";
    if console.output != expected.as_bytes() {
        let output = String::from_utf8_lossy(&console.output);
        return Err(format!("unexpected shell session output {output:?}"));
    }
    Ok(())
}

struct RecordedConsole<'a> {
    input: &'a [u8],
    output: Vec<u8>,
}

impl Console for RecordedConsole<'_> {
    type Error = Infallible;

    fn read(&mut self, buffer: &mut [u8]) -> std::result::Result<usize, Self::Error> {
        // This also tests keeping a session across transport reads and
        // splitting UTF-8 and escape sequences between reads.
        let len = self.input.len().min(buffer.len()).min(1);
        let (bytes, remaining) = self.input.split_at(len);
        buffer[..len].copy_from_slice(bytes);
        self.input = remaining;
        Ok(len)
    }

    fn write(&mut self, bytes: &[u8]) -> std::result::Result<usize, Self::Error> {
        self.output.extend_from_slice(bytes);
        Ok(bytes.len())
    }
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
