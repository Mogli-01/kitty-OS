//! Deliberate crashes used by automated tests to check that exceptions
//! produce a report instead of a reset. Compiled only with the matching
//! `selftest-*` feature; never present in normal kernels.

#[cfg(feature = "selftest-double-fault")]
use core::arch::asm;

/// A canonical lower-half address that is never mapped while the kernel has
/// no user space (Phase 6 keeps low memory unmapped too).
#[cfg(feature = "selftest-page-fault")]
const UNMAPPED: usize = 0x0000_0000_dead_b000;

/// Reads an unmapped address: expect `Page Fault (#PF)` with
/// `cr2=0x00000000deadb000`, "non-present page on read in kernel mode".
#[cfg(feature = "selftest-page-fault")]
pub fn page_fault() {
    let ptr = core::ptr::without_provenance::<u64>(UNMAPPED);
    // SAFETY: deliberately invalid read to test the #PF handler, which never
    // returns from a kernel page fault (it panics). Volatile so the compiler
    // cannot remove or reorder it.
    let _ = unsafe { ptr.read_volatile() };
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
