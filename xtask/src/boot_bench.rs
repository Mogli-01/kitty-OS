//! QEMU boot benchmark using a monotonic host clock.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::{
    BOOT_COMPLETE, BOOT_MESSAGE, Opts, QEMU_SUCCESS, Result, boot_trace, build_dir, build_kernel,
    debug_exit_command,
};

const SAMPLES: usize = 7;
const MIN_REGRESSION_DELTA: Duration = Duration::from_millis(50);

pub(crate) fn run(opts: &Opts) -> Result<()> {
    let kernel = build_kernel(opts, true)?;
    let baseline = std::env::var_os("KITTY_BENCH_AGAINST").map(PathBuf::from);
    let baseline_median = baseline
        .as_deref()
        .map(|path| measure(opts, path, "baseline", "serial-bench-base.log"))
        .transpose()?;
    let candidate = measure(opts, &kernel, "candidate", "serial-bench-boot.log")?;
    if let Some(baseline) = baseline_median {
        let regression = is_regression(baseline, candidate);
        println!(
            "boot_compare regression={regression} baseline_ms={:.3} candidate_ms={:.3}",
            milliseconds(baseline),
            milliseconds(candidate)
        );
        if regression {
            eprintln!("xtask: warning: boot median increased by at least 25% and 50 ms");
        }
    }
    Ok(())
}

fn measure(opts: &Opts, kernel: &Path, label: &str, log_name: &str) -> Result<Duration> {
    let mut command = debug_exit_command(opts, kernel)?;
    let log = build_dir().join(log_name);
    let firmware = if opts.bios { "bios" } else { "uefi" };
    let profile = if opts.release { "release" } else { "debug" };
    println!("xtask: benchmark {label} ({firmware}, {profile}, {SAMPLES} samples and one warmup)");

    let mut elapsed = [Duration::ZERO; SAMPLES];
    for iteration in 0..=SAMPLES {
        let output = boot_trace::run(&mut command)?;
        fs::write(&log, &output.serial).map_err(|e| format!("writing {}: {e}", log.display()))?;
        if output.status.and_then(|status| status.code()) != Some(QEMU_SUCCESS)
            || !String::from_utf8_lossy(&output.serial).contains(BOOT_MESSAGE)
        {
            return Err(format!(
                "benchmark guest failed before completing boot (see {})",
                log.display()
            ));
        }
        let boot = output.boot_elapsed.ok_or_else(|| {
            format!(
                "benchmark guest did not print {BOOT_COMPLETE:?} (see {})",
                log.display()
            )
        })?;
        if iteration == 0 {
            println!("warmup_ms={:.3}", milliseconds(boot));
            continue;
        }
        elapsed[iteration - 1] = boot;
        println!("sample={iteration} boot_ms={:.3}", milliseconds(boot));
    }
    elapsed.sort_unstable();
    println!(
        "boot_bench firmware={firmware} profile={profile} samples={SAMPLES} min_ms={:.3} median_ms={:.3} max_ms={:.3} kernel={label}",
        milliseconds(elapsed[0]),
        milliseconds(elapsed[SAMPLES / 2]),
        milliseconds(elapsed[SAMPLES - 1])
    );
    Ok(elapsed[SAMPLES / 2])
}

fn is_regression(baseline: Duration, candidate: Duration) -> bool {
    candidate.saturating_sub(baseline) >= MIN_REGRESSION_DELTA
        && candidate.as_nanos() * 4 >= baseline.as_nanos() * 5
}

fn milliseconds(elapsed: Duration) -> f64 {
    elapsed.as_secs_f64() * 1_000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requires_both_the_relative_and_absolute_increase() {
        let ms = Duration::from_millis;
        assert!(is_regression(ms(200), ms(250)));
        assert!(!is_regression(ms(200), ms(249)));
        assert!(!is_regression(ms(100), ms(140)));
        assert!(!is_regression(ms(400), ms(495)));
        assert!(!is_regression(ms(400), ms(200)));
    }
}
