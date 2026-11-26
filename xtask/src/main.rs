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
//!
//! Extra options:
//!   --features <a,b>   extra kernel cargo features (e.g. selftest-page-fault)
//!   --expect-panic     (test) pass only if the kernel panics: QEMU exit code
//!                      35 and "KERNEL PANIC" on the serial port
//!   --expect <text>    (test) the serial output must also contain <text>
//!                      (repeatable)
//!
//! Environment overrides: KITTY_QEMU (qemu binary), KITTY_OVMF (UEFI code
//! firmware), KITTY_XORRISO (xorriso binary).
//!
//! Only std is used on purpose (dependency policy, docs/DECISIONS.md).

use std::env;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

mod memory_tests;
mod visual_test;

/// Limine binary release we boot with. Pinned to an exact commit of the
/// official `v11.x-binary` branch so a compromised or changed branch cannot
/// silently change our bootloader (supply-chain safety). Bump deliberately.
const LIMINE_REPO: &str = "https://github.com/limine-bootloader/limine.git";
const LIMINE_COMMIT: &str = "5be26a73d7b7b4d4477d18be94e1d16e615adf56";

const KERNEL_NAME: &str = "kitty-kernel";
const BOOT_MESSAGE: &str = "Hello from kitty-OS";
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

    let result = match cmd.as_str() {
        "build" => build_kernel(&opts, false).map(|p| println!("kernel: {}", p.display())),
        "esp" => build_kernel(&opts, false)
            .and_then(|k| make_esp(&k))
            .map(|_| ()),
        "iso" => build_kernel(&opts, false)
            .and_then(|k| make_iso(&k))
            .map(|_| ()),
        "run" => run(&opts),
        "test" => test(&opts),
        "test-memory" => memory_tests::run(&opts),
        "test-visual" => visual_test::run(&opts),
        _ => {
            eprintln!(
                "usage: cargo xtask <build|esp|iso|run|test> [--release] [--bios] [--features a,b] [--expect-panic] [--expect text]"
            );
            eprintln!("       cargo xtask test-memory [--release] [--bios]");
            eprintln!("       cargo xtask test-visual [--release] [--bios]");
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
    git(&["fetch", "-q", "--depth=1", LIMINE_REPO, LIMINE_COMMIT])?;
    git(&["checkout", "-q", "FETCH_HEAD"])?;
    fs::write(&stamp, LIMINE_COMMIT).map_err(|e| format!("writing stamp: {e}"))?;
    Ok(dir)
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
fn make_esp(kernel: &Path) -> Result<PathBuf> {
    let limine = limine_dir()?;
    let esp = build_dir().join("esp");
    if esp.exists() {
        fs::remove_dir_all(&esp).map_err(|e| format!("cleaning {}: {e}", esp.display()))?;
    }
    copy(
        &limine.join("BOOTX64.EFI"),
        &esp.join("EFI/BOOT/BOOTX64.EFI"),
    )?;
    copy(kernel, &esp.join("boot").join(KERNEL_NAME))?;
    write_conf(&esp.join("boot/limine/limine.conf"))?;
    Ok(esp)
}

/// Builds a hybrid ISO that boots on both BIOS and UEFI.
fn make_iso(kernel: &Path) -> Result<PathBuf> {
    let limine = limine_dir()?;
    let iso_root = build_dir().join("iso_root");
    if iso_root.exists() {
        fs::remove_dir_all(&iso_root).map_err(|e| format!("cleaning: {e}"))?;
    }
    copy(kernel, &iso_root.join("boot").join(KERNEL_NAME))?;
    write_conf(&iso_root.join("boot/limine/limine.conf"))?;
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

    let iso = build_dir().join("kitty-os.iso");
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

fn write_conf(path: &Path) -> Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("mkdir: {e}"))?;
    }
    fs::write(path, LIMINE_CONF).map_err(|e| format!("writing {}: {e}", path.display()))
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
fn qemu_command(opts: &Opts, kernel: &Path) -> Result<Command> {
    let qemu = qemu_binary();
    let mut cmd = Command::new(&qemu);
    cmd.current_dir(root());
    // q35 = modern chipset (PCIe, AHCI). `-cpu max` exposes every feature
    // TCG/KVM can emulate, including SMEP/SMAP/UMIP/NX needed by later phases.
    cmd.args(["-machine", "q35", "-cpu", "max", "-m", "256M", "-no-reboot"]);

    if opts.bios {
        let iso = make_iso(kernel)?;
        cmd.arg("-cdrom").arg(iso);
    } else {
        make_esp(kernel)?;
        let (code, vars) = find_ovmf(&qemu)?;
        cmd.arg("-drive").arg(format!(
            "if=pflash,format=raw,unit=0,readonly=on,file={}",
            code.display()
        ));
        if let Some(vars) = vars {
            let local = build_dir().join("ovmf-vars.fd");
            copy(&vars, &local)?;
            cmd.arg("-drive").arg(format!(
                "if=pflash,format=raw,unit=1,file={}",
                local.display()
            ));
        }
        // Relative path on purpose: vvfat's option parser chokes on `C:\`.
        cmd.args(["-drive", "format=raw,file=fat:rw:build/esp"]);
    }

    Ok(cmd)
}

fn run(opts: &Opts) -> Result<()> {
    let kernel = build_kernel(opts, false)?;
    run_cmd(qemu_command(opts, &kernel)?.args(["-serial", "stdio"]))
}

fn test(opts: &Opts) -> Result<()> {
    boot_test(opts, &build_dir().join("serial-test.log")).map(|_| ())
}

fn boot_test(opts: &Opts, log: &Path) -> Result<String> {
    let kernel = build_kernel(opts, true)?;
    let mut cmd = qemu_command(opts, &kernel)?;
    cmd.args(["-display", "none", "-monitor", "none", "-serial", "stdio"]);
    cmd.args(["-device", "isa-debug-exit,iobase=0xf4,iosize=0x04"]);
    let mode = if opts.bios { "BIOS/ISO" } else { "UEFI" };
    println!("xtask: boot test ({mode})");

    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("cannot start QEMU: {e}"))?;

    // Read serial output on a thread so a chatty kernel cannot block QEMU on
    // a full pipe while we wait for the timeout.
    let mut stdout = child.stdout.take().ok_or("no QEMU stdout")?;
    let reader = std::thread::spawn(move || {
        let mut out = Vec::new();
        let _ = stdout.read_to_end(&mut out);
        out
    });

    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if start.elapsed() > TEST_TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => return Err(format!("waiting for QEMU: {e}")),
        }
    };
    let serial = String::from_utf8_lossy(&reader.join().unwrap_or_default()).into_owned();
    println!(
        "----- serial output -----\n{}\n-------------------------",
        serial.trim_end()
    );
    fs::write(log, &serial).map_err(|e| format!("writing {}: {e}", log.display()))?;

    let Some(status) = status else {
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
