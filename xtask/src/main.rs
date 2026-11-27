//! `cargo xtask` — host-side build tool for kitty-OS.
//!
//! Commands:
//!   build [--release]            build the kernel ELF
//!   esp   [--release]            build an EFI System Partition folder (build/esp)
//!   iso   [--release]            build a hybrid BIOS/UEFI ISO (needs xorriso)
//!   run   [--release] [--bios]   boot in QEMU (UEFI from build/esp, or BIOS from the ISO)
//!   test  [--release] [--bios]   boot headless, check serial output and exit code
//!   test-memory [--release] [--bios]   check the Phase 3 memory protections in QEMU
//!   test-visual [--release] [--bios]   check the framebuffer banner against a reference
//!   bench-boot [--release] [--bios]    sample time to the boot-complete serial line
//!   test-keyboard [--release] [--bios] check PS/2 key events through IRQ1 in QEMU
//!   test-mouse [--release] [--bios]    check PS/2 mouse reports through IRQ12 in QEMU
//!   bench-scheduler [--release] [--bios]   sample cooperative yield+switch TSC cycles
//!   test-initrd [--release] [--bios]   check initrd mounting and rejection in QEMU
//!
//! Extra options:
//!   --features <a,b>   extra kernel cargo features (e.g. selftest-page-fault)
//!   --initrd <path>    include a validated USTAR archive in the boot media
//!   --expect-panic     (test) pass only if the kernel panics: QEMU exit code
//!                      35 and "KERNEL PANIC" on the serial port
//!   --expect <text>    require a serial marker (repeatable). Input tests
//!                      require the marker before injecting events.
//!
//! Environment overrides: KITTY_QEMU (qemu binary), KITTY_OVMF (UEFI code
//! firmware), KITTY_XORRISO (xorriso binary), KITTY_BENCH_AGAINST (baseline ELF).
//!
//! Host standard library and the project's VFS for initrd validation.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::Duration;

mod boot_bench;
mod boot_trace;
mod initrd;
mod initrd_tests;
mod input_tests;
mod memory_tests;
mod scheduler_bench;
mod visual_test;

/// Limine binary release we boot with. Pinned to an exact commit of the
/// official `v11.x-binary` branch so a compromised or changed branch cannot
/// silently change our bootloader (supply-chain safety). Bump deliberately.
const LIMINE_REPO: &str = "https://github.com/limine-bootloader/limine.git";
const LIMINE_COMMIT: &str = "5be26a73d7b7b4d4477d18be94e1d16e615adf56";
const LIMINE_FETCH_ATTEMPTS: u64 = 3;

const KERNEL_NAME: &str = "kitty-kernel";
const BOOT_MESSAGE: &str = "Hello from kitty-OS";
const BOOT_COMPLETE: &str = "[boot] boot complete,";
/// isa-debug-exit: kernel writes 0x10 -> QEMU exits with (0x10 << 1) | 1.
const QEMU_SUCCESS: i32 = 33;
/// Kernel panicked under the `qemu-exit` feature: (0x11 << 1) | 1.
const QEMU_FAILURE: i32 = 35;
const PANIC_MESSAGE: &str = "KERNEL PANIC";
const TEST_TIMEOUT: Duration = Duration::from_secs(60);

type Result<T> = std::result::Result<T, String>;

#[derive(Default)]
struct Opts {
    release: bool,
    bios: bool,
    features: Vec<String>,
    expect_panic: bool,
    expect: Vec<String>,
    initrd: Option<PathBuf>,
}

impl Opts {
    fn load_initrd(&self) -> Result<Option<Vec<u8>>> {
        self.initrd.as_deref().map(initrd::load).transpose()
    }
}

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let cmd = args.next().unwrap_or_default();
    let mut opts = Opts::default();
    while let Some(a) = args.next() {
        match a.as_str() {
            "--release" => opts.release = true,
            "--bios" => opts.bios = true,
            "--expect-panic" => opts.expect_panic = true,
            "--initrd" => match args.next() {
                Some(path) if opts.initrd.is_none() => opts.initrd = Some(PathBuf::from(path)),
                _ => {
                    eprintln!("xtask: --initrd requires exactly one archive path");
                    return ExitCode::FAILURE;
                }
            },
            "--expect" => match args.next() {
                Some(text) => opts.expect.push(text),
                None => {
                    eprintln!("xtask: --expect needs a value");
                    return ExitCode::FAILURE;
                }
            },
            "--features" => match args.next() {
                Some(list) => opts
                    .features
                    .extend(list.split(',').filter(|f| !f.is_empty()).map(String::from)),
                None => {
                    eprintln!("xtask: --features needs a value");
                    return ExitCode::FAILURE;
                }
            },
            other => {
                eprintln!("xtask: unknown option `{other}`");
                return ExitCode::FAILURE;
            }
        }
    }

    if cmd == "build" && opts.initrd.is_some() {
        eprintln!("xtask: use --initrd with a command that prepares boot media");
        return ExitCode::FAILURE;
    }
    let result = match cmd.as_str() {
        "build" => build_kernel(&opts, false).map(|p| println!("kernel: {}", p.display())),
        "esp" => build_kernel(&opts, false)
            .and_then(|k| make_esp(&k, opts.load_initrd()?.as_deref(), &build_dir()))
            .map(|_| ()),
        "iso" => build_kernel(&opts, false)
            .and_then(|k| make_iso(&k, opts.load_initrd()?.as_deref(), &build_dir()))
            .map(|_| ()),
        "run" => run(&opts),
        "test" => test(&opts),
        "test-memory" => memory_tests::run(&opts),
        "test-visual" => visual_test::run(&opts),
        "bench-boot" => boot_bench::run(&opts),
        "test-keyboard" => input_tests::run(&opts, input_tests::KEYBOARD),
        "test-mouse" => input_tests::run(&opts, input_tests::MOUSE),
        "bench-scheduler" => scheduler_bench::run(&opts),
        "test-initrd" => initrd_tests::run(&opts),
        _ => {
            eprintln!(
                "usage: cargo xtask <build|esp|iso|run|test> [--release] [--bios] [--features a,b] [--initrd path] [--expect-panic] [--expect text]"
            );
            eprintln!("       cargo xtask test-memory [--release] [--bios]");
            eprintln!("       cargo xtask test-visual [--release] [--bios]");
            eprintln!("       cargo xtask bench-boot [--release] [--bios]");
            eprintln!(
                "       cargo xtask <test-keyboard|test-mouse> [--release] [--bios] [--features a,b] [--expect text]"
            );
            eprintln!("       cargo xtask bench-scheduler [--release] [--bios]");
            eprintln!("       cargo xtask test-initrd [--release] [--bios]");
            return ExitCode::FAILURE;
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("xtask: error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn root() -> PathBuf {
    // xtask/ lives directly under the workspace root.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn build_dir() -> PathBuf {
    root().join("build")
}

fn run_cmd(cmd: &mut Command) -> Result<()> {
    let status = cmd
        .status()
        .map_err(|e| format!("cannot run {:?}: {e}", cmd.get_program()))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{:?} failed with {status}", cmd.get_program()))
    }
}

fn copy(from: &Path, to: &Path) -> Result<()> {
    if let Some(dir) = to.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("mkdir {}: {e}", dir.display()))?;
    }
    fs::copy(from, to)
        .map(|_| ())
        .map_err(|e| format!("copy {} -> {}: {e}", from.display(), to.display()))
}

// ---------------------------------------------------------------- kernel ---

fn build_kernel(opts: &Opts, qemu_exit: bool) -> Result<PathBuf> {
    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let mut cmd = Command::new(cargo);
    cmd.current_dir(root()).args(["build", "-p", KERNEL_NAME]);
    if opts.release {
        cmd.arg("--release");
    }
    let mut features = opts.features.clone();
    if qemu_exit {
        features.push("qemu-exit".into());
    }
    if !features.is_empty() {
        cmd.arg("--features").arg(features.join(","));
    }
    run_cmd(&mut cmd)?;
    let profile = if opts.release { "release" } else { "debug" };
    Ok(root()
        .join("target/x86_64-unknown-none")
        .join(profile)
        .join(KERNEL_NAME))
}

// ---------------------------------------------------------------- limine ---

/// Fetches the pinned Limine binaries into build/limine (once).
fn limine_dir() -> Result<PathBuf> {
    let dir = build_dir().join("limine");
    let stamp = dir.join(".kitty-commit");
    if fs::read_to_string(&stamp).ok().as_deref() == Some(LIMINE_COMMIT) {
        return Ok(dir);
    }
    if dir.exists() {
        fs::remove_dir_all(&dir).map_err(|e| format!("cleaning {}: {e}", dir.display()))?;
    }
    fs::create_dir_all(&dir).map_err(|e| format!("mkdir {}: {e}", dir.display()))?;
    println!("xtask: fetching Limine {LIMINE_COMMIT}");
    let git = |args: &[&str]| run_cmd(Command::new("git").current_dir(&dir).args(args));
    git(&["init", "-q"])?;
    fetch_limine(&dir, run_cmd, std::thread::sleep)?;
    git(&["checkout", "-q", "--detach", LIMINE_COMMIT])?;
    fs::write(&stamp, LIMINE_COMMIT).map_err(|e| format!("writing stamp: {e}"))?;
    Ok(dir)
}

fn fetch_limine(
    dir: &Path,
    mut run: impl FnMut(&mut Command) -> Result<()>,
    mut sleep: impl FnMut(Duration),
) -> Result<()> {
    let mut git = Command::new("git");
    git.current_dir(dir).env("GIT_TERMINAL_PROMPT", "0").args([
        "-c",
        "http.lowSpeedLimit=1",
        "-c",
        "http.lowSpeedTime=20",
        "fetch",
        "-q",
        "--depth=1",
        LIMINE_REPO,
        LIMINE_COMMIT,
    ]);
    for attempt in 1..LIMINE_FETCH_ATTEMPTS {
        match run(&mut git) {
            Ok(()) => return Ok(()),
            Err(error) => {
                eprintln!("xtask: fetching Limine failed ({error}); retrying in {attempt}s");
                sleep(Duration::from_secs(attempt));
            }
        }
    }
    run(&mut git)
        .map_err(|error| format!("fetching Limine after {LIMINE_FETCH_ATTEMPTS} attempts: {error}"))
}

/// Path of the `limine` host tool (used for `bios-install`). The binary
/// branch ships a Windows build; elsewhere we compile it from limine.c.
fn limine_tool(dir: &Path) -> Result<PathBuf> {
    if cfg!(windows) {
        return Ok(dir.join("limine.exe"));
    }
    let tool = dir.join("limine");
    if !tool.exists() {
        run_cmd(Command::new("make").arg("-C").arg(dir))?;
    }
    Ok(tool)
}

const LIMINE_CONF: &str = "\
# kitty-OS boot configuration (Limine). Generated by `cargo xtask`.
timeout: 0

/kitty-OS
    protocol: limine
    path: boot():/boot/kitty-kernel
";

// ------------------------------------------------------------ boot media ---

/// Lays out an EFI System Partition as a plain folder. QEMU can expose a
/// folder as a FAT disk (vvfat), so UEFI boot needs no image tools at all.
fn make_esp(kernel: &Path, archive: Option<&[u8]>, output: &Path) -> Result<PathBuf> {
    let limine = limine_dir()?;
    let esp = output.join("esp");
    if esp.exists() {
        fs::remove_dir_all(&esp).map_err(|e| format!("cleaning {}: {e}", esp.display()))?;
    }
    copy(
        &limine.join("BOOTX64.EFI"),
        &esp.join("EFI/BOOT/BOOTX64.EFI"),
    )?;
    stage_boot_files(&esp, kernel, archive)?;
    Ok(esp)
}

/// Builds a hybrid ISO that boots on both BIOS and UEFI.
fn make_iso(kernel: &Path, archive: Option<&[u8]>, output: &Path) -> Result<PathBuf> {
    let limine = limine_dir()?;
    let iso_root = output.join("iso_root");
    if iso_root.exists() {
        fs::remove_dir_all(&iso_root).map_err(|e| format!("cleaning: {e}"))?;
    }
    stage_boot_files(&iso_root, kernel, archive)?;
    for f in [
        "limine-bios.sys",
        "limine-bios-cd.bin",
        "limine-uefi-cd.bin",
    ] {
        copy(&limine.join(f), &iso_root.join("boot/limine").join(f))?;
    }
    copy(
        &limine.join("BOOTX64.EFI"),
        &iso_root.join("EFI/BOOT/BOOTX64.EFI"),
    )?;

    let iso = output.join("kitty-os.iso");
    let xorriso = env::var("KITTY_XORRISO").unwrap_or_else(|_| "xorriso".into());
    run_cmd(
        Command::new(xorriso)
            .args(["-as", "mkisofs", "-R", "-r", "-J"])
            .args(["-b", "boot/limine/limine-bios-cd.bin"])
            .args(["-no-emul-boot", "-boot-load-size", "4", "-boot-info-table"])
            .args(["-hfsplus", "-apm-block-size", "2048"])
            .args(["--efi-boot", "boot/limine/limine-uefi-cd.bin"])
            .args([
                "-efi-boot-part",
                "--efi-boot-image",
                "--protective-msdos-label",
            ])
            .arg(&iso_root)
            .arg("-o")
            .arg(&iso),
    )
    .map_err(|e| format!("{e} (is xorriso installed? set KITTY_XORRISO)"))?;
    run_cmd(
        Command::new(limine_tool(&limine)?)
            .arg("bios-install")
            .arg(&iso),
    )?;
    println!("iso: {}", iso.display());
    Ok(iso)
}

fn stage_boot_files(media: &Path, kernel: &Path, archive: Option<&[u8]>) -> Result<()> {
    let boot = media.join("boot");
    copy(kernel, &boot.join(KERNEL_NAME))?;
    if let Some(archive) = archive {
        let path = boot.join("initrd.tar");
        fs::write(&path, archive).map_err(|e| format!("writing {}: {e}", path.display()))?;
    }
    let config_dir = boot.join("limine");
    fs::create_dir_all(&config_dir).map_err(|e| format!("mkdir {}: {e}", config_dir.display()))?;
    let config = config_dir.join("limine.conf");
    fs::write(&config, initrd::limine_config(archive.is_some()))
        .map_err(|e| format!("writing {}: {e}", config.display()))
}

// ------------------------------------------------------------------ qemu ---

fn qemu_binary() -> PathBuf {
    if let Ok(q) = env::var("KITTY_QEMU") {
        return q.into();
    }
    let win = Path::new(r"C:\Program Files\qemu\qemu-system-x86_64.exe");
    if cfg!(windows) && win.exists() {
        return win.to_path_buf();
    }
    "qemu-system-x86_64".into()
}

/// Finds UEFI firmware: returns (code, optional vars template).
fn find_ovmf(qemu: &Path) -> Result<(PathBuf, Option<PathBuf>)> {
    if let Ok(code) = env::var("KITTY_OVMF") {
        return Ok((code.into(), None));
    }
    let mut candidates: Vec<(PathBuf, PathBuf)> = Vec::new();
    if let Some(dir) = qemu.parent().filter(|d| !d.as_os_str().is_empty()) {
        let share = dir.join("share");
        candidates.push((
            share.join("edk2-x86_64-code.fd"),
            share.join("edk2-i386-vars.fd"),
        ));
    }
    for (c, v) in [
        (
            "/usr/share/OVMF/OVMF_CODE_4M.fd",
            "/usr/share/OVMF/OVMF_VARS_4M.fd",
        ),
        (
            "/usr/share/OVMF/OVMF_CODE.fd",
            "/usr/share/OVMF/OVMF_VARS.fd",
        ),
        (
            "/usr/share/edk2/x64/OVMF_CODE.4m.fd",
            "/usr/share/edk2/x64/OVMF_VARS.4m.fd",
        ),
        (
            "/usr/share/qemu/edk2-x86_64-code.fd",
            "/usr/share/qemu/edk2-i386-vars.fd",
        ),
        (
            "/opt/homebrew/share/qemu/edk2-x86_64-code.fd",
            "/opt/homebrew/share/qemu/edk2-i386-vars.fd",
        ),
        (
            "/usr/local/share/qemu/edk2-x86_64-code.fd",
            "/usr/local/share/qemu/edk2-i386-vars.fd",
        ),
    ] {
        candidates.push((c.into(), v.into()));
    }
    candidates
        .into_iter()
        .find(|(c, _)| c.exists())
        .map(|(c, v)| (c, v.exists().then_some(v)))
        .ok_or_else(|| "UEFI firmware (OVMF) not found; set KITTY_OVMF or use --bios".into())
}

/// Builds the QEMU command line for the requested boot mode.
fn qemu_media_command(
    opts: &Opts,
    kernel: &Path,
    archive: Option<&[u8]>,
    output: &Path,
) -> Result<Command> {
    let qemu = qemu_binary();
    let mut cmd = Command::new(&qemu);
    cmd.current_dir(root());
    // q35 = modern chipset (PCIe, AHCI). `-cpu max` exposes every feature
    // TCG/KVM can emulate, including SMEP/SMAP/UMIP/NX needed by later phases.
    cmd.args(["-machine", "q35", "-cpu", "max", "-m", "256M", "-no-reboot"]);

    if opts.bios {
        let iso = make_iso(kernel, archive, output)?;
        cmd.arg("-cdrom").arg(iso);
    } else {
        let esp = make_esp(kernel, archive, output)?;
        let (code, vars) = find_ovmf(&qemu)?;
        cmd.arg("-drive").arg(format!(
            "if=pflash,format=raw,unit=0,readonly=on,file={}",
            code.display()
        ));
        if let Some(vars) = vars {
            let local = output.join("ovmf-vars.fd");
            copy(&vars, &local)?;
            cmd.arg("-drive").arg(format!(
                "if=pflash,format=raw,unit=1,file={}",
                local.display()
            ));
        }
        // Relative path on purpose: vvfat's option parser chokes on `C:\`.
        let esp = esp
            .strip_prefix(root())
            .map_err(|e| format!("ESP path: {e}"))?;
        cmd.arg("-drive")
            .arg(format!("format=raw,file=fat:rw:{}", esp.display()));
    }

    Ok(cmd)
}

/// QEMU with the boot media for `kernel`, including the `--initrd` archive
/// if one was given. Callers add the display/serial/exit arguments.
fn qemu_command(opts: &Opts, kernel: &Path) -> Result<Command> {
    let archive = opts.load_initrd()?;
    qemu_media_command(opts, kernel, archive.as_deref(), &build_dir())
}

fn run(opts: &Opts) -> Result<()> {
    let kernel = build_kernel(opts, false)?;
    run_cmd(qemu_command(opts, &kernel)?.args(["-serial", "stdio"]))
}

fn test(opts: &Opts) -> Result<()> {
    boot_test(opts, &build_dir().join("serial-test.log")).map(|_| ())
}

fn add_debug_exit_args(command: &mut Command) {
    command.args(["-display", "none", "-monitor", "none", "-serial", "stdio"]);
    command.args(["-device", "isa-debug-exit,iobase=0xf4,iosize=0x04"]);
}

fn debug_exit_command(opts: &Opts, kernel: &Path) -> Result<Command> {
    let mut command = qemu_command(opts, kernel)?;
    add_debug_exit_args(&mut command);
    Ok(command)
}

fn boot_test(opts: &Opts, log: &Path) -> Result<String> {
    let kernel = build_kernel(opts, true)?;
    boot_test_with_kernel(opts, &kernel, log)
}

fn boot_test_with_kernel(opts: &Opts, kernel: &Path, log: &Path) -> Result<String> {
    let mut command = debug_exit_command(opts, kernel)?;
    finish_boot_test(opts, &mut command, log)
}

/// Boot test with an explicit initrd archive (or none) staged in `output`.
fn boot_test_with_initrd(
    opts: &Opts,
    log: &Path,
    archive: Option<&[u8]>,
    output: &Path,
) -> Result<String> {
    let kernel = build_kernel(opts, true)?;
    let mut command = qemu_media_command(opts, &kernel, archive, output)?;
    add_debug_exit_args(&mut command);
    finish_boot_test(opts, &mut command, log)
}

fn finish_boot_test(opts: &Opts, command: &mut Command, log: &Path) -> Result<String> {
    let mode = if opts.bios { "BIOS/ISO" } else { "UEFI" };
    println!("xtask: boot test ({mode})");

    let output = boot_trace::run(command)?;
    let serial = String::from_utf8_lossy(&output.serial).into_owned();
    println!(
        "----- serial output -----\n{}\n-------------------------",
        serial.trim_end()
    );
    fs::write(log, &serial).map_err(|e| format!("writing {}: {e}", log.display()))?;

    let Some(status) = output.status else {
        return Err(format!("timed out after {}s", TEST_TIMEOUT.as_secs()));
    };
    if !serial.contains(BOOT_MESSAGE) {
        return Err(format!("serial output does not contain {BOOT_MESSAGE:?}"));
    }
    for text in &opts.expect {
        if !serial.contains(text.as_str()) {
            return Err(format!("serial output does not contain {text:?}"));
        }
    }
    if opts.expect_panic {
        if status.code() != Some(QEMU_FAILURE) || !serial.contains(PANIC_MESSAGE) {
            return Err(format!(
                "expected a kernel panic, got QEMU exit status {status}"
            ));
        }
    } else if status.code() != Some(QEMU_SUCCESS) {
        return Err(format!(
            "kernel reported failure (QEMU exit status {status})"
        ));
    }
    println!("xtask: boot test passed ({mode})");
    Ok(serial)
}

#[cfg(test)]
mod limine_tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn retries_only_the_pinned_fetch_with_bounded_backoff() {
        for (failures, expected_calls, expected_delays) in [
            (0, 1, &[][..]),
            (1, 2, &[1]),
            (2, 3, &[1, 2]),
            (3, 3, &[1, 2]),
        ] {
            let mut calls = 0;
            let mut delays = Vec::new();
            let result = fetch_limine(
                Path::new("limine"),
                |git| {
                    assert_eq!(git.get_program(), "git");
                    assert_eq!(git.get_current_dir(), Some(Path::new("limine")));
                    let args: Vec<_> = git.get_args().collect();
                    assert!(args.ends_with(&[OsStr::new(LIMINE_REPO), OsStr::new(LIMINE_COMMIT)]));
                    assert!(args.contains(&OsStr::new("fetch")));
                    assert!(git.get_envs().any(|(key, value)| {
                        key == "GIT_TERMINAL_PROMPT" && value.is_some_and(|value| value == "0")
                    }));
                    calls += 1;
                    if calls <= failures {
                        Err("simulated fetch failure".into())
                    } else {
                        Ok(())
                    }
                },
                |delay| delays.push(delay.as_secs()),
            );
            assert_eq!(calls, expected_calls);
            assert_eq!(delays, expected_delays);
            if failures == 3 {
                assert_eq!(
                    result,
                    Err("fetching Limine after 3 attempts: simulated fetch failure".into())
                );
            } else {
                assert_eq!(result, Ok(()));
            }
        }
    }
}
