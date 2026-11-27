//! CPU exception handling policy and crash reports.
//!
//! Policy:
//! - Any exception raised by the **kernel** is a kernel bug, so it is fatal
//!   — except `#BP` (breakpoint, used as a self-test), NMI (reported and
//!   resumed) and a #PF on the user-copy instruction (becomes `E_FAULT`).
//! - An exception raised by **user code** (saved CS has RPL 3) kills the
//!   offending process, never the kernel (Phase 6b, D-060). Exceptions that
//!   are not the program's doing — NMI, machine check, double fault — keep
//!   the kernel policy.
//!
//! Security: the full report (registers, addresses) goes **only to the
//! serial debug console**. The screen gets the exception name without any
//! address, because once user space exists the screen is visible to it and
//! kernel addresses would defeat KASLR (Phase 8).

use super::cpu;
use super::trap::TrapFrame;
use crate::console;

/// Human-readable names of the 32 architectural exception vectors.
const NAMES: [&str; 32] = [
    "Divide Error (#DE)",
    "Debug (#DB)",
    "Non-Maskable Interrupt (NMI)",
    "Breakpoint (#BP)",
    "Overflow (#OF)",
    "Bound Range Exceeded (#BR)",
    "Invalid Opcode (#UD)",
    "Device Not Available (#NM)",
    "Double Fault (#DF)",
    "Coprocessor Segment Overrun",
    "Invalid TSS (#TS)",
    "Segment Not Present (#NP)",
    "Stack-Segment Fault (#SS)",
    "General Protection Fault (#GP)",
    "Page Fault (#PF)",
    "Reserved (15)",
    "x87 Floating-Point Error (#MF)",
    "Alignment Check (#AC)",
    "Machine Check (#MC)",
    "SIMD Floating-Point (#XM)",
    "Virtualization (#VE)",
    "Control Protection (#CP)",
    "Reserved (22)",
    "Reserved (23)",
    "Reserved (24)",
    "Reserved (25)",
    "Reserved (26)",
    "Reserved (27)",
    "Hypervisor Injection (#HV)",
    "VMM Communication (#VC)",
    "Security (#SX)",
    "Reserved (31)",
];

const VEC_NMI: u64 = 2;
const VEC_BREAKPOINT: u64 = 3;
const VEC_DOUBLE_FAULT: u64 = 8;
const VEC_PAGE_FAULT: u64 = 14;

/// Name of exception `vector`.
pub fn name(vector: u64) -> &'static str {
    usize::try_from(vector)
        .ok()
        .and_then(|v| NAMES.get(v))
        .copied()
        .unwrap_or("Unknown vector")
}

const VEC_MACHINE_CHECK: u64 = 18;

/// Number of user faults seen with the kernel GS base correctly restored
/// by `swapgs` (and with a wrong one): evidence for the self-test.
static SWAPGS_OK: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
static SWAPGS_BAD: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// `(good, bad)` GS-base checks made on user faults so far.
pub fn swapgs_checks() -> (u64, u64) {
    use core::sync::atomic::Ordering::Relaxed;
    (SWAPGS_OK.load(Relaxed), SWAPGS_BAD.load(Relaxed))
}

/// Entry point from the trap stub.
pub(super) fn handle(frame: &mut TrapFrame) {
    let from_user = frame.cs & 3 != 0;
    if from_user && !matches!(frame.vector, VEC_NMI | VEC_DOUBLE_FAULT | VEC_MACHINE_CHECK) {
        user_fault(frame);
    }
    // A page fault on the user-copy instruction is an expected outcome (a
    // bad user pointer), not a kernel bug: resume at the copy's fixup,
    // which returns an error to the caller. Only faults raised in ring 0
    // at exactly that instruction qualify.
    // The faulting address must be on the *user* side (the kernel side is
    // always a valid kernel buffer: a fault there is memory corruption), and
    // a reserved-bit fault means corrupted page tables: both stay fatal
    // instead of being turned into a quiet E_FAULT (security review).
    const PF_RESERVED_BIT: u64 = 1 << 3;
    const USER_END: u64 = 0x0000_8000_0000_0000;
    if frame.vector == VEC_PAGE_FAULT
        && frame.cs & 3 == 0
        && frame.error_code & PF_RESERVED_BIT == 0
        && cpu::read_cr2() < USER_END
        && let Some(fixup) = super::usercopy::fixup_for(frame.rip)
    {
        frame.rip = fixup;
        return;
    }
    match frame.vector {
        VEC_BREAKPOINT => {
            // `int3` is a trap: the saved RIP already points after it, so
            // returning resumes execution.
            console::debug_print(format_args!(
                "[cpu] breakpoint at rip={:#018x}\n",
                frame.rip
            ));
        }
        VEC_NMI => {
            console::debug_print(format_args!("[cpu] NMI received, resuming\n"));
        }
        _ => fatal(frame),
    }
}

/// A user program faulted: report it (serial only — even user addresses
/// stay off the screen, D-023) and kill its process.
fn user_fault(frame: &TrapFrame) -> ! {
    use core::sync::atomic::Ordering::Relaxed;
    if super::cpu::gs_base() == super::gdt::percpu_address() {
        SWAPGS_OK.fetch_add(1, Relaxed);
    } else {
        SWAPGS_BAD.fetch_add(1, Relaxed);
    }
    console::debug_print(format_args!(
        "[proc] user fault: {} at rip={:#x} rsp={:#x} error={:#x}{}
",
        name(frame.vector),
        frame.rip,
        frame.rsp,
        frame.error_code,
        if frame.vector == VEC_PAGE_FAULT {
            " (page fault)"
        } else {
            ""
        },
    ));
    if frame.vector == VEC_PAGE_FAULT {
        console::debug_print(format_args!(
            "    cr2={:#x}
",
            cpu::read_cr2()
        ));
    }
    crate::proc::exit_current_on_fault(frame.vector as u8)
}

fn fatal(frame: &TrapFrame) -> ! {
    let name = name(frame.vector);
    // Screen + serial, no addresses.
    // `emergency_print` never blocks on the console lock (the fault may have
    // happened while it was held).
    console::emergency_print(format_args!(
        "\n*** CPU EXCEPTION: {} - full report on the serial port\n",
        name
    ));
    report(frame);
    if frame.vector == VEC_DOUBLE_FAULT {
        // A double fault is an abort: the saved state may be meaningless and
        // returning is architecturally impossible.
        console::debug_print(format_args!(
            "    (running on the IST double-fault stack)\n"
        ));
    }
    panic!("fatal CPU exception: {}", name);
}

/// Full register dump on the serial debug console.
fn report(f: &TrapFrame) {
    let p = console::debug_print;
    p(format_args!(
        "=== {} (vector {}) error_code={:#x}\n",
        name(f.vector),
        f.vector,
        f.error_code
    ));
    match f.vector {
        VEC_PAGE_FAULT => report_page_fault(f.error_code),
        10..=13 if f.error_code != 0 => report_selector_error(f.error_code),
        _ => {}
    }
    let ring = f.cs & 3;
    p(format_args!(
        "    rip={:#018x} cs={:#06x} (ring {}) rflags={:#018x}\n",
        f.rip, f.cs, ring, f.rflags
    ));
    p(format_args!("    rsp={:#018x} ss={:#06x}\n", f.rsp, f.ss));
    p(format_args!(
        "    rax={:#018x} rbx={:#018x} rcx={:#018x} rdx={:#018x}\n",
        f.rax, f.rbx, f.rcx, f.rdx
    ));
    p(format_args!(
        "    rsi={:#018x} rdi={:#018x} rbp={:#018x}\n",
        f.rsi, f.rdi, f.rbp
    ));
    p(format_args!(
        "    r8 ={:#018x} r9 ={:#018x} r10={:#018x} r11={:#018x}\n",
        f.r8, f.r9, f.r10, f.r11
    ));
    p(format_args!(
        "    r12={:#018x} r13={:#018x} r14={:#018x} r15={:#018x}\n",
        f.r12, f.r13, f.r14, f.r15
    ));
}

/// Decodes the #PF error code. CR2 holds the faulting linear address.
fn report_page_fault(code: u64) {
    let bit = |n: u32| code & (1 << n) != 0;
    console::debug_print(format_args!(
        "    cr2={:#018x}: {} {} in {} mode{}{}{}\n",
        cpu::read_cr2(),
        if bit(0) {
            "protection violation on"
        } else {
            "non-present page on"
        },
        if bit(4) {
            "instruction fetch"
        } else if bit(1) {
            "write"
        } else {
            "read"
        },
        if bit(2) { "user" } else { "kernel" },
        if bit(3) {
            ", reserved bit set in a page table"
        } else {
            ""
        },
        if bit(5) { ", protection key" } else { "" },
        if bit(6) { ", shadow stack" } else { "" },
    ));
}

/// Decodes a selector error code (#TS, #NP, #SS, #GP).
fn report_selector_error(code: u64) {
    let table = if code & 0b10 != 0 {
        "IDT"
    } else if code & 0b100 != 0 {
        "LDT"
    } else {
        "GDT"
    };
    console::debug_print(format_args!(
        "    selector: {} index {}{}\n",
        table,
        (code >> 3) & 0x1FFF,
        if code & 1 != 0 {
            " (external event)"
        } else {
            ""
        }
    ));
}
