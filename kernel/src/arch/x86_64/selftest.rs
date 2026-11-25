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

/// Fires a real hardware interrupt: programs PIT channel 0 for a ~1 ms
/// one-shot and waits for IRQ0 to arrive through the 8259 PIC. Proves the
/// whole path: PIC remap/unmask, IDT gate, stub, dispatch, handler, EOI.
/// Must run with interrupts enabled. Returns whether IRQ0 arrived.
#[cfg(feature = "selftest-irq-pit")]
pub fn irq_pit() -> bool {
    use core::sync::atomic::{AtomicU32, Ordering};
    use kitty_hal::PortBlock;

    use super::irq::{self, Vector};
    use super::legacy::{self, LegacyDevice};

    static FIRED: AtomicU32 = AtomicU32::new(0);
    fn on_irq0(_: Vector) {
        FIRED.fetch_add(1, Ordering::Relaxed);
    }

    let Some(pit) = legacy::claim(LegacyDevice::Pit) else {
        return false;
    };
    if irq::register_isa(0, on_irq0).is_err() {
        return false;
    }
    // Mode/command: channel 0, low then high byte, mode 0 (interrupt on
    // terminal count), binary. Count 1193 at 1.193182 MHz ≈ 1 ms.
    let programmed = pit.write8(3, 0x30).is_some()
        && pit.write8(0, 0xA9).is_some()
        && pit.write8(0, 0x04).is_some();
    let mut spins = 0u64;
    while programmed && FIRED.load(Ordering::Relaxed) == 0 && spins < 500_000_000 {
        core::hint::spin_loop();
        spins += 1;
    }
    let _ = irq::unregister_isa(0);
    FIRED.load(Ordering::Relaxed) > 0
}
