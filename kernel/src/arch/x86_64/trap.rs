//! Low-level interrupt/exception entry: assembly stubs and the trap frame.
//!
//! Every vector gets a tiny stub that makes the stack look the same for all
//! of them, then jumps to one common routine:
//!
//! ```text
//!  higher addresses
//!  +-----------+
//!  | SS        |  \
//!  | RSP       |   |
//!  | RFLAGS    |   |  pushed by the CPU
//!  | CS        |   |
//!  | RIP       |  /
//!  | error code|  <- pushed by the CPU for some vectors, a dummy 0 otherwise
//!  | vector    |  <- pushed by the stub
//!  | RAX..R15  |  <- pushed by `trap_common` (15 registers)
//!  +-----------+  <- RSP when Rust is called = &TrapFrame
//!  lower addresses
//! ```
//!
//! Why our own stubs instead of `extern "x86-interrupt"`: we get *all*
//! general-purpose registers in one struct (for crash reports now, and for
//! the scheduler in Phase 5, which switches threads by switching frames),
//! the code is stable Rust, and it is only ~40 lines of assembly.
//!
//! Stack alignment: in 64-bit mode the CPU aligns RSP to 16 bytes before
//! pushing its 5-qword frame. We then push 2 + 15 = 17 more qwords:
//! 5 + 17 = 22 qwords = 176 bytes, a multiple of 16, so RSP is 16-aligned at
//! the `call`, exactly as the System V ABI requires.

use core::arch::global_asm;

/// Number of vectors that have a stub: all 256 (0–31 CPU exceptions,
/// 32–255 interrupts, Phase 4).
pub const STUB_COUNT: usize = 256;

/// Register state saved on every interrupt/exception, in stack order.
#[repr(C)]
#[derive(Debug)]
pub struct TrapFrame {
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub r11: u64,
    pub r10: u64,
    pub r9: u64,
    pub r8: u64,
    pub rbp: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rdx: u64,
    pub rcx: u64,
    pub rbx: u64,
    pub rax: u64,
    /// Interrupt vector number (pushed by our stub).
    pub vector: u64,
    /// CPU error code, or 0 for vectors that do not push one.
    pub error_code: u64,
    // Pushed by the CPU:
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

const _: () = assert!(core::mem::size_of::<TrapFrame>() == 22 * 8);

// The vectors for which the CPU pushes an error code: #DF(8), #TS(10),
// #NP(11), #SS(12), #GP(13), #PF(14), #AC(17), #CP(21), #VC(29), #SX(30).
// For every other vector the stub pushes a dummy 0 so the frame layout is
// identical for all vectors.
//
// `trap_common`:
// - `cld`: the ABI requires the direction flag clear on function entry; the
//   interrupted code may have set it.
// - Phase 6 TODO: `swapgs` when coming from ring 3 (CS & 3 != 0).
global_asm!(
    r#"
.macro TRAP_NOERR n
    .global kitty_trap_\n
kitty_trap_\n:
    push 0
    push \n
    jmp kitty_trap_common
.endm

.macro TRAP_ERR n
    .global kitty_trap_\n
kitty_trap_\n:
    push \n
    jmp kitty_trap_common
.endm

.section .text
.irp n, 0,1,2,3,4,5,6,7,9,15,16,18,19,20,22,23,24,25,26,27,28,31
    TRAP_NOERR \n
.endr
.irp n, 8,10,11,12,13,14,17,21,29,30
    TRAP_ERR \n
.endr
// Interrupt vectors 32-255: the CPU never pushes an error code for them.
.irp n, 32,33,34,35,36,37,38,39,40,41,42,43,44,45,46,47,48,49,50,51,52,53,54,55,56,57,58,59,60,61,62,63,64,65,66,67,68,69,70,71,72,73,74,75,76,77,78,79,80,81,82,83,84,85,86,87,88,89,90,91,92,93,94,95,96,97,98,99,100,101,102,103,104,105,106,107,108,109,110,111,112,113,114,115,116,117,118,119,120,121,122,123,124,125,126,127,128,129,130,131,132,133,134,135,136,137,138,139,140,141,142,143,144,145,146,147,148,149,150,151,152,153,154,155,156,157,158,159,160,161,162,163,164,165,166,167,168,169,170,171,172,173,174,175,176,177,178,179,180,181,182,183,184,185,186,187,188,189,190,191,192,193,194,195,196,197,198,199,200,201,202,203,204,205,206,207,208,209,210,211,212,213,214,215,216,217,218,219,220,221,222,223,224,225,226,227,228,229,230,231,232,233,234,235,236,237,238,239,240,241,242,243,244,245,246,247,248,249,250,251,252,253,254,255
    TRAP_NOERR \n
.endr

kitty_trap_common:
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
    pop r15
    pop r14
    pop r13
    pop r12
    pop r11
    pop r10
    pop r9
    pop r8
    pop rbp
    pop rdi
    pop rsi
    pop rdx
    pop rcx
    pop rbx
    pop rax
    add rsp, 16
    iretq

.section .rodata
.balign 8
.global kitty_trap_stubs
kitty_trap_stubs:
.irp n, 0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31,32,33,34,35,36,37,38,39,40,41,42,43,44,45,46,47,48,49,50,51,52,53,54,55,56,57,58,59,60,61,62,63,64,65,66,67,68,69,70,71,72,73,74,75,76,77,78,79,80,81,82,83,84,85,86,87,88,89,90,91,92,93,94,95,96,97,98,99,100,101,102,103,104,105,106,107,108,109,110,111,112,113,114,115,116,117,118,119,120,121,122,123,124,125,126,127,128,129,130,131,132,133,134,135,136,137,138,139,140,141,142,143,144,145,146,147,148,149,150,151,152,153,154,155,156,157,158,159,160,161,162,163,164,165,166,167,168,169,170,171,172,173,174,175,176,177,178,179,180,181,182,183,184,185,186,187,188,189,190,191,192,193,194,195,196,197,198,199,200,201,202,203,204,205,206,207,208,209,210,211,212,213,214,215,216,217,218,219,220,221,222,223,224,225,226,227,228,229,230,231,232,233,234,235,236,237,238,239,240,241,242,243,244,245,246,247,248,249,250,251,252,253,254,255
    .quad kitty_trap_\n
.endr
.section .text
"#,
    dispatch = sym trap_dispatch,
);

unsafe extern "C" {
    /// Addresses of the entry stubs, indexed by vector (defined above).
    #[link_name = "kitty_trap_stubs"]
    static TRAP_STUBS: [u64; STUB_COUNT];
}

/// Entry address of the stub for `vector`.
pub fn stub_address(vector: usize) -> Option<u64> {
    // SAFETY: `kitty_trap_stubs` is defined in the global_asm above as
    // exactly STUB_COUNT quadwords in .rodata; it is never written, so
    // reading it (by value, bounds-checked by `get`) is sound.
    let table = unsafe { &TRAP_STUBS };
    table.get(vector).copied()
}

/// Called by `kitty_trap_common` with RSP pointing at the saved frame.
///
/// `extern "C"` so the assembly can call it with the System V convention
/// (first argument in RDI).
extern "C" fn trap_dispatch(frame: &mut TrapFrame) {
    // The CPU does not clear RFLAGS.AC on interrupt entry. If we interrupted
    // a user copy (AC=1), close the SMAP window for the handler; `iretq`
    // restores the interrupted code's AC from the frame.
    super::usercopy::clac();
    match u8::try_from(frame.vector) {
        Ok(v) if v >= super::irq::FIRST_INTERRUPT_VECTOR => super::irq::dispatch(v),
        _ => super::exceptions::handle(frame),
    }
}
