//! Boot self-test of user mode (Phase 6b): real ring-3 programs that must
//! be stopped by the CPU protections and killed, never crash the kernel;
//! register and FPU hygiene; preemption of user code; memory reclaimed.

use alloc::sync::Arc;

use super::{DATA_BASE, ProcError, Process, Status};
use crate::arch::x86_64::cpu;
use crate::arch::x86_64::usermode::{TestProgram, test_program};
use crate::mm::pmm;
use crate::sched;

const VEC_UD: u8 = 6;
const VEC_GP: u8 = 13;
const VEC_PF: u8 = 14;

/// What the self-test saw (for the boot log; no addresses).
#[derive(Debug)]
pub struct Report {
    /// Fault programs run and killed with the expected exception.
    pub faults_ok: u32,
    /// Whether the spinning programs ran (needs a timer to preempt them).
    pub preemption_tested: bool,
    /// Whether `sgdt` was stopped by UMIP (else UMIP is unavailable).
    pub umip: bool,
}

/// Runs the self-test. `Err` names the first check that failed.
pub fn run() -> Result<Report, &'static str> {
    let umip = cpu::umip_enabled();
    // Warm-up: the first process allocates scheduler and heap structures
    // that stay allocated; measure leaks only after it.
    run_to_fault(TestProgram::NullRead).map_err(|_| "could not start a user process")?;
    settle();
    let free_before = pmm::stats().free_frames;
    let (gs_ok_before, gs_bad_before) = crate::arch::x86_64::swapgs_checks();

    let cases: [(TestProgram, u8, &str); 6] = [
        (TestProgram::NullRead, VEC_PF, "null read was not a #PF"),
        (
            TestProgram::KernelRead,
            VEC_PF,
            "kernel-half read was not a #PF",
        ),
        (
            TestProgram::Sgdt,
            if umip { VEC_GP } else { VEC_UD },
            "sgdt was not stopped as expected",
        ),
        (TestProgram::Cli, VEC_GP, "cli in ring 3 was not a #GP"),
        (
            TestProgram::PortIo,
            VEC_GP,
            "port I/O in ring 3 was not a #GP",
        ),
        (
            TestProgram::Int3,
            VEC_GP,
            "int3 through a DPL-0 gate was not a #GP",
        ),
    ];
    let mut faults_ok = 0;
    for (program, vector, why) in cases {
        let status = run_to_fault(program).map_err(|_| "could not start a user process")?;
        if status != Status::Fault(vector) {
            return Err(why);
        }
        faults_ok += 1;
    }

    let (gs_ok, gs_bad) = crate::arch::x86_64::swapgs_checks();
    if gs_bad != gs_bad_before || gs_ok < gs_ok_before + u64::from(faults_ok) {
        return Err("GS base was not the kernel's after a trap from ring 3");
    }

    // `spawn_user` must refuse entry points outside the user half (an
    // `iretq` to them would fault in ring 0).
    let probe = Process::from_test_image(test_program(TestProgram::NullRead))
        .map_err(|_| "could not create a user process")?;
    let refused = sched::spawn_user(
        "selftest-bad",
        Arc::clone(&probe),
        0xFFFF_8000_0000_0000,
        super::STACK_TOP,
    ) == Err(sched::SpawnError::BadUserAddress)
        && sched::spawn_user("selftest-bad", Arc::clone(&probe), super::CODE_BASE, 0x1000)
            == Err(sched::SpawnError::BadUserAddress);
    drop(probe);
    if !refused {
        return Err("spawn_user accepted a non-user entry point or stack");
    }

    let preemption_tested = timer_running();
    if preemption_tested {
        hygiene_and_preemption()?;
    }

    settle();
    if pmm::stats().free_frames != free_before {
        return Err("frames leaked by dead processes");
    }
    Ok(Report {
        faults_ok,
        preemption_tested,
        umip,
    })
}

/// Checks console I/O and exit through the actual Ring 3 syscall entry.
#[cfg(feature = "qemu-exit")]
pub(crate) fn console() -> Result<(), &'static str> {
    {
        let process = Process::from_test_image(test_program(TestProgram::Console))
            .map_err(|_| "could not create console test process")?;
        process.grant_console();
        let thread = super::start(&process, "selftest-console")
            .map_err(|_| "could not start console test process")?;
        if !wait_until(|| sched::is_blocked(thread), 100_000) {
            return Err("console read did not block");
        }
        crate::console::input_push(b"\n");
        if !wait_until(|| process.status() != Status::Running, 100_000)
            || process.status() != Status::Exited(42)
        {
            return Err("console test process did not finish successfully");
        }
    }
    settle();
    Ok(())
}

/// Runs a program that is expected to fault; returns how it ended.
fn run_to_fault(program: TestProgram) -> Result<Status, ProcError> {
    let process = Process::from_test_image(test_program(program))?;
    super::start(&process, "selftest-user")?;
    wait_until(|| process.status() != Status::Running, 100_000);
    Ok(process.status())
}

/// A process that leaves a secret in its SSE registers runs alongside a
/// fresh one that checks all its registers are zero: no leak across a
/// context switch. Both spin, so they must be preempted by the timer, and
/// are then killed.
fn hygiene_and_preemption() -> Result<(), &'static str> {
    let secret = Process::from_test_image(test_program(TestProgram::Secret))
        .map_err(|_| "could not create the secret process")?;
    super::start(&secret, "selftest-secret").map_err(|_| "could not start the secret process")?;
    if !wait_until(|| counter(&secret) > 1000, 2_000_000) {
        return Err("a spinning user process never ran");
    }
    let clean = Process::from_test_image(test_program(TestProgram::Clean))
        .map_err(|_| "could not create the clean process")?;
    super::start(&clean, "selftest-clean").map_err(|_| "could not start the clean process")?;
    let both_ran = wait_until(|| counter(&clean) > 1000, 2_000_000);
    let before = counter(&secret);
    // Both keep running between our yields only if the timer preempts them.
    let both_progress = wait_until(|| counter(&secret) > before + 1000, 2_000_000);
    let leaked = clean.space().peek_u64(DATA_BASE);
    secret.kill();
    clean.kill();
    let ended = wait_until(
        || secret.status() == Status::Killed && clean.status() == Status::Killed,
        2_000_000,
    );
    if !both_ran || !both_progress {
        return Err("user processes were not preempted");
    }
    if leaked != Some(0) {
        return Err("registers or selectors leaked into a new user process");
    }
    if !ended {
        return Err("killed processes did not end");
    }
    drop((secret, clean));
    Ok(())
}

/// The spinning programs' counter (DATA + 8).
fn counter(process: &Arc<Process>) -> u64 {
    process.space().peek_u64(DATA_BASE + 8).unwrap_or(0)
}

/// Yields until `done()` or `rounds` yields.
fn wait_until(mut done: impl FnMut() -> bool, rounds: u32) -> bool {
    for _ in 0..rounds {
        if done() {
            return true;
        }
        sched::yield_now();
    }
    done()
}

/// Lets dead threads be reaped (each `schedule` frees the previous one).
fn settle() {
    for _ in 0..4 {
        sched::yield_now();
    }
}

/// Whether a timer drives the scheduler (some self-test builds start none;
/// then spinning user code would never give the CPU back).
fn timer_running() -> bool {
    let start = sched::ticks();
    let t0 = cpu::rdtsc();
    while cpu::rdtsc().wrapping_sub(t0) < 300_000_000 {
        if sched::ticks() > start + 1 {
            return true;
        }
        core::hint::spin_loop();
    }
    false
}
