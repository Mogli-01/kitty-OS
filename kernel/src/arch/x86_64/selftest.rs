//! Deliberate crashes used by automated tests to check that exceptions
//! produce a report instead of a reset. Compiled only with the matching
//! `selftest-*` feature; never present in normal kernels.
//!
//! Every faulting access is a single CPU instruction in `asm!`, without
//! `nomem`/`nostack`: the fault handler writes Rust memory (the console and
//! its lock) and pushes on the stack before it panics. Plain Rust accesses
//! (`read_volatile` on an unmapped address, calling a data pointer) would
//! promise the compiler things that are not true here (issue #7).

use core::arch::asm;

/// A canonical lower-half address that is never mapped while the kernel has
/// no user space (Phase 6 keeps low memory unmapped too).
#[cfg(feature = "selftest-page-fault")]
const UNMAPPED: usize = 0x0000_0000_dead_b000;

/// Reads an unmapped address: expect `Page Fault (#PF)` with
/// `cr2=0x00000000deadb000`, "non-present page on read in kernel mode".
#[cfg(feature = "selftest-page-fault")]
pub fn page_fault() {
    // SAFETY: deliberately reads an unmapped address to test the #PF
    // handler, which reports and panics: control never comes back. The block
    // may access memory and the stack (the handler does both).
    unsafe {
        asm!("mov {tmp}, qword ptr [{addr}]", addr = in(reg) UNMAPPED, tmp = out(reg) _);
    }
}

/// Makes the stack pointer non-canonical and triggers `int3`. Delivering #BP
/// needs to push on that stack, which faults; delivering *that* fault needs
/// the same broken stack again, so the CPU escalates to a double fault.
/// Only the IST stack lets the #DF handler run: expect `Double Fault (#DF)`
/// and "running on the IST double-fault stack".
#[cfg(feature = "selftest-double-fault")]
pub fn double_fault() {
    // SAFETY: deliberately destroys RSP. Control never returns here: the
    // double-fault handler (on IST1) reports and panics.
    unsafe {
        asm!(
            "mov rsp, {bad}",
            "int3",
            bad = in(reg) 0x8000_0000_0000_0000u64,
            options(noreturn),
        );
    }
}

/// Recurses until the stack runs into its guard page. The push into the
/// unmapped guard page faults; delivering that #PF needs the same stack, so
/// the CPU raises a double fault, handled on IST1. Without the guard page
/// the recursion would silently overwrite memory below the stack.
/// Expect `Double Fault (#DF)`.
#[cfg(feature = "selftest-stack-overflow")]
pub fn stack_overflow() {
    #[allow(unconditional_recursion)]
    fn recurse(depth: u64) -> u64 {
        // A big local buffer per frame so the overflow happens quickly;
        // `black_box` stops the compiler from turning this into a loop.
        let buf = core::hint::black_box([depth; 64]);
        recurse(depth + 1) + buf[(depth % 64) as usize]
    }
    core::hint::black_box(recurse(0));
}

/// Writes to the kernel's own code. With W^X + CR0.WP this must fault:
/// expect `Page Fault (#PF)` and "protection violation on write".
#[cfg(feature = "selftest-write-text")]
pub fn write_text() {
    let target = write_text as fn() as usize;
    // SAFETY: deliberately writes into .text to test W^X + CR0.WP; the #PF
    // handler reports and panics, control never comes back.
    unsafe {
        asm!("mov byte ptr [{addr}], 0xCC", addr = in(reg) target);
    }
}

/// Jumps into a data buffer containing `ret`. With NX this must fault on
/// the instruction fetch: expect "protection violation on instruction fetch".
#[cfg(feature = "selftest-exec-data")]
pub fn exec_data() {
    static mut CODE: [u8; 1] = [0xC3]; // `ret`
    let addr = (&raw mut CODE).addr();
    // SAFETY: deliberately jumps into a non-executable data page to test NX;
    // the instruction fetch faults and the #PF handler panics, so nothing
    // returns. `clobber_abi("C")` covers the call in case it ever did.
    unsafe {
        asm!("call {addr}", addr = in(reg) addr, clobber_abi("C"));
    }
}

/// Writes to the IDT, which is mapped read-only after boot: expect
/// "protection violation on write".
#[cfg(feature = "selftest-write-idt")]
pub fn write_idt() {
    let addr = super::idt::table_page();
    // SAFETY: deliberately writes to the read-only IDT page; the #PF handler
    // reports and panics, control never comes back.
    unsafe {
        asm!("mov qword ptr [{addr}], {zero}", addr = in(reg) addr, zero = in(reg) 0u64);
    }
}

/// Writes one byte past the end of a large heap allocation. The page after
/// it is an unmapped guard page: expect "non-present page on write".
#[cfg(feature = "selftest-heap-overflow")]
pub fn heap_overflow() {
    let mut buf = alloc::vec![0u8; 8192];
    let end = buf.as_mut_ptr().wrapping_add(buf.len()).addr();
    // SAFETY: deliberately writes one byte past the buffer, into the heap
    // guard page; the #PF handler reports and panics.
    unsafe {
        asm!("mov byte ptr [{addr}], 1", addr = in(reg) end);
    }
    core::hint::black_box(&buf);
}

/// Fires real hardware interrupts: programs PIT channel 0 as a ~1 kHz rate
/// generator, then requires **two** IRQ0s through the 8259 PIC after a
/// baseline taken once the PIT is programmed and the line registered.
///
/// Why two (review of PR #15): the PIC is edge-triggered and will not
/// deliver IRQ0 again while its in-service bit is set, so the second
/// interrupt can only arrive if the core sent EOI for the first. The
/// baseline discards any interrupt latched before our programming (e.g.
/// from the firmware's PIT setup). Must run with interrupts enabled.
/// Returns whether both interrupts arrived.
#[cfg(feature = "selftest-irq-pit")]
pub fn irq_pit() -> bool {
    use core::sync::atomic::{AtomicU32, Ordering};
    use kitty_hal::PortBlock;

    use super::irq::{self, Vector};
    use super::legacy::{self, LegacyDevice};

    static FIRED: AtomicU32 = AtomicU32::new(0);
    fn on_irq0(_: Vector) {
        FIRED.fetch_add(1, Ordering::Release);
    }

    let Some(pit) = legacy::claim(LegacyDevice::Pit) else {
        return false;
    };
    // 1. Program the PIT while IRQ0 is still masked. Mode/command: channel
    //    0, low then high byte, mode 2 (rate generator), binary.
    //    Count 1193 at 1.193182 MHz ≈ 1 ms period.
    let programmed = pit.write8(3, 0x34).is_some()
        && pit.write8(0, 0xA9).is_some()
        && pit.write8(0, 0x04).is_some();
    if !programmed {
        return false;
    }
    // 2. Register (unmasks the line), then take the baseline: anything
    //    counted up to here may have been latched before we programmed it.
    if irq::register_isa(0, on_irq0).is_err() {
        return false;
    }
    let baseline = FIRED.load(Ordering::Acquire);
    // 3. Two more interrupts: the second proves EOI released the PIC.
    let mut spins = 0u64;
    while FIRED.load(Ordering::Acquire) < baseline + 2 && spins < 500_000_000 {
        core::hint::spin_loop();
        spins += 1;
    }
    let delivered = FIRED.load(Ordering::Acquire) >= baseline + 2;
    // 4. Mask the line again and stop the timer: writing only the mode-0
    //    control word sets OUT low and the counter does not start until a
    //    count is written, so no further edge (and no stale IRQ0 latched in
    //    the PIC's IRR for the next owner of line 0) is produced.
    let _ = irq::unregister_isa(0);
    let _ = pit.write8(3, 0x30);
    delivered
}

/// Preemption self-test: two threads spin without ever yielding while the
/// boot thread also spins; a PIT-driven `sched::tick` must make all three
/// progress. Returns whether both workers ran and finished.
#[cfg(feature = "selftest-preempt")]
pub fn preempt() -> bool {
    use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use kitty_hal::PortBlock;

    use super::irq::{self, Vector};
    use super::legacy::{self, LegacyDevice};

    static A: AtomicU64 = AtomicU64::new(0);
    static B: AtomicU64 = AtomicU64::new(0);
    static STOP: AtomicBool = AtomicBool::new(false);
    static DONE: AtomicU64 = AtomicU64::new(0);
    fn busy(counter: &AtomicU64) {
        while !STOP.load(Ordering::Acquire) {
            counter.fetch_add(1, Ordering::Relaxed); // never yields
        }
        DONE.fetch_add(1, Ordering::Release);
    }
    fn worker_a() {
        busy(&A);
    }
    fn worker_b() {
        busy(&B);
    }
    fn on_tick(_: Vector) {
        crate::sched::tick();
    }

    let Some(pit) = legacy::claim(LegacyDevice::Pit) else {
        return false;
    };
    // ~1 kHz rate generator, programmed while IRQ0 is masked.
    let programmed = pit.write8(3, 0x34).is_some()
        && pit.write8(0, 0xA9).is_some()
        && pit.write8(0, 0x04).is_some();
    if !programmed || irq::register_isa(0, on_tick).is_err() {
        return false;
    }
    if crate::sched::spawn("busy-a", worker_a).is_err()
        || crate::sched::spawn("busy-b", worker_b).is_err()
    {
        return false;
    }
    // The boot thread spins too, without yielding: only preemption can let
    // the workers run.
    let mut spins = 0u64;
    while (A.load(Ordering::Relaxed) == 0 || B.load(Ordering::Relaxed) == 0)
        && spins < 2_000_000_000
    {
        core::hint::spin_loop();
        spins += 1;
    }
    let both_ran = A.load(Ordering::Relaxed) > 0 && B.load(Ordering::Relaxed) > 0;
    STOP.store(true, Ordering::Release);
    while DONE.load(Ordering::Acquire) < 2 {
        crate::sched::yield_now();
    }
    let _ = irq::unregister_isa(0);
    let _ = pit.write8(3, 0x30); // stop the PIT (mode 0, no count loaded)
    crate::sched::yield_now(); // reap the dead workers
    both_ran
}

/// Runs a kernel thread that recurses into its stack guard page. Expect a
/// double fault on the IST stack (the #PF cannot be delivered on the broken
/// thread stack).
#[cfg(feature = "selftest-thread-stack-overflow")]
pub fn thread_stack_overflow() {
    fn overflow() {
        #[allow(unconditional_recursion)]
        fn recurse(depth: u64) -> u64 {
            let buf = core::hint::black_box([depth; 64]);
            recurse(depth + 1) + buf[(depth % 64) as usize]
        }
        core::hint::black_box(recurse(0));
    }
    if crate::sched::spawn("overflow", overflow).is_ok() {
        loop {
            crate::sched::yield_now();
        }
    }
}
