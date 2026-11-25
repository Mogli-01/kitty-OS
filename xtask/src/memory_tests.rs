//! QEMU checks for the memory protections available in Phase 3.

use std::fs;
use std::path::Path;

use super::{Opts, PANIC_MESSAGE, Result, boot_test, build_dir};

const PAGE_FAULT: &str = "=== Page Fault (#PF) (vector 14)";
const DOUBLE_FAULT: &[&str] = &[
    "=== Double Fault (#DF) (vector 8)",
    "(running on the IST double-fault stack)",
];
const WRITE_PROTECTION: &[&str] = &[PAGE_FAULT, "protection violation on write in kernel mode"];
const FAULTS: &[(&str, &[&str])] = &[
    (
        "selftest-page-fault",
        &[PAGE_FAULT, "non-present page on read in kernel mode"],
    ),
    ("selftest-double-fault", DOUBLE_FAULT),
    ("selftest-stack-overflow", DOUBLE_FAULT),
    ("selftest-write-text", WRITE_PROTECTION),
    (
        "selftest-exec-data",
        &[
            PAGE_FAULT,
            "protection violation on instruction fetch in kernel mode",
        ],
    ),
    ("selftest-write-idt", WRITE_PROTECTION),
    (
        "selftest-heap-overflow",
        &[PAGE_FAULT, "non-present page on write in kernel mode"],
    ),
];

pub(super) fn run(opts: &Opts) -> Result<()> {
    if opts.expect_panic || !opts.expect.is_empty() || !opts.features.is_empty() {
        return Err("test-memory selects the features and expectations for each case".into());
    }

    let firmware = if opts.bios { "bios" } else { "uefi" };
    let profile = if opts.release { "release" } else { "debug" };
    let logs = build_dir()
        .join("memory-tests")
        .join(format!("{firmware}-{profile}"));
    fs::create_dir_all(&logs).map_err(|e| format!("creating {}: {e}", logs.display()))?;

    run_case("boot", opts, &logs, &[])?;
    for &(feature, report) in FAULTS {
        let fault_opts = Opts {
            bios: opts.bios,
            release: opts.release,
            features: vec![feature.into()],
            expect_panic: true,
            expect: report.iter().map(|marker| (*marker).to_owned()).collect(),
        };
        run_case(feature, &fault_opts, &logs, report)?;
    }
    println!("xtask: memory tests passed ({firmware}, {profile})");
    Ok(())
}

fn run_case(name: &str, opts: &Opts, logs: &Path, report: &[&str]) -> Result<()> {
    let log = logs.join(format!("{name}.log"));
    // Clear a previous run's log even if building or starting QEMU fails.
    fs::File::create(&log).map_err(|e| format!("creating {}: {e}", log.display()))?;
    println!("xtask: memory test {name}");
    boot_test(opts, &log)
        .and_then(|serial| check_serial(&serial, report))
        .map_err(|e| format!("{name}: {e} (log: {})", log.display()))
}

fn check_serial(serial: &str, report: &[&str]) -> Result<()> {
    let mut lines = serial.lines();
    if !lines.by_ref().any(|line| {
        line.starts_with("[mm] physical memory: ")
            && line.ends_with("; frame allocator self-test passed")
    }) {
        return Err("missing physical frame allocator self-test success".into());
    }
    if !lines.by_ref().any(|line| {
        line.strip_prefix("[mm] W^X audit: ")
            .and_then(|audit| {
                audit.strip_suffix(" mappings, 0 writable+executable, 0 user pages in kernel half")
            })
            .and_then(|count| count.parse::<u64>().ok())
            .is_some_and(|count| count > 0)
    }) {
        return Err("missing a passing W^X and kernel user-page audit".into());
    }

    // Find the fault only after the kernel reached the guarded stack and completed MMIO checks.
    let boot_markers = [
        "[mm] running on the kernel stack (guard page below it)",
        "heap self-test passed",
        "[mm] MMIO self-test passed",
    ];
    for marker in boot_markers.into_iter().chain(report.iter().copied()) {
        if !lines.by_ref().any(|line| line.contains(marker)) {
            return Err(format!("missing serial marker {marker:?}"));
        }
    }
    if !report.is_empty() && !lines.any(|line| line.contains(PANIC_MESSAGE)) {
        return Err("missing kernel panic after the expected fault report".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOOT: &str = "\
[mm] physical memory: 253 MiB managed, 252 MiB free; frame allocator self-test passed
[mm] W^X audit: 1553 mappings, 0 writable+executable, 0 user pages in kernel half
[mm] running on the kernel stack (guard page below it)
[mm] kernel heap ready (12 pages mapped); heap self-test passed
[mm] MMIO self-test passed (local APIC version 0x14)\n";
    const WRITE_FAULT: &str = "\
=== Page Fault (#PF) (vector 14) error_code=0x3
    cr2=<address>: protection violation on write in kernel mode
*** KERNEL PANIC\n";

    #[test]
    fn accepts_boot_and_expected_fault_with_crlf() {
        assert!(check_serial(BOOT, &[]).is_ok());
        let log = format!("{BOOT}{WRITE_FAULT}").replace('\n', "\r\n");
        assert!(check_serial(&log, WRITE_PROTECTION).is_ok());
    }

    #[test]
    fn rejects_empty_and_failed_audits() {
        for invalid in [
            BOOT.replace("1553 mappings", "0 mappings"),
            BOOT.replace("0 writable+executable", "10 writable+executable"),
            BOOT.replace("0 user pages", "1 user pages"),
        ] {
            assert!(check_serial(&invalid, &[]).is_err());
        }
    }

    #[test]
    fn rejects_wrong_fault_or_panic_order() {
        let wrong_access = WRITE_FAULT.replace("on write", "on instruction fetch");
        assert!(check_serial(&format!("{BOOT}{wrong_access}"), WRITE_PROTECTION).is_err());
        assert!(check_serial(&format!("{WRITE_FAULT}{BOOT}"), WRITE_PROTECTION).is_err());
        let missing_panic = WRITE_FAULT.replace(PANIC_MESSAGE, "");
        assert!(check_serial(&format!("{BOOT}{missing_panic}"), WRITE_PROTECTION).is_err());
    }
}
