//! Interrupt Descriptor Table.
//!
//! 256 gates of 16 bytes. Each gate says: which code segment and address to
//! jump to, which IST stack to use (0 = none), the gate type, and the DPL —
//! the lowest privilege allowed to trigger the vector with `int n`.
//!
//! Security choices:
//! - Every gate has **DPL 0**: user code (Phase 6) cannot fire any vector
//!   with `int n`; trying raises #GP. Only `int3` may be opened later for
//!   debuggers, as an explicit decision.
//! - All gates are **interrupt gates** (type 0xE), not trap gates: the CPU
//!   clears IF on entry, so a handler is never interrupted before it has
//!   saved state. We re-enable interrupts explicitly where it is safe.
//! - Unused vectors are **not present**: a stray interrupt on them raises
//!   #NP instead of jumping to garbage.

use core::arch::asm;
use core::cell::UnsafeCell;
use core::mem::size_of;
use core::sync::atomic::{AtomicBool, Ordering};

use super::gdt::{self, DescriptorTablePointer, KERNEL_CS};
use super::trap::{self, STUB_COUNT};

/// Number of entries in the IDT.
const IDT_ENTRIES: usize = 256;

/// One 16-byte IDT gate (Intel SDM Vol. 3, 6.14.1).
#[derive(Clone, Copy)]
#[repr(C)]
struct Gate {
    offset_low: u16,
    selector: u16,
    /// Bits 0..2: IST index (1..7), 0 = use the current/RSP0 stack.
    ist: u8,
    /// P | DPL | 0 | type.
    type_attr: u8,
    offset_mid: u16,
    offset_high: u32,
    _reserved: u32,
}

const _: () = assert!(size_of::<Gate>() == 16);

/// Present, DPL 0, 64-bit interrupt gate.
const INTERRUPT_GATE: u8 = 0x8E;

impl Gate {
    const MISSING: Self = Self {
        offset_low: 0,
        selector: 0,
        ist: 0,
        type_attr: 0,
        offset_mid: 0,
        offset_high: 0,
        _reserved: 0,
    };

    fn interrupt(handler: u64, ist: u8) -> Self {
        Self {
            offset_low: handler as u16,
            selector: KERNEL_CS,
            ist: ist & 0b111,
            type_attr: INTERRUPT_GATE,
            offset_mid: (handler >> 16) as u16,
            offset_high: (handler >> 32) as u32,
            _reserved: 0,
        }
    }
}

struct Idt(UnsafeCell<[Gate; IDT_ENTRIES]>);

// SAFETY: written once by the caller that wins `INITIALIZED`, before `lidt`;
// afterwards only the CPU reads it.
unsafe impl Sync for Idt {}

static IDT: Idt = Idt(UnsafeCell::new([Gate::MISSING; IDT_ENTRIES]));
static INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Vectors that must always run on a known-good stack.
fn ist_for(vector: usize) -> u8 {
    match vector {
        2 => gdt::IST_NMI,
        8 => gdt::IST_DOUBLE_FAULT,
        18 => gdt::IST_MACHINE_CHECK,
        _ => 0,
    }
}

/// Fills the IDT with the exception stubs and loads it. Only the first call
/// does anything. Must run after [`gdt::init`] (gates reference KERNEL_CS
/// and the IST slots of our TSS).
pub fn init() {
    if INITIALIZED.swap(true, Ordering::AcqRel) {
        return;
    }

    let mut table = [Gate::MISSING; IDT_ENTRIES];
    for (vector, gate) in table.iter_mut().enumerate().take(STUB_COUNT) {
        if let Some(handler) = trap::stub_address(vector) {
            *gate = Gate::interrupt(handler, ist_for(vector));
        }
    }

    let idt_ptr = IDT.0.get();
    // SAFETY: we won `INITIALIZED`, so nothing else writes the IDT, and the
    // CPU is not using it yet. The pointer is to a live, aligned static.
    unsafe {
        idt_ptr.write(table);
    }

    let pointer = DescriptorTablePointer {
        limit: (size_of::<[Gate; IDT_ENTRIES]>() - 1) as u16,
        base: idt_ptr.addr() as u64,
    };
    // SAFETY: `pointer` describes a fully initialised 'static IDT whose
    // present gates all point at valid stubs in kernel code with the kernel
    // code selector. `lidt` only reads the 10-byte operand.
    unsafe {
        asm!("lidt [{}]", in(reg) &pointer, options(readonly, nostack, preserves_flags));
    }
}
