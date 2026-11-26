//! QEMU boot benchmark using a monotonic host clock.

use std::fs;
use std::time::Duration;

use crate::{
    BOOT_COMPLETE, BOOT_MESSAGE, Opts, QEMU_SUCCESS, Result, boot_trace, build_dir, build_kernel,
    debug_exit_command,
};

const SAMPLES: usize = 7;

pub(crate) fn run(opts: &Opts) -> Result<()> {
    let kernel = build_kernel(opts, true)?;
    let mut command = debug_exit_command(opts, &kernel)?;
    let firmware = if opts.bios { "bios" } else { "uefi" };
    let profile = if opts.release { "release" } else { "debug" };
    println!("xtask: benchmark boot ({firmware}, {profile}, {SAMPLES} samples and one warmup)");

    let mut elapsed = [Duration::ZERO; SAMPLES];
    for iteration in 0..=SAMPLES {
        let output = boot_trace::run(&mut command)?;
        let log = build_dir().join("serial-bench-boot.log");
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
        "boot_bench firmware={firmware} profile={profile} samples={SAMPLES} min_ms={:.3} median_ms={:.3} max_ms={:.3}",
        milliseconds(elapsed[0]),
        milliseconds(elapsed[SAMPLES / 2]),
        milliseconds(elapsed[SAMPLES - 1])
    );
    Ok(())
}

fn milliseconds(elapsed: Duration) -> f64 {
    elapsed.as_secs_f64() * 1_000.0
}
