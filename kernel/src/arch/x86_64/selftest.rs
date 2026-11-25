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
    let ptr = core::ptr::with_exposed_provenance_mut::<u8>(target);
    // SAFETY: deliberately invalid write into .text to test W^X; the #PF
    // handler panics, nothing returns here.
    unsafe { ptr.write_volatile(0xCC) };
}

/// Jumps into a data buffer containing `ret`. With NX this must fault on
/// the instruction fetch: expect "protection violation on instruction fetch".
#[cfg(feature = "selftest-exec-data")]
pub fn exec_data() {
    static mut CODE: [u8; 1] = [0xC3]; // `ret`
    let addr = (&raw mut CODE).addr();
    // SAFETY: deliberately calls into a non-executable page to test NX; the
    // #PF handler panics, so the call never "returns".
    let f: extern "C" fn() = unsafe { core::mem::transmute(addr) };
    f();
}

/// Writes to the IDT, which is mapped read-only after boot: expect
/// "protection violation on write".
#[cfg(feature = "selftest-write-idt")]
pub fn write_idt() {
    let ptr = core::ptr::with_exposed_provenance_mut::<u64>(super::idt::table_page() as usize);
    // SAFETY: deliberately invalid write to test that the IDT page is
    // read-only; the #PF handler panics.
    unsafe { ptr.write_volatile(0) };
}

/// Writes one byte past the end of a large heap allocation. The page after
/// it is an unmapped guard page: expect "non-present page on write".
#[cfg(feature = "selftest-heap-overflow")]
pub fn heap_overflow() {
    let mut buf = alloc::vec![0u8; 8192];
    let end = buf.as_mut_ptr().wrapping_add(buf.len());
    // SAFETY: deliberately out of bounds to test the heap guard page; the
    // #PF handler panics, nothing returns here.
    unsafe { end.write_volatile(1) };
    core::hint::black_box(&buf);
}
