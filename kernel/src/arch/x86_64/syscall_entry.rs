//! The `syscall` entry (Phase 6c, docs/FASE6.md §3, D-063).
//!
//! `syscall` jumps to LSTAR in ring 0 but does **not** switch stacks: RSP
//! is still the user's (any value the program wants). So the very first
//! instructions run with interrupts off (FMASK clears IF) and:
//! 1. `swapgs` — GS now points at the kernel's per-CPU block;
//! 2. save the user RSP in a per-CPU slot and load the current thread's
//!    kernel stack top from another (kept equal to `TSS.RSP0` by the
//!    scheduler);
//! 3. build exactly the [`TrapFrame`] a trap from ring 3 would have (SS,
//!    RSP, RFLAGS = R11, CS, RIP = RCX, error 0, "vector" 256), push the
//!    registers in the same order, and call [`syscall_dispatch`].
//!
//! The return goes through the **same exit as interrupts** (`swapgs` +
//! `iretq`), not `sysret`: `sysret` with a non-canonical RCX faults in
//! ring 0 on Intel CPUs (with the user GS loaded), and one well-tested exit
//! path is worth more than the cycles saved (D-063; measure first).
//!
//! FMASK clears IF, DF, TF, AC, NT and IOPL on entry: no interrupt before
//! the stack switch, the ABI's direction flag, no single-step trap in the
//! kernel, **SMAP closed** even if the program set AC with `popf` (the 6a
//! note), and no nested-task `iretq`.

use core::arch::global_asm;

use super::gdt::{KERNEL_CS, PERCPU_KERNEL_RSP, PERCPU_USER_RSP, USER_CS, USER_DS};
use super::trap::TrapFrame;

/// "Vector" recorded in the frame of a system call.
pub const SYSCALL_VECTOR: u64 = 256;

/// RFLAGS bits cleared on `syscall`: TF(8), IF(9), DF(10), IOPL(12-13),
/// NT(14), AC(18).
const FMASK: u64 = (1 << 8) | (1 << 9) | (1 << 10) | (3 << 12) | (1 << 14) | (1 << 18);

global_asm!(
    r#"
.section .text
.global kitty_syscall_entry
kitty_syscall_entry:
    swapgs
    mov qword ptr gs:[{user_rsp}], rsp
    mov rsp, qword ptr gs:[{kernel_rsp}]
    push {user_ds}
    push qword ptr gs:[{user_rsp}]
    push r11
    push {user_cs}
    push rcx
    push 0
    push {vector}
    cld
    push rax
    push rbx
    push rcx
    push rdx
    push rsi
    push rdi
    push rbp
    push r8
    push r9
    push r10
    push r11
    push r12
    push r13
    push r14
    push r15
    mov rdi, rsp
    call {dispatch}
    jmp kitty_trap_restore
"#,
    user_rsp = const PERCPU_USER_RSP,
    kernel_rsp = const PERCPU_KERNEL_RSP,
    user_ds = const USER_DS as u64,
    user_cs = const USER_CS as u64,
    vector = const SYSCALL_VECTOR,
    dispatch = sym syscall_dispatch,
);

unsafe extern "C" {
    fn kitty_syscall_entry();
}

/// Rust side of the entry. Runs on the thread's kernel stack with
/// interrupts off; turns them on for the call itself (so a blocking call
/// can sleep and the timer can preempt), and off again before returning.
extern "C" fn syscall_dispatch(frame: &mut TrapFrame) {
    super::cpu::enable_interrupts();
    let args = [
        frame.rdi, frame.rsi, frame.rdx, frame.r10, frame.r8, frame.r9,
    ];
    frame.rax = crate::syscall::dispatch(frame.rax, args) as u64;
    super::cpu::disable_interrupts();
    crate::proc::on_return_to_user();
}

/// Installs the entry. Call once, after the GDT (its layout is what STAR
/// encodes) and the per-CPU GS base are set up.
pub fn init() {
    // STAR[47:32]: `syscall` loads CS = KERNEL_CS, SS = KERNEL_CS + 8.
    // STAR[63:48]: `sysret` would load SS = base + 8, CS = base + 16; with
    // base = USER_DS - 8 that is USER_DS / USER_CS (unused today: we return
    // with `iretq`, but keep it right for a later `sysret` path).
    let star = (u64::from(USER_DS - 8) << 48) | (u64::from(KERNEL_CS) << 32);
    let entry = kitty_syscall_entry as *const () as usize as u64;
    // SAFETY: `entry` is our routine above; STAR matches gdt.rs (kernel
    // code 0x08, kernel data 0x10; user data 0x18|3, user code 0x20|3).
    unsafe { super::cpu::enable_syscall(entry, star, FMASK) };
}
