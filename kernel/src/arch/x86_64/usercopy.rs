//! Raw copies between kernel memory and user memory, with **#PF recovery**
//! (Phase 6a, docs/FASE6.md §4, D-050).
//!
//! The copy is a single `rep movsb` whose address is known to the #PF
//! handler (an "exception table" with one entry). If a user address is not
//! mapped or not accessible, the CPU faults *on that instruction*; instead of
//! a kernel panic, the handler moves RIP to a fixup label that returns an
//! error. A bad pointer from a program therefore becomes `E_FAULT`, never a
//! crash.
//!
//! SMAP: user pages are inaccessible to the kernel unless RFLAGS.AC is set.
//! `stac` (set AC) is executed right before the copy and `clac` right after,
//! including on the fault path, so the window where the kernel may touch
//! user memory is exactly this one instruction. (The CPU does not clear AC
//! when an interrupt arrives, so `trap_dispatch` clears it on entry: a
//! handler that interrupts a copy never runs with SMAP open.)
//!
//! Only `mm::user` (the safe `UserSlice`/`UserPtr` API) calls this.

use core::arch::{asm, global_asm};
use core::sync::atomic::{AtomicBool, Ordering};

global_asm!(
    r#"
.section .text
.global kitty_copy_user
// rdi = destination, rsi = source, rdx = byte count. Returns rax = 0 on
// success, 1 if a page fault hit the copy. Exactly one of the two pointers
// is a user address (validated by the caller), the other a kernel buffer.
kitty_copy_user:
    cld
    mov rcx, rdx
.global kitty_copy_user_insn
kitty_copy_user_insn:
    rep movsb
    xor eax, eax
    ret
.global kitty_copy_user_fixup
kitty_copy_user_fixup:
    mov eax, 1
    ret
"#
);

unsafe extern "C" {
    fn kitty_copy_user(dst: *mut u8, src: *const u8, len: usize) -> u64;
    static kitty_copy_user_insn: u8;
    static kitty_copy_user_fixup: u8;
}

/// Whether SMAP is enabled (then `stac`/`clac` exist and are used).
static SMAP_ON: AtomicBool = AtomicBool::new(false);

/// Records that CR4.SMAP was enabled (called once by `cpu::enable_smap`).
pub(super) fn note_smap_enabled() {
    SMAP_ON.store(true, Ordering::Release);
}

/// Whether SMAP is active.
pub fn smap_enabled() -> bool {
    SMAP_ON.load(Ordering::Acquire)
}

/// Clears RFLAGS.AC (closes the SMAP window). No-op without SMAP (`clac`
/// would be an invalid instruction there).
#[inline]
pub fn clac() {
    if smap_enabled() {
        // SAFETY: `clac` only clears RFLAGS.AC, which re-enables SMAP
        // protection; it is valid because SMAP is supported and enabled.
        // Not `nomem`: memory accesses must not move across the boundary.
        unsafe { asm!("clac", options(nostack)) };
    }
}

#[inline]
fn stac() {
    if smap_enabled() {
        // SAFETY: `stac` sets RFLAGS.AC so the following instruction may
        // access user pages. It is only used around `kitty_copy_user`, whose
        // caller validated the user range, and `clac` always follows.
        unsafe { asm!("stac", options(nostack)) };
    }
}

/// If a page fault at `rip` happened on the user-copy instruction, returns
/// the address to resume at (the fixup); otherwise `None` (a real bug).
pub(super) fn fixup_for(rip: u64) -> Option<u64> {
    let insn = (&raw const kitty_copy_user_insn).addr() as u64;
    let fixup = (&raw const kitty_copy_user_fixup).addr() as u64;
    (rip == insn).then_some(fixup)
}

/// Copies `len` bytes from `src` to `dst`, where exactly one side is user
/// memory. Returns `false` if the user side faulted (nothing is guaranteed
/// about how many bytes were copied then).
///
/// # Safety
/// - The kernel side must be a valid, writable (for `dst`) or readable (for
///   `src`) kernel buffer of `len` bytes that does not overlap the user side.
/// - The user side must lie entirely in the lower (user) half of the address
///   space: the #PF recovery only applies to the copy instruction, and the
///   kernel must never be tricked into copying to/from kernel addresses
///   through this function. `mm::user::UserSlice` guarantees this.
pub unsafe fn copy(dst: *mut u8, src: *const u8, len: usize) -> bool {
    stac();
    // SAFETY: forwarded from the caller: the kernel buffer is valid; the
    // user range is in the user half, and any fault on it is caught by the
    // #PF fixup (`fixup_for`), returning 1 instead of crashing.
    let faulted = unsafe { kitty_copy_user(dst, src, len) };
    clac();
    faulted == 0
}
