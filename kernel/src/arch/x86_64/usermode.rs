//! Entering ring 3 and per-thread user FPU/SSE state (Phase 6b, D-060).
//!
//! **First entry.** A user thread starts as a kernel thread whose entry
//! function calls [`enter_user`]. It builds the frame `iretq` expects
//! (SS, RSP, RFLAGS, CS, RIP), **zeroes every general-purpose register**
//! (so no kernel value leaks into user space), switches GS to the user
//! base with `swapgs`, and executes `iretq`. Interrupts are disabled from
//! the moment the frame is built until `iretq` atomically sets IF again:
//! no interrupt can arrive with the GS bases half swapped.
//!
//! RFLAGS for user code: only IF (interrupts on, so the timer can preempt
//! it) and the always-one bit 1. IOPL = 0 (no `in`/`out`/`cli`), no TF, no
//! NT, no AC.
//!
//! **Later entries** happen through `iretq` at the end of `trap_common`:
//! user code is interrupted (timer, fault) and resumed from its saved
//! trap frame.
//!
//! **FPU/SSE.** The kernel never uses them (soft-float target), so the
//! registers always hold *some user thread's* state. The scheduler saves it
//! with `fxsave64` when a user thread stops running and restores the next
//! user thread's with `fxrstor64`; a new thread starts from the
//! architectural initial state. Without this, one process could read
//! another's XMM registers (passwords, keys) after a context switch.

use core::arch::{asm, global_asm};

use super::gdt::{USER_CS, USER_DS};

/// RFLAGS for user code: IF (bit 9) and the reserved bit 1.
const USER_RFLAGS: u64 = 0x202;

global_asm!(
    r#"
.section .text
.global kitty_enter_user
// rdi = user RIP, rsi = user RSP. Never returns.
kitty_enter_user:
    cli
    push {user_ds}
    push rsi
    push {rflags}
    push {user_cs}
    push rdi
    xor eax, eax
    xor ebx, ebx
    xor ecx, ecx
    xor edx, edx
    xor esi, esi
    xor edi, edi
    xor ebp, ebp
    xor r8d, r8d
    xor r9d, r9d
    xor r10d, r10d
    xor r11d, r11d
    xor r12d, r12d
    xor r13d, r13d
    xor r14d, r14d
    xor r15d, r15d
    swapgs
    iretq
"#,
    user_ds = const USER_DS as u64,
    user_cs = const USER_CS as u64,
    rflags = const USER_RFLAGS,
);

unsafe extern "C" {
    fn kitty_enter_user(rip: u64, rsp: u64) -> !;
}

/// Leaves the kernel and starts running user code at `rip` with stack
/// `rsp`, in the address space currently loaded in CR3.
///
/// # Safety
/// - CR3 must be the address space of the current thread's process, and
///   `TSS.RSP0` the top of the current thread's kernel stack (the scheduler
///   sets both before running a user thread).
/// - `rip` and `rsp` must be user-half addresses (the CPU would happily
///   `iretq` to kernel addresses with CPL 3; they would just fault, but the
///   caller must not rely on that).
/// - Nothing on the current kernel stack may be needed again: the next
///   trap from ring 3 starts again at the top of the stack.
pub unsafe fn enter_user(rip: u64, rsp: u64) -> ! {
    // SAFETY: forwarded from the caller's contract.
    unsafe { kitty_enter_user(rip, rsp) }
}

/// Makes DS/ES/FS/GS null before a user thread runs, so a selector that
/// another process loaded (say `mov fs, 0x1b`) cannot be seen by this one.
/// The 64-bit user ABI does not use them; FS/GS bases stay 0 for user
/// space. Cheap in the common case (all already null). If GS has to be
/// reloaded, the kernel GS base is restored right after (loading a GS
/// selector may clear the base the kernel is using).
pub fn reset_user_selectors() {
    let (ds, es, fs, gs): (u16, u16, u16, u16);
    // SAFETY: reading segment selectors has no side effects.
    unsafe {
        asm!(
            "mov {0:x}, ds",
            "mov {1:x}, es",
            "mov {2:x}, fs",
            "mov {3:x}, gs",
            out(reg) ds, out(reg) es, out(reg) fs, out(reg) gs,
            options(nomem, nostack, preserves_flags),
        );
    }
    if ds | es | fs != 0 {
        // SAFETY: loading the null selector into DS/ES/FS is always valid in
        // 64-bit mode (data segments are not used for addressing), and the
        // kernel never relies on their values or on the FS base.
        unsafe {
            asm!(
                "mov ds, {0:x}",
                "mov es, {0:x}",
                "mov fs, {0:x}",
                in(reg) 0u16,
                options(nomem, nostack, preserves_flags),
            );
        }
    }
    if gs != 0 {
        // SAFETY: as above for GS; no GS-relative access happens between
        // this and the base being restored on the next line.
        unsafe { asm!("mov gs, {0:x}", in(reg) 0u16, options(nomem, nostack, preserves_flags)) };
        // SAFETY: the per-CPU block is a 'static; this restores exactly the
        // bases `gdt::init` installed (kernel block, user base 0).
        unsafe { super::cpu::set_gs_bases(super::gdt::percpu_address()) };
    }
}

/// Saved x87/MMX/SSE state of one user thread (`fxsave64` format).
#[repr(C, align(16))]
pub struct FpuState([u8; 512]);

impl FpuState {
    /// The architectural initial state (what `fninit` + default MXCSR
    /// give): FCW = 0x037F (all x87 exceptions masked, 64-bit precision),
    /// MXCSR = 0x1F80 (all SSE exceptions masked, round to nearest), every
    /// register zero and every x87 slot empty.
    pub fn initial() -> Self {
        let mut bytes = [0u8; 512];
        bytes[0..2].copy_from_slice(&0x037Fu16.to_le_bytes());
        bytes[24..28].copy_from_slice(&0x1F80u32.to_le_bytes());
        Self(bytes)
    }
}

/// Saves the live FPU/SSE registers into `state`.
pub fn fpu_save(state: &mut FpuState) {
    // SAFETY: `state` is a valid, 16-byte aligned, exclusively borrowed
    // 512-byte buffer, exactly what `fxsave64` writes. CR4.OSFXSR is set
    // (`cpu::enable_user_sse`) before any user thread exists.
    unsafe {
        asm!("fxsave64 [{}]", in(reg) state.0.as_mut_ptr(), options(nostack, preserves_flags))
    };
}

/// Loads the FPU/SSE registers from `state`.
pub fn fpu_restore(state: &FpuState) {
    // SAFETY: `state` is a valid, aligned 512-byte image produced by
    // `fxsave64` or `FpuState::initial` (whose reserved MXCSR bits are 0,
    // so `fxrstor64` cannot #GP); it only changes FPU/SSE registers, which
    // the kernel never uses.
    unsafe {
        asm!("fxrstor64 [{}]", in(reg) state.0.as_ptr(), options(nostack, preserves_flags, readonly))
    };
}

// ---------------------------------------------------------------------------
// Tiny user programs for the Phase 6b self-tests (`proc::selftest`). They
// are only *data* for the kernel: copied into a fresh user page and run in
// ring 3. Each is position-independent; the data page sits right after the
// code page (`DATA` below), the stack is wherever the loader put it.

/// User address of the test programs' data page (code page + 4 KiB).
pub const TEST_DATA: u64 = 0x0040_1000;

global_asm!(
    r#"
.section .rodata.kitty_user_tests, "a"
.balign 16

// Reads address 0: #PF (the first 1 MiB is never mapped).
.global kitty_ut_null_start
kitty_ut_null_start:
    xor eax, eax
    mov rax, qword ptr [rax]
    ud2
.global kitty_ut_null_end
kitty_ut_null_end:

// Reads the first byte of the kernel half: #PF (present but supervisor).
.global kitty_ut_kread_start
kitty_ut_kread_start:
    movabs rax, 0xFFFF800000000000
    mov rax, qword ptr [rax]
    ud2
.global kitty_ut_kread_end
kitty_ut_kread_end:

// `sgdt` would leak the GDT address: with UMIP it is #GP. (Without UMIP
// it succeeds and `ud2` gives #UD instead.)
.global kitty_ut_sgdt_start
kitty_ut_sgdt_start:
    sub rsp, 16
    sgdt [rsp]
    ud2
.global kitty_ut_sgdt_end
kitty_ut_sgdt_end:

// `cli` in ring 3 with IOPL 0: #GP.
.global kitty_ut_cli_start
kitty_ut_cli_start:
    cli
    ud2
.global kitty_ut_cli_end
kitty_ut_cli_end:

// Port I/O in ring 3 (keyboard port): #GP (IOPL 0, no I/O bitmap).
.global kitty_ut_io_start
kitty_ut_io_start:
    in al, 0x60
    ud2
.global kitty_ut_io_end
kitty_ut_io_end:

// `int3` goes through an IDT gate with DPL 0: #GP, not #BP (D-026).
.global kitty_ut_int3_start
kitty_ut_int3_start:
    int3
    ud2
.global kitty_ut_int3_end
kitty_ut_int3_end:

// ORs every general-purpose register, xmm0-xmm2 and the DS/ES/FS/GS
// selectors together and stores the
// result at DATA (must be 0: nothing leaked from the kernel or from
// another process), then counts forever at DATA + 8.
.global kitty_ut_clean_start
kitty_ut_clean_start:
    or rax, rbx
    or rax, rcx
    or rax, rdx
    or rax, rsi
    or rax, rdi
    or rax, rbp
    or rax, r8
    or rax, r9
    or rax, r10
    or rax, r11
    or rax, r12
    or rax, r13
    or rax, r14
    or rax, r15
    movq rcx, xmm0
    or rax, rcx
    movq rcx, xmm1
    or rax, rcx
    movq rcx, xmm2
    or rax, rcx
    mov ecx, ds
    or rax, rcx
    mov ecx, es
    or rax, rcx
    mov ecx, fs
    or rax, rcx
    mov ecx, gs
    or rax, rcx
    mov rdx, 0x401000
    mov qword ptr [rdx], rax
3:
    inc qword ptr [rdx + 8]
    jmp 3b
.global kitty_ut_clean_end
kitty_ut_clean_end:

// Fills xmm0-xmm2 with a secret pattern, loads the user data selector into
// DS/ES/FS/GS, and counts forever at DATA + 8.
// Used to check that the pattern never reaches another process.
.global kitty_ut_secret_start
kitty_ut_secret_start:
    movabs rax, 0x5EC12E7C0FFEE000
    movq xmm0, rax
    movq xmm1, rax
    movq xmm2, rax
    mov eax, 0x1b
    mov ds, eax
    mov es, eax
    mov fs, eax
    mov gs, eax
    mov rdx, 0x401000
4:
    inc qword ptr [rdx + 8]
    jmp 4b
.global kitty_ut_secret_end
kitty_ut_secret_end:

// Phase 6c: exercises every system call and its error paths. Results go
// to DATA + 8*i (see `proc::selftest`); then exit(42).
.global kitty_ut_syscalls_start
kitty_ut_syscalls_start:
    mov r12, 0x401000
    // [0] console_write(0, msg, len) = len
    mov eax, 5
    xor edi, edi
    lea rsi, [rip + 90f]
    mov edx, 37 // length of the message at 90:
    syscall
    mov [r12 + 0], rax
    // [1] console_write(0, kernel address, 8) = E_FAULT
    mov eax, 5
    xor edi, edi
    movabs rsi, 0xFFFF800000000000
    mov edx, 8
    syscall
    mov [r12 + 8], rax
    // [2] console_write(0, msg, 5000) = E_TOOBIG
    mov eax, 5
    xor edi, edi
    lea rsi, [rip + 90f]
    mov edx, 5000
    syscall
    mov [r12 + 16], rax
    // [3] console_write(7, msg, 1) = E_BADHANDLE
    mov eax, 5
    mov edi, 7
    lea rsi, [rip + 90f]
    mov edx, 1
    syscall
    mov [r12 + 24], rax
    // [4] handle_dup(0, READ) = h2
    mov eax, 3
    xor edi, edi
    mov esi, 1
    syscall
    mov [r12 + 32], rax
    mov r13, rax
    // [5] console_write(h2, msg, 1) = E_PERM (no WRITE)
    mov eax, 5
    mov rdi, r13
    lea rsi, [rip + 90f]
    mov edx, 1
    syscall
    mov [r12 + 40], rax
    // [6] handle_info(h2) = (1 << 32) | READ
    mov eax, 4
    mov rdi, r13
    syscall
    mov [r12 + 48], rax
    // [7] handle_close(h2) = 0
    mov eax, 2
    mov rdi, r13
    syscall
    mov [r12 + 56], rax
    // [8] handle_close(h2) again = E_BADHANDLE
    mov eax, 2
    mov rdi, r13
    syscall
    mov [r12 + 64], rax
    // [9] handle_dup(0, undefined right bit 6) = E_INVAL
    mov eax, 3
    xor edi, edi
    mov esi, 0x40
    syscall
    mov [r12 + 72], rax
    // [10] handle_dup(0, MAP) = E_PERM (rights would grow)
    mov eax, 3
    xor edi, edi
    mov esi, 4
    syscall
    mov [r12 + 80], rax
    // [11] unknown call 999 = E_NOSYS
    mov eax, 999
    syscall
    mov [r12 + 88], rax
    // [12] console_read(0, DATA + 0x800, 64) = bytes
    mov eax, 6
    xor edi, edi
    lea rsi, [r12 + 0x800]
    mov edx, 64
    syscall
    mov [r12 + 96], rax
    // [13] TSC cycles for 1000 null system calls
    rdtsc
    shl rdx, 32
    or rax, rdx
    mov r14, rax
    mov r15d, 1000
5:
    mov eax, 999
    syscall
    dec r15d
    jnz 5b
    rdtsc
    shl rdx, 32
    or rax, rdx
    sub rax, r14
    mov [r12 + 104], rax
    // [14] registers survive a system call (r12-r14 callee data, rbx)
    mov rbx, 0x1234
    mov eax, 1
    syscall
    mov [r12 + 112], rbx
    // exit(42)
    mov eax, 0
    mov edi, 42
    syscall
    ud2
90:
    .ascii "[user] hello from ring 3 via syscall\n"
91:
.global kitty_ut_syscalls_end
kitty_ut_syscalls_end:

// Phase 6d: a whole ELF image's code. Writes a line, checks that its stack
// (64 KiB, mapped by the loader) and its .bss are usable, then exit(7).
.global kitty_ut_elf_start
kitty_ut_elf_start:
    mov eax, 5
    xor edi, edi
    lea rsi, [rip + 92f]
    mov edx, 24 // length of the message at 92:
    syscall
    // touch near the bottom of the 64 KiB stack (the initial RSP may sit
    // up to 4 KiB below the top: ASLR, Phase 8b)
    mov qword ptr [rsp - 0xF000], 1
    // .bss (second segment, zero-filled)
    mov rdx, 0x402000
    mov rcx, qword ptr [rdx]
    mov edi, 7
    add rdi, rcx
    mov eax, 0
    syscall
    ud2
92:
    .ascii "[user] ELF image loaded\n"
.global kitty_ut_elf_end
kitty_ut_elf_end:

// Phase 6e: reads input events through handle 1 (INPUT), twice, tries to
// duplicate it, then exit(5). Results at DATA (+0 n1, +8 n2, +16 dup);
// events at DATA + 0x100 and DATA + 0x400.
.global kitty_ut_input_start
kitty_ut_input_start:
    mov r12, 0x401000
    mov eax, 13
    mov edi, 1
    lea rsi, [r12 + 0x100]
    mov edx, 8
    syscall
    mov [r12 + 0], rax
    mov eax, 13
    mov edi, 1
    lea rsi, [r12 + 0x400]
    mov edx, 8
    syscall
    mov [r12 + 8], rax
    mov eax, 3
    mov edi, 1
    mov esi, 1
    syscall
    mov [r12 + 16], rax
    mov eax, 0
    mov edi, 5
    syscall
    ud2
.global kitty_ut_input_end
kitty_ut_input_end:

// Phase 8a: random_fill twice (32 bytes each), once with a kernel pointer,
// once with length 0. Results at DATA (+0, +8, +16, +24); bytes at
// DATA + 0x100 and DATA + 0x200. Then exit(8).
.global kitty_ut_random_start
kitty_ut_random_start:
    mov r12, 0x401000
    mov eax, 8
    lea rdi, [r12 + 0x100]
    mov esi, 32
    syscall
    mov [r12 + 0], rax
    mov eax, 8
    lea rdi, [r12 + 0x200]
    mov esi, 32
    syscall
    mov [r12 + 8], rax
    mov eax, 8
    movabs rdi, 0xFFFF800000000000
    mov esi, 32
    syscall
    mov [r12 + 16], rax
    mov eax, 8
    lea rdi, [r12 + 0x300]
    xor esi, esi
    syscall
    mov [r12 + 24], rax
    mov eax, 0
    mov edi, 8
    syscall
    ud2
.global kitty_ut_random_end
kitty_ut_random_end:

// Phase 8b: a static-PIE image. Its data page (image base + 0x1000) holds
// a pointer the loader relocated to `kitty_ut_pie_target`; exit(1) if it
// matches the address computed rip-relatively, else exit(2).
.global kitty_ut_pie_start
kitty_ut_pie_start:
    lea rdx, [rip + kitty_ut_pie_start]
    lea rax, [rip + kitty_ut_pie_target]
    mov rcx, qword ptr [rdx + 0x1000]
    cmp rax, rcx
    jne 7f
    mov eax, 0
    mov edi, 1
    syscall
7:
    mov eax, 0
    mov edi, 2
    syscall
.global kitty_ut_pie_target
kitty_ut_pie_target:
    ud2
.global kitty_ut_pie_end
kitty_ut_pie_end:

// Blocks forever in console_read (to test killing a blocked process).
.global kitty_ut_reader_start
kitty_ut_reader_start:
6:
    mov eax, 6
    xor edi, edi
    mov rsi, 0x401000
    mov edx, 16
    syscall
    jmp 6b
.global kitty_ut_reader_end
kitty_ut_reader_end:
.section .text
"#
);

unsafe extern "C" {
    static kitty_ut_null_start: u8;
    static kitty_ut_null_end: u8;
    static kitty_ut_kread_start: u8;
    static kitty_ut_kread_end: u8;
    static kitty_ut_sgdt_start: u8;
    static kitty_ut_sgdt_end: u8;
    static kitty_ut_cli_start: u8;
    static kitty_ut_cli_end: u8;
    static kitty_ut_io_start: u8;
    static kitty_ut_io_end: u8;
    static kitty_ut_int3_start: u8;
    static kitty_ut_int3_end: u8;
    static kitty_ut_clean_start: u8;
    static kitty_ut_clean_end: u8;
    static kitty_ut_secret_start: u8;
    static kitty_ut_secret_end: u8;
    static kitty_ut_syscalls_start: u8;
    static kitty_ut_syscalls_end: u8;
    static kitty_ut_reader_start: u8;
    static kitty_ut_reader_end: u8;
    static kitty_ut_elf_start: u8;
    static kitty_ut_elf_end: u8;
    static kitty_ut_input_start: u8;
    static kitty_ut_input_end: u8;
    static kitty_ut_random_start: u8;
    static kitty_ut_random_end: u8;
    static kitty_ut_pie_start: u8;
    static kitty_ut_pie_target: u8;
    static kitty_ut_pie_end: u8;
}

/// The user test programs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TestProgram {
    /// Null pointer read.
    NullRead,
    /// Read of a kernel-half address.
    KernelRead,
    /// `sgdt` (UMIP).
    Sgdt,
    /// `cli`.
    Cli,
    /// `in al, 0x60`.
    PortIo,
    /// `int3` through a DPL-0 gate.
    Int3,
    /// Register-cleanliness check, then counts.
    Clean,
    /// Fills SSE registers with a secret, then counts.
    Secret,
    /// Every system call and its errors (6c), then `exit(42)`.
    Syscalls,
    /// Blocks in `console_read` forever.
    Reader,
    /// Code of the ELF loader test image (6d): writes, uses its stack and
    /// .bss, `exit(7)`.
    ElfMain,
    /// Reads INPUT events twice through handle 1, tries `dup`, `exit(5)`.
    InputReader,
    /// Calls `random_fill` (good, bad pointer, empty), `exit(8)`.
    Random,
    /// Static-PIE code: checks a relocated pointer, `exit(1)` if right.
    Pie,
}

/// Offset of `kitty_ut_pie_target` inside the PIE test program.
pub fn pie_target_offset() -> u64 {
    let start = (&raw const kitty_ut_pie_start).addr();
    let target = (&raw const kitty_ut_pie_target).addr();
    target.saturating_sub(start) as u64
}

/// Machine code of `program`.
pub fn test_program(program: TestProgram) -> &'static [u8] {
    let (start, end) = match program {
        TestProgram::NullRead => (&raw const kitty_ut_null_start, &raw const kitty_ut_null_end),
        TestProgram::KernelRead => (
            &raw const kitty_ut_kread_start,
            &raw const kitty_ut_kread_end,
        ),
        TestProgram::Sgdt => (&raw const kitty_ut_sgdt_start, &raw const kitty_ut_sgdt_end),
        TestProgram::Cli => (&raw const kitty_ut_cli_start, &raw const kitty_ut_cli_end),
        TestProgram::PortIo => (&raw const kitty_ut_io_start, &raw const kitty_ut_io_end),
        TestProgram::Int3 => (&raw const kitty_ut_int3_start, &raw const kitty_ut_int3_end),
        TestProgram::Clean => (
            &raw const kitty_ut_clean_start,
            &raw const kitty_ut_clean_end,
        ),
        TestProgram::Syscalls => (
            &raw const kitty_ut_syscalls_start,
            &raw const kitty_ut_syscalls_end,
        ),
        TestProgram::Reader => (
            &raw const kitty_ut_reader_start,
            &raw const kitty_ut_reader_end,
        ),
        TestProgram::ElfMain => (&raw const kitty_ut_elf_start, &raw const kitty_ut_elf_end),
        TestProgram::InputReader => (
            &raw const kitty_ut_input_start,
            &raw const kitty_ut_input_end,
        ),
        TestProgram::Random => (
            &raw const kitty_ut_random_start,
            &raw const kitty_ut_random_end,
        ),
        TestProgram::Pie => (&raw const kitty_ut_pie_start, &raw const kitty_ut_pie_end),
        TestProgram::Secret => (
            &raw const kitty_ut_secret_start,
            &raw const kitty_ut_secret_end,
        ),
    };
    let len = end.addr().saturating_sub(start.addr());
    // SAFETY: both symbols delimit one program in the read-only section
    // above (start <= end, same section, emitted together), so the range is
    // valid, immutable, initialised bytes for the whole kernel lifetime.
    unsafe { core::slice::from_raw_parts(start, len) }
}
