//! Repeated measurements from the guest's cooperative scheduler self-test.

use std::num::NonZeroU64;

use crate::{Opts, Result, boot_test_with_kernel, build_dir, build_kernel};

const SAMPLES: usize = 7;
const PREFIX: &str = "[sched] cooperative self-test passed: ";

pub(crate) fn run(opts: &Opts) -> Result<()> {
    if !opts.features.is_empty() || opts.expect_panic || !opts.expect.is_empty() {
        return Err("bench-scheduler supports only --release and --bios".into());
    }

    let kernel = build_kernel(opts, true)?;
    let log = build_dir().join("serial-bench-scheduler.log");
    let firmware = if opts.bios { "bios" } else { "uefi" };
    let profile = if opts.release { "release" } else { "debug" };
    println!(
        "xtask: benchmark cooperative scheduling ({firmware}, {profile}, {SAMPLES} samples and one warmup)"
    );

    let mut values = [0; SAMPLES];
    for iteration in 0..=SAMPLES {
        let serial = boot_test_with_kernel(opts, &kernel, &log)?;
        let cycles = cycles_per_yield_switch(&serial)
            .map_err(|error| format!("{error} (see {})", log.display()))?;
        if iteration == 0 {
            println!("warmup_tsc_cycles={cycles}");
            continue;
        }
        values[iteration - 1] = cycles;
        println!("sample={iteration} tsc_cycles_per_yield_switch={cycles}");
    }

    values.sort_unstable();
    println!(
        "scheduler_bench firmware={firmware} profile={profile} samples={SAMPLES} min_tsc_cycles={} median_tsc_cycles={} max_tsc_cycles={}",
        values[0],
        values[SAMPLES / 2],
        values[SAMPLES - 1],
    );
    Ok(())
}

fn cycles_per_yield_switch(serial: &str) -> std::result::Result<u64, &'static str> {
    let mut readings = serial.lines().filter_map(|line| line.strip_prefix(PREFIX));
    let reading = readings.next().ok_or("scheduler measurement missing")?;
    if readings.next().is_some() {
        return Err("multiple scheduler measurements in one boot");
    }
    let (switches, cycles) = reading
        .strip_suffix(" cycles per yield+switch")
        .and_then(|reading| reading.split_once(" switches so far, ~"))
        .ok_or("invalid scheduler measurement format")?;
    switches
        .parse::<NonZeroU64>()
        .map_err(|_| "invalid context-switch count")?;
    cycles
        .parse::<NonZeroU64>()
        .map(NonZeroU64::get)
        .map_err(|_| "invalid scheduler cycle count")
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str =
        "[sched] cooperative self-test passed: 3003 switches so far, ~1054 cycles per yield+switch";

    #[test]
    fn extracts_one_cooperative_result_from_crlf_serial() {
        let serial =
            format!("Hello from kitty-OS\r\n{VALID}\r\n[sched] wait queue self-test passed");
        assert_eq!(cycles_per_yield_switch(&serial), Ok(1054));
    }

    #[test]
    fn rejects_missing_duplicate_and_invalid_results() {
        assert_eq!(
            cycles_per_yield_switch("[sched] wait queue self-test passed"),
            Err("scheduler measurement missing"),
        );
        assert_eq!(
            cycles_per_yield_switch(&format!("{VALID}\n{VALID}\n")),
            Err("multiple scheduler measurements in one boot"),
        );
        for (serial, error) in [
            (
                VALID.replace("yield+switch", "switch"),
                "invalid scheduler measurement format",
            ),
            (VALID.replace("3003", "0"), "invalid context-switch count"),
            (VALID.replace("1054", "0"), "invalid scheduler cycle count"),
            (
                VALID.replace("1054", "18446744073709551616"),
                "invalid scheduler cycle count",
            ),
        ] {
            assert_eq!(cycles_per_yield_switch(&serial), Err(error));
        }
    }
}
