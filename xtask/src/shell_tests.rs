//! Build and exercise the actual Ring 3 shell through the QEMU keyboard.

use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

use super::{Opts, Result, build_dir, build_kernel, qemu_media_command, root, run_cmd};

const CONTENTS: &str = "kitty-OS phase 7 file";
const PROMPT: &str = "kitty> ";

pub(super) fn package(profile_dir: &std::path::Path) -> Result<PathBuf> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    // The initrd loader expects a static executable at a fixed user address.
    // Apply these flags to all target crates in this Cargo invocation under
    // LTO. The small code model below applies only to the init executable.
    let rustflags = ["-Crelocation-model=static", "-Ccode-model=kernel"].join("\x1f");
    run_cmd(
        Command::new(cargo)
            .current_dir(root())
            .env("CARGO_ENCODED_RUSTFLAGS", rustflags)
            .args([
                "rustc",
                "-p",
                "kitty-shell",
                "--bin",
                "kitty-init",
                "--features",
                "init-image",
                "--target",
                "x86_64-unknown-none",
                "--release",
                "--locked",
                "--",
                "-Ccode-model=small",
                "-Clink-arg=-Tuserland/shell/init.ld",
            ]),
    )?;

    let files = profile_dir.join("files");
    for directory in ["sbin", "etc"] {
        fs::create_dir_all(files.join(directory))
            .map_err(|error| format!("creating shell initrd directory: {error}"))?;
    }
    let program = root().join("target/x86_64-unknown-none/release/kitty-init");
    fs::copy(program, files.join("sbin/init"))
        .map_err(|error| format!("copying /sbin/init: {error}"))?;
    fs::write(files.join("etc/kitty.txt"), format!("{CONTENTS}\n"))
        .map_err(|error| format!("writing shell fixture: {error}"))?;

    let archive = profile_dir.join("shell.tar");
    run_cmd(
        Command::new("tar")
            .args(["--format=ustar", "-cf"])
            .arg(&archive)
            .arg("-C")
            .arg(files)
            .args(["sbin/init", "etc/kitty.txt"]),
    )?;
    Ok(archive)
}

pub(super) fn run(opts: &Opts) -> Result<()> {
    if opts.expect_panic || opts.initrd.is_some() || !opts.expect.is_empty() {
        return Err("test-shell uses its own initrd and console expectations".into());
    }
    let firmware = if opts.bios { "bios" } else { "uefi" };
    let profile = if opts.release { "release" } else { "debug" };
    let output = build_dir()
        .join("shell-tests")
        .join(format!("{firmware}-{profile}"));
    let archive = package(&output)?;
    let archive = super::initrd::load(&archive)?;
    let kernel = build_kernel(opts, false)?;
    let serial = output.join("serial.log");
    File::create(&serial).map_err(|error| format!("creating shell serial log: {error}"))?;
    let mut qemu = qemu_media_command(opts, &kernel, Some(&archive), &output)?
        .args(["-display", "none", "-monitor", "stdio"])
        .arg("-serial")
        .arg(format!("file:{}", serial.display()))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .map_err(|error| format!("starting QEMU: {error}"))?;

    let result = (|| {
        let mut after =
            super::input_tests::wait_for(&mut qemu, &serial, "Ring 3 shell prompt", |log| {
                log.contains("[init] /sbin/init started (pid ") && log.contains(PROMPT)
            })?;
        let steps: &[(&[&str], &str)] = &[
            (
                &["h", "e", "l", "p", "ret"],
                "\n[u] help       list commands\r\n",
            ),
            (
                &[
                    "e",
                    "c",
                    "o",
                    "left",
                    "h",
                    "right",
                    "spc",
                    "x",
                    "backspace",
                    "c",
                    "a",
                    "f",
                    "bracket_right",
                    "e",
                    "ret",
                ],
                "\n[u] café\r\n",
            ),
            (&["x", "ctrl-c"], "^C\r\n"),
            (
                &[
                    "c", "a", "t", "spc", "e", "t", "c", "shift-7", "k", "i", "t", "t", "y", "dot",
                    "t", "x", "t", "ret",
                ],
                "\n[u] kitty-OS phase 7 file\r\n",
            ),
            (
                &["c", "a", "t", "spc", "shift-7", "x", "ret"],
                "\n[u] invalid relative path\r\n",
            ),
            (
                &["c", "a", "t", "spc", "x", "ret"],
                "\n[u] file not found\r\n",
            ),
        ];
        for &(keys, expected) in steps {
            let monitor = qemu.stdin.as_mut().ok_or("missing QEMU monitor input")?;
            for key in keys {
                writeln!(monitor, "sendkey {key} 30")
                    .map_err(|error| format!("sending {key}: {error}"))?;
                std::thread::sleep(Duration::from_millis(60));
            }
            after = super::input_tests::wait_for(&mut qemu, &serial, expected, |log| {
                log.strip_prefix(&after)
                    .is_some_and(|new| new.contains(expected) && new.ends_with(PROMPT))
            })?;
        }
        Ok::<(), String>(())
    })();
    let _ = qemu.kill();
    let reaped = qemu
        .wait()
        .map_err(|error| format!("reaping QEMU: {error}"));
    result.map_err(|error| format!("{error} (log: {})", serial.display()))?;
    reaped?;
    println!("xtask: Ring 3 shell, keyboard and initrd file verified ({firmware}, {profile})");
    Ok(())
}
