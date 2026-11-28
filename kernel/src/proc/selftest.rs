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
    /// Average TSC cycles of a null system call, measured in ring 3.
    pub syscall_cycles: u64,
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

    let syscall_cycles = syscalls()?;
    blocked_then_killed()?;
    elf()?;
    spawn_rules()?;
    input()?;
    random()?;
    aslr()?;
    if !super::fs::selftest_paths() {
        return Err("open_at accepted a path that leaves its directory");
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
        syscall_cycles,
    })
}

/// Expected results of the `Syscalls` program, slot by slot.
fn expected_syscall_results() -> [(i64, &'static str); 13] {
    use crate::syscall::errno::*;
    const MSG_LEN: i64 = "[user] hello from ring 3 via syscall\n".len() as i64;
    [
        (MSG_LEN, "console_write did not accept the whole message"),
        (
            E_FAULT,
            "console_write with a kernel pointer was not E_FAULT",
        ),
        (E_TOOBIG, "console_write of 5000 bytes was not E_TOOBIG"),
        (E_BADHANDLE, "console_write on handle 7 was not E_BADHANDLE"),
        (1, "handle_dup(0, READ) did not return handle 1"),
        (E_PERM, "writing through a READ-only handle was not E_PERM"),
        ((1 << 32) | 1, "handle_info did not report CONSOLE + READ"),
        (0, "handle_close failed"),
        (E_BADHANDLE, "a closed handle still worked"),
        (E_INVAL, "an undefined right bit was not E_INVAL"),
        (E_PERM, "handle_dup could add a right"),
        (E_NOSYS, "an unknown system call was not E_NOSYS"),
        (6, "console_read did not return the 6 pushed bytes"),
    ]
}

/// Runs the `Syscalls` program: every call and error path, input read,
/// registers preserved, `exit(42)`. Returns the cost of a null call.
fn syscalls() -> Result<u64, &'static str> {
    crate::console::input_push(b"kitty\n");
    let process = Process::from_test_image(test_program(TestProgram::Syscalls))
        .map_err(|_| "could not create the syscall process")?;
    super::start(&process, "selftest-syscalls")
        .map_err(|_| "could not start the syscall process")?;
    wait_until(|| process.status() != Status::Running, 1_000_000);
    if process.status() != Status::Exited(42) {
        return Err("the syscall process did not exit(42)");
    }
    let slot = |i: u64| {
        process
            .peek_u64(DATA_BASE + 8 * i)
            .map_or(i64::MIN, |v| v as i64)
    };
    for (i, (want, why)) in expected_syscall_results().into_iter().enumerate() {
        if slot(i as u64) != want {
            return Err(why);
        }
    }
    if slot(14) != 0x1234 {
        return Err("a register changed across a system call");
    }
    let echoed = process.peek_u64(DATA_BASE + 0x800);
    if echoed != Some(u64::from_le_bytes(*b"kitty\n\0\0")) {
        return Err("console_read did not deliver the pushed bytes");
    }
    Ok((slot(13).max(0) as u64) / 1000)
}

/// A process blocked in `console_read` must still die when killed.
fn blocked_then_killed() -> Result<(), &'static str> {
    let process = Process::from_test_image(test_program(TestProgram::Reader))
        .map_err(|_| "could not create the reader process")?;
    let thread = super::start(&process, "selftest-reader")
        .map_err(|_| "could not start the reader process")?;
    if !wait_until(|| sched::is_blocked(thread), 1_000_000) {
        return Err("the reader never blocked in console_read");
    }
    process.kill();
    if !wait_until(|| process.status() == Status::Killed, 1_000_000) {
        return Err("a process blocked in a system call could not be killed");
    }
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
    let leaked = clean.peek_u64(DATA_BASE);
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
    process.peek_u64(DATA_BASE + 8).unwrap_or(0)
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

// ---------------------------------------------------------------- 6d ---

use super::elf::{ElfError, Image, build_test_elf};
use super::handles::{Object, Rights};
use super::spawn::{Grant, SPAWN_COPY, SPAWN_MOVE, SpawnFailure};

const RW: u32 = 4 | 2;
const RX: u32 = 4 | 1;

/// The ELF loader refuses every malformed image below with the right
/// error, and runs a valid one (code + .bss + 64 KiB stack) to `exit(7)`.
pub fn elf() -> Result<(), &'static str> {
    let code = test_program(TestProgram::ElfMain);
    let good = build_test_elf(
        &[(0x40_0000, RX, code, 0x1000), (0x40_2000, RW, &[], 0x1000)],
        0x40_0000,
    );
    Image::parse(&good).map_err(|_| "a valid ELF image was refused")?;

    let mutate = |at: usize, bytes: &[u8]| {
        let mut img = good.clone();
        if let Some(d) = img.get_mut(at..at + bytes.len()) {
            d.copy_from_slice(bytes);
        }
        img
    };
    let ph1 = 64 + 56; // second program header
    let cases: [(alloc::vec::Vec<u8>, ElfError, &str); 12] = [
        (
            mutate(0, b"\x7fELG"),
            ElfError::BadMagic,
            "bad magic accepted",
        ),
        (
            mutate(4, &[1]),
            ElfError::Unsupported,
            "32-bit ELF accepted",
        ),
        (
            mutate(18, &3u16.to_le_bytes()),
            ElfError::Unsupported,
            "non-x86_64 ELF accepted",
        ),
        (
            mutate(16, &1u16.to_le_bytes()),
            ElfError::NotExecutable,
            "ET_REL accepted",
        ),
        (
            mutate(56, &65u16.to_le_bytes()),
            ElfError::BadProgramHeaders,
            "too many program headers accepted",
        ),
        (
            mutate(32, &u64::MAX.to_le_bytes()),
            ElfError::BadProgramHeaders,
            "headers outside the file accepted",
        ),
        (
            mutate(ph1 + 4, &7u32.to_le_bytes()),
            ElfError::WriteAndExecute,
            "a W+X segment was accepted",
        ),
        (
            mutate(ph1 + 32, &0x2000u64.to_le_bytes()),
            ElfError::BadSegment,
            "filesz > memsz accepted",
        ),
        (
            mutate(ph1 + 16, &0xFFFF_8000_0000_0000u64.to_le_bytes()),
            ElfError::OutOfRange,
            "a kernel-half segment was accepted",
        ),
        (
            mutate(ph1 + 16, &0x40_0000u64.to_le_bytes()),
            ElfError::Overlap,
            "overlapping segments accepted",
        ),
        (
            mutate(24, &0x40_2000u64.to_le_bytes()),
            ElfError::BadEntry,
            "entry outside executable code accepted",
        ),
        (
            mutate(ph1 + 16, &0x40_2008u64.to_le_bytes()),
            ElfError::BadSegment,
            "vaddr/offset misalignment accepted",
        ),
    ];
    for (img, want, why) in cases {
        if Image::parse(&img).err() != Some(want) {
            return Err(why);
        }
    }
    let low = build_test_elf(&[(0x1000, RX, code, 0x1000)], 0x1000);
    if Image::parse(&low).err() != Some(ElfError::OutOfRange) {
        return Err("a segment below 1 MiB was accepted");
    }
    let top = build_test_elf(&[(0x7FFF_FFFF_F000, RX, code, 0x1000)], 0x7FFF_FFFF_F000);
    if Image::parse(&top).err() != Some(ElfError::OutOfRange) {
        return Err("a segment in the last user page was accepted");
    }
    let guard = super::STACK_TOP - (super::spawn::STACK_PAGES + 1) * 4096;
    let on_guard = build_test_elf(&[(guard, RX, code, 0x1000)], guard);
    if Image::parse(&on_guard).err() != Some(ElfError::OutOfRange) {
        return Err("a segment on the stack guard page was accepted");
    }
    let process = super::spawn::spawn_test_from_kernel(
        &good,
        alloc::vec![(Object::Console, super::CONSOLE_RIGHTS)],
    )
    .map_err(|_| "the valid ELF image could not be started")?;
    wait_until(|| process.status() != Status::Running, 1_000_000);
    if process.status() != Status::Exited(7) {
        return Err("the ELF image did not run to exit(7)");
    }
    Ok(())
}

/// `process_spawn` rules (#79) through the kernel API: copy needs DUP,
/// move needs TRANSFER, a moved handle leaves the parent, rights never
/// grow, a moved handle listed twice is refused, and a failed spawn leaves
/// the parent exactly as it was.
pub fn spawn_rules() -> Result<(), &'static str> {
    let code = test_program(TestProgram::ElfMain);
    let image = build_test_elf(
        &[(0x40_0000, RX, code, 0x1000), (0x40_2000, RW, &[], 0x1000)],
        0x40_0000,
    );
    let parent = Process::from_test_image(test_program(TestProgram::NullRead))
        .map_err(|_| "could not create the parent")?;
    // Parent handles: 0 = CONSOLE (READ|WRITE|DUP|TRANSFER, from the
    // test image), 1 = CONSOLE READ only (no DUP, no TRANSFER).
    let weak = parent
        .handles()
        .lock()
        .insert(Object::Console, Rights::READ)
        .map_err(|_| "could not add a handle")?;
    let g = |handle, rights, flags| Grant {
        handle,
        rights,
        flags,
    };
    let try_spawn = |grants: &[Grant]| super::spawn::spawn(&parent, &image, grants).map(|_| ());
    if try_spawn(&[g(weak, 1, SPAWN_COPY)]) != Err(SpawnFailure::Perm) {
        return Err("SPAWN_COPY without DUP was accepted");
    }
    if try_spawn(&[g(weak, 1, SPAWN_MOVE)]) != Err(SpawnFailure::Perm) {
        return Err("SPAWN_MOVE without TRANSFER was accepted");
    }
    if try_spawn(&[g(0, 0x3F, SPAWN_COPY)]) != Err(SpawnFailure::Perm) {
        return Err("a spawn grant added rights");
    }
    if try_spawn(&[g(0, 1, SPAWN_MOVE), g(0, 1, SPAWN_COPY)]) != Err(SpawnFailure::Invalid) {
        return Err("a moved handle listed twice was accepted");
    }
    if try_spawn(&[g(0, 1, 7)]) != Err(SpawnFailure::Invalid) {
        return Err("unknown grant flags were accepted");
    }
    // A failing image after valid grants: the parent keeps everything.
    let before = parent.handles().lock().len();
    let refused = super::spawn::spawn(&parent, b"not an elf", &[g(0, 1, SPAWN_MOVE)]);
    if !matches!(refused, Err(SpawnFailure::Elf(_)))
        || parent.handles().lock().len() != before
        || parent.handles().lock().get(0).is_err()
    {
        return Err("a failed spawn changed the parent's handles");
    }
    // A real move: the child gets CONSOLE as handle 0, the parent loses it
    // and gets a PROCESS handle instead.
    let (h, child) = super::spawn::spawn(
        &parent,
        &image,
        &[g(0, super::CONSOLE_RIGHTS.bits(), SPAWN_MOVE)],
    )
    .map_err(|_| "a valid spawn failed")?;
    let parent_lost = parent.handles().lock().get(0).is_err();
    let parent_has_child =
        matches!(parent.handles().lock().get(h), Ok((Object::Process(_), r)) if r == Rights::READ);
    wait_until(|| child.status() != Status::Running, 1_000_000);
    if !parent_lost || !parent_has_child {
        return Err("SPAWN_MOVE did not move the handle");
    }
    if child.status() != Status::Exited(7) {
        return Err("the spawned child did not run with the moved CONSOLE");
    }
    drop((parent, child));
    Ok(())
}

// ---------------------------------------------------------------- 6e ---

/// INPUT: one grant at a time, events reach the holder in order, a full
/// ring is reported as "lost" first, the handle cannot be duplicated, and
/// when the holder dies the ring is emptied and the grant comes back.
pub fn input() -> Result<(), &'static str> {
    use crate::input::{self, InputEvent, KIND_KEY, KIND_LOST};
    let code = test_program(TestProgram::InputReader);
    let image = build_test_elf(
        &[(0x40_0000, RX, code, 0x1000), (0x40_1000, RW, &[], 0x1000)],
        0x40_0000,
    );
    let grant = input::grant().ok_or("the INPUT grant was not available")?;
    if input::grant().is_some() {
        return Err("a second INPUT grant was handed out");
    }
    // Overfill the ring: 300 key events, 256 fit, the rest are lost.
    for i in 0..300u16 {
        input::push(InputEvent {
            kind: KIND_KEY,
            code: i,
            value: 1,
            ..InputEvent::default()
        });
    }
    let process = super::spawn::spawn_test_from_kernel(
        &image,
        alloc::vec![
            (Object::Console, super::CONSOLE_RIGHTS),
            (Object::Input(grant), super::INPUT_RIGHTS)
        ],
    )
    .map_err(|_| "could not start the INPUT reader")?;
    wait_until(|| process.status() != Status::Running, 1_000_000);
    if process.status() != Status::Exited(5) {
        return Err("the INPUT reader did not exit(5)");
    }
    let word = |at: u64| process.peek_u64(DATA_BASE + at).unwrap_or(u64::MAX);
    // Event i of a read at `base`: kind is the u16 at offset 8, code at 10.
    let kind_code = |base: u64, i: u64| {
        let w = word(base + i * 24 + 8);
        ((w & 0xFFFF) as u16, ((w >> 16) & 0xFFFF) as u16)
    };
    if word(0) != 8 || word(8) != 8 {
        return Err("input_read did not return 8 events twice");
    }
    if kind_code(0x100, 0) != (KIND_KEY, 0)
        || kind_code(0x100, 7) != (KIND_KEY, 7)
        || kind_code(0x400, 0) != (KIND_KEY, 8)
    {
        return Err("INPUT events were not delivered in order");
    }
    if word(16) as i64 != crate::syscall::errno::E_PERM {
        return Err("the INPUT handle could be duplicated");
    }
    // The holder is dead: its handles were released, so the grant is free
    // again and the 240 unread events are gone. (Its handles are released
    // just after its status changes: wait for that, with a bound.)
    let mut again = None;
    wait_until(
        || {
            again = input::grant();
            again.is_some()
        },
        1_000_000,
    );
    let again = again.ok_or("the INPUT grant did not come back")?;
    input::push(InputEvent {
        kind: KIND_KEY,
        code: 999,
        ..InputEvent::default()
    });
    let mut probe = [InputEvent::default(); 4];
    let n = input::read(&mut probe, || false);
    if n != 1 || probe[0].code != 999 {
        return Err("events of the previous INPUT owner leaked to the next");
    }
    // Overflow: the 256 queued events come first, then the lost marker.
    for i in 0..300u16 {
        input::push(InputEvent {
            kind: KIND_KEY,
            code: i,
            ..InputEvent::default()
        });
    }
    let mut seen = 0usize;
    let mut chunk = [InputEvent::default(); 32];
    loop {
        let n = input::read(&mut chunk, || false);
        let lost_at = chunk[..n].iter().position(|e| e.kind == KIND_LOST);
        match lost_at {
            Some(i) => {
                seen += i;
                break;
            }
            None => seen += n,
        }
        if seen > input::CAPACITY {
            return Err("no lost marker after a full INPUT ring");
        }
    }
    if seen != input::CAPACITY {
        return Err("the lost marker did not come after the queued events");
    }
    // Keyboard routing: text reaches the console only while nobody reads
    // INPUT (anti-keylogger).
    let before = crate::console::input_pending();
    input::push_key(
        InputEvent {
            kind: KIND_KEY,
            code: 1,
            value: 1,
            ..InputEvent::default()
        },
        b"k",
    );
    let to_console = crate::console::input_pending() == before + 1;
    input::mark_active();
    input::push_key(
        InputEvent {
            kind: KIND_KEY,
            code: 2,
            value: 1,
            ..InputEvent::default()
        },
        b"x",
    );
    let kept_from_console = crate::console::input_pending() == before + 1;
    let mut drain = [0u8; 8];
    let _ = crate::console::input_read(&mut drain, || false);
    drop(again);
    input::flush();
    if !to_console || !kept_from_console || input::active() {
        return Err("keyboard text reached the console while INPUT was being read");
    }
    drop(process);
    Ok(())
}

// ---------------------------------------------------------------- 8a ---

/// `random_fill` from ring 3: two requests give different, non-zero
/// bytes; a kernel pointer is `E_FAULT`; length 0 is 0.
pub fn random() -> Result<(), &'static str> {
    let code = test_program(TestProgram::Random);
    let image = build_test_elf(
        &[(0x40_0000, RX, code, 0x1000), (0x40_1000, RW, &[], 0x1000)],
        0x40_0000,
    );
    let process = super::spawn::spawn_test_from_kernel(
        &image,
        alloc::vec![(Object::Console, super::CONSOLE_RIGHTS)],
    )
    .map_err(|_| "could not start the random_fill process")?;
    wait_until(|| process.status() != Status::Running, 1_000_000);
    if process.status() != Status::Exited(8) {
        return Err("the random_fill process did not exit(8)");
    }
    let word = |at: u64| process.peek_u64(DATA_BASE + at).unwrap_or(u64::MAX);
    if word(0) != 32 || word(8) != 32 {
        return Err("random_fill did not fill 32 bytes");
    }
    if word(16) as i64 != crate::syscall::errno::E_FAULT || word(24) != 0 {
        return Err("random_fill accepted a kernel pointer or mishandled length 0");
    }
    let a: [u64; 4] = [word(0x100), word(0x108), word(0x110), word(0x118)];
    let b: [u64; 4] = [word(0x200), word(0x208), word(0x210), word(0x218)];
    if a == b || a == [0; 4] || b == [0; 4] {
        return Err("random_fill returned repeated or zero bytes");
    }
    drop(process);
    Ok(())
}

// ---------------------------------------------------------------- 8b ---

/// A static PIE: code at 0 (RX), a data page at 0x1000 (RW) whose first
/// u64 is relocated (R_X86_64_RELATIVE) to `target`, and a read-only page
/// at 0x2000 with the dynamic section and the relocation table.
/// `tweak(&mut bytes)` lets tests break one field.
fn build_test_pie(code: &[u8], target: u64, tweak: impl FnOnce(&mut [u8])) -> alloc::vec::Vec<u8> {
    let mut out = alloc::vec![0u8; 0x4000];
    let mut put = |at: usize, v: &[u8]| {
        if let Some(d) = out.get_mut(at..at + v.len()) {
            d.copy_from_slice(v);
        }
    };
    put(0, b"\x7fELF");
    put(4, &[2, 1, 1]);
    put(16, &3u16.to_le_bytes()); // ET_DYN
    put(18, &62u16.to_le_bytes());
    put(20, &1u32.to_le_bytes());
    put(24, &0u64.to_le_bytes()); // entry = image start
    put(32, &64u64.to_le_bytes());
    put(52, &64u16.to_le_bytes());
    put(54, &56u16.to_le_bytes());
    put(56, &4u16.to_le_bytes());
    // (type, flags, offset, vaddr, filesz, memsz)
    let phdrs: [(u32, u32, u64, u64, u64, u64); 4] = [
        (1, RX, 0x1000, 0x0000, code.len() as u64, 0x1000),
        (1, RW, 0x2000, 0x1000, 8, 0x1000),
        (1, R_ONLY, 0x3000, 0x2000, 0x200, 0x1000),
        (2, R_ONLY, 0x3000, 0x2000, 64, 64), // PT_DYNAMIC
    ];
    for (i, (ty, fl, off, va, fsz, msz)) in phdrs.iter().enumerate() {
        let ph = 64 + i * 56;
        put(ph, &ty.to_le_bytes());
        put(ph + 4, &fl.to_le_bytes());
        put(ph + 8, &off.to_le_bytes());
        put(ph + 16, &va.to_le_bytes());
        put(ph + 32, &fsz.to_le_bytes());
        put(ph + 40, &msz.to_le_bytes());
        put(ph + 48, &4096u64.to_le_bytes());
    }
    put(0x1000, code);
    // Dynamic section: DT_RELA = 0x2100, DT_RELASZ = 24, DT_RELAENT = 24.
    let dynamic: [(u64, u64); 4] = [(7, 0x2100), (8, 24), (9, 24), (0, 0)];
    for (i, (tag, val)) in dynamic.iter().enumerate() {
        put(0x3000 + i * 16, &tag.to_le_bytes());
        put(0x3000 + i * 16 + 8, &val.to_le_bytes());
    }
    // One RELATIVE relocation: *(base + 0x1000) = base + target.
    put(0x3100, &0x1000u64.to_le_bytes());
    put(0x3108, &8u64.to_le_bytes());
    put(0x3110, &target.to_le_bytes());
    tweak(&mut out);
    out
}

const R_ONLY: u32 = 4;

/// ASLR: a static PIE runs at a random base with its relocation applied
/// (checked by the program itself in ring 3); two runs get different
/// bases and stacks; malformed or unsupported relocations are refused.
pub fn aslr() -> Result<(), &'static str> {
    use crate::arch::x86_64::usermode::pie_target_offset;
    let code = test_program(TestProgram::Pie);
    let target = pie_target_offset();
    let good = build_test_pie(code, target, |_| {});
    let image = Image::parse(&good).map_err(|_| "a valid static PIE was refused")?;
    if !image.is_pie() {
        return Err("an ET_DYN image was not treated as a PIE");
    }
    let run = || {
        super::spawn::spawn_test_from_kernel(
            &good,
            alloc::vec![(Object::Console, super::CONSOLE_RIGHTS)],
        )
        .map_err(|_| "could not start the PIE")
    };
    let a = run()?;
    let b = run()?;
    wait_until(
        || a.status() != Status::Running && b.status() != Status::Running,
        1_000_000,
    );
    if a.status() != Status::Exited(1) || b.status() != Status::Exited(1) {
        return Err("the PIE's relocation was not applied at its load base");
    }
    let (base_a, stack_a) = a.layout();
    let (base_b, stack_b) = b.layout();
    let in_window = |base: u64| {
        (super::elf::PIE_BASE_MIN..super::elf::PIE_BASE_MIN + super::elf::PIE_BASE_PAGES * 4096)
            .contains(&base)
            && base % 4096 == 0
    };
    if !in_window(base_a) || !in_window(base_b) {
        return Err("a PIE base outside the ASLR window");
    }
    // 2^24 bases and 2^16 stack slides: equal values mean ASLR is off.
    if base_a == base_b || stack_a == stack_b {
        return Err("two processes got the same base or stack (no ASLR)");
    }
    drop((a, b));

    let refused = |img: &[u8], why| match Image::parse(img) {
        Err(ElfError::BadRelocation) => Ok(()),
        _ => Err(why),
    };
    refused(
        &build_test_pie(code, target, |b| b[0x3108] = 1),
        "a non-RELATIVE relocation (R_X86_64_64) was accepted",
    )?;
    refused(
        &build_test_pie(code, target, |b| {
            b[0x3100..0x3108].copy_from_slice(&0x10u64.to_le_bytes())
        }),
        "a relocation into code (text relocation) was accepted",
    )?;
    refused(
        &build_test_pie(code, target, |b| b[0x3000] = 1),
        "a PIE needing a shared library (DT_NEEDED) was accepted",
    )?;
    refused(
        &build_test_pie(code, target, |b| b[0x3028] = 16),
        "a bad DT_RELAENT was accepted",
    )?;
    // PT_DYNAMIC in a fixed-address executable stays refused.
    let exec_dyn = build_test_pie(code, target, |b| b[16] = 2);
    // (Its segments at address 0 are refused too; either refusal will do.)
    if !matches!(
        Image::parse(&exec_dyn),
        Err(ElfError::Dynamic | ElfError::OutOfRange)
    ) {
        return Err("PT_DYNAMIC in an ET_EXEC image was accepted");
    }
    Ok(())
}
