//! Kernel thread context switch (Phase 5, docs/DECISIONS.md D-045).
//!
//! A thread that is not running is fully described by **one number**: its
//! saved stack pointer. Everything else it needs is on its own stack:
//!
//! ```text
//!   saved rsp → [ r15 ][ r14 ][ r13 ][ r12 ][ rbx ][ rbp ][ return address ]
//! ```
//!
//! `kitty_switch` pushes the six registers the System V ABI says a callee
//! must preserve, stores RSP, loads the next thread's RSP, pops its six
//! registers and returns *into the next thread*. That is the whole switch:
//! 14 instructions, no memory other than the two stacks.
//!
//! Why so little is enough:
//! - caller-saved registers (rax, rcx, rdx, rsi, rdi, r8–r11) are already
//!   saved by the compiler around the call, because `switch` is a normal
//!   function call for it;
//! - the kernel never uses SSE/AVX (soft-float target), so there is no FPU
//!   state to save (user-mode FPU state comes in Phase 6);
//! - RFLAGS: switches only happen with interrupts disabled, and the caller
//!   restores the interrupt flag it had afterwards;
//! - segment registers and CR3 are the same for all kernel threads.

use core::arch::global_asm;

global_asm!(
    r#"
.section .text
.global kitty_switch
// rdi = where to save the current RSP, rsi = RSP to resume.
kitty_switch:
    push rbp
    push rbx
    push r12
    push r13
    push r14
    push r15
    mov [rdi], rsp
    mov rsp, rsi
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbx
    pop rbp
    ret

// First code a new thread runs: `kitty_switch` "returns" here with the
// thread's entry point in r12 (placed by `initial_frame`). RSP is 16-byte
// aligned at this point, so the `call` below enters Rust with the
// alignment the ABI requires.
.global kitty_thread_trampoline
kitty_thread_trampoline:
    mov rdi, r12
    call {start}
    ud2
"#,
    start = sym crate::sched::thread_start,
);

unsafe extern "C" {
    fn kitty_switch(save_rsp: *mut u64, resume_rsp: u64);
    fn kitty_thread_trampoline();
}

/// Number of words `initial_frame` places at the top of a new stack.
pub const INITIAL_FRAME_WORDS: usize = 7;

/// The words to place at the very top of a new thread's stack so that the
/// first `switch` to it starts `entry` (an address passed to
/// `sched::thread_start`). Returns the words in memory order (lowest
/// address first); the thread's initial saved RSP is
/// `top - 8 * INITIAL_FRAME_WORDS`.
pub fn initial_frame(entry: u64) -> [u64; INITIAL_FRAME_WORDS] {
    let trampoline = kitty_thread_trampoline as *const () as usize as u64;
    // r15, r14, r13, r12 (= entry), rbx, rbp, return address. The return
    // address sits at `top - 8`, so after `ret` RSP == top (16-aligned).
    [0, 0, 0, entry, 0, 0, trampoline]
}

/// Saves the current thread's state, writing its RSP to `*save_rsp`, and
/// resumes the thread whose saved RSP is `resume_rsp`. Returns when some
/// later switch resumes the current thread.
///
/// # Safety
/// - Interrupts must be disabled (the switch is not atomic otherwise).
/// - `save_rsp` must be valid for a write of a `u64` and stay valid until
///   the write happens (it does, before the stack changes).
/// - `resume_rsp` must be the RSP of a thread suspended by `switch` (or a
///   fresh stack prepared with [`initial_frame`]) whose stack is still
///   mapped, and that thread must not be running anywhere else.
pub unsafe fn switch(save_rsp: *mut u64, resume_rsp: u64) {
    // SAFETY: forwarded from the caller's contract. `kitty_switch` follows
    // the C ABI: it preserves every callee-saved register of *this* thread
    // across the call (they are on its stack while it is suspended).
    unsafe { kitty_switch(save_rsp, resume_rsp) }
}
