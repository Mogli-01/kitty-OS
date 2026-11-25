//! Global Descriptor Table and Task State Segment.
//!
//! In 64-bit mode segmentation is almost gone: bases and limits are ignored
//! for CS/DS/ES/SS. What still matters:
//! - **CS** selects the privilege level (ring 0 vs ring 3) and long mode (L bit).
//! - **The TSS** tells the CPU which stack to switch to on an interrupt:
//!   `RSP0` when coming from ring 3 (Phase 6) and the **IST** stacks, which
//!   are used for specific vectors *no matter what* the current stack is.
//!
//! IST is the reason this phase exists security-wise: if the kernel stack
//! overflows (or RSP is corrupted), a fault cannot push its frame and the
//! CPU raises a double fault. Without IST the double fault would try the same
//! broken stack and the machine triple-faults (resets) with no diagnostics.
//! With a dedicated, known-good IST stack we always get a report.
//!
//! Layout (order matters for `syscall`/`sysret` in Phase 6: SYSRET loads
//! SS = STAR[63:48] + 8 and CS = STAR[63:48] + 16, so user data must come
//! right before user code):
//!
//! | index | selector | descriptor            |
//! |-------|----------|-----------------------|
//! | 0     | 0x00     | null (required)       |
//! | 1     | 0x08     | kernel code, 64-bit   |
//! | 2     | 0x10     | kernel data           |
//! | 3     | 0x18 \|3 | user data (Phase 6)   |
//! | 4     | 0x20 \|3 | user code, 64-bit     |
//! | 5–6   | 0x28     | TSS (16-byte system descriptor) |

use core::arch::asm;
use core::cell::UnsafeCell;
use core::mem::size_of;
use core::sync::atomic::{AtomicBool, Ordering};

/// Kernel code segment selector.
pub const KERNEL_CS: u16 = 0x08;
/// Kernel data/stack segment selector.
pub const KERNEL_DS: u16 = 0x10;
/// User data/stack segment selector (RPL 3).
#[expect(dead_code, reason = "used by syscall/sysret setup in Phase 6")]
pub const USER_DS: u16 = 0x18 | 3;
/// User code segment selector (RPL 3).
#[expect(dead_code, reason = "used by syscall/sysret setup in Phase 6")]
pub const USER_CS: u16 = 0x20 | 3;
/// TSS selector.
pub const TSS_SEL: u16 = 0x28;

/// IST slot (1-based, as the IDT encodes it) used by each critical vector.
pub const IST_DOUBLE_FAULT: u8 = 1;
/// IST slot for NMI: an NMI can arrive at *any* instruction, including the
/// few where RSP is not valid yet (e.g. right after `syscall` in Phase 6).
pub const IST_NMI: u8 = 2;
/// IST slot for machine check, same reasoning as NMI.
pub const IST_MACHINE_CHECK: u8 = 3;

/// Size of each IST stack.
const IST_STACK_SIZE: usize = 16 * 1024;
const IST_COUNT: usize = 3;

// Descriptor encodings. Access byte: P (present), DPL, S (code/data), type.
// Flags nibble: G (4 KiB granularity), L (64-bit code) or DB (32-bit data).
const KERNEL_CODE: u64 = 0x00AF_9A00_0000_FFFF; // P, DPL0, exec/read, L=1
const KERNEL_DATA: u64 = 0x00CF_9200_0000_FFFF; // P, DPL0, read/write
const USER_DATA: u64 = 0x00CF_F200_0000_FFFF; // P, DPL3, read/write
const USER_CODE: u64 = 0x00AF_FA00_0000_FFFF; // P, DPL3, exec/read, L=1

/// 64-bit Task State Segment (Intel SDM Vol. 3, 8.7).
#[repr(C, packed(4))]
struct Tss {
    _reserved0: u32,
    /// Stack pointers loaded on a privilege change to ring 0/1/2.
    rsp: [u64; 3],
    _reserved1: u64,
    /// Interrupt Stack Table: IST1..IST7.
    ist: [u64; 7],
    _reserved2: u64,
    _reserved3: u16,
    /// Offset of the I/O permission bitmap. Pointing it at the end of the
    /// TSS means "no bitmap": with IOPL=0, ring 3 can never use `in`/`out`.
    iomap_base: u16,
}

const TSS_SIZE: usize = size_of::<Tss>();
const _: () = assert!(TSS_SIZE == 104);

#[repr(C, align(16))]
struct IstStacks([[u8; IST_STACK_SIZE]; IST_COUNT]);

/// Memory the CPU reads (and, for the TSS busy bit, *writes*) behind the
/// compiler's back. `UnsafeCell` tells Rust it may change, so it is never
/// treated as immutable data.
struct CpuTables {
    gdt: UnsafeCell<[u64; 7]>,
    tss: UnsafeCell<Tss>,
    ist_stacks: UnsafeCell<IstStacks>,
}

// SAFETY: the tables are written exactly once, by the single caller that
// wins the `INITIALIZED` flag in `init`, before the CPU is told about them.
// Afterwards Rust code never touches them again; only the CPU does.
unsafe impl Sync for CpuTables {}

static TABLES: CpuTables = CpuTables {
    gdt: UnsafeCell::new([0; 7]),
    tss: UnsafeCell::new(Tss {
        _reserved0: 0,
        rsp: [0; 3],
        _reserved1: 0,
        ist: [0; 7],
        _reserved2: 0,
        _reserved3: 0,
        iomap_base: TSS_SIZE as u16,
    }),
    ist_stacks: UnsafeCell::new(IstStacks([[0; IST_STACK_SIZE]; IST_COUNT])),
};

static INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Operand of `lgdt`/`lidt`: 16-bit limit followed by a 64-bit base.
#[repr(C, packed)]
pub(super) struct DescriptorTablePointer {
    pub limit: u16,
    pub base: u64,
}

/// Builds the 16-byte TSS system descriptor (type 0x9 = available 64-bit TSS).
fn tss_descriptor(base: u64) -> (u64, u64) {
    let limit = (TSS_SIZE - 1) as u64;
    let low = (limit & 0xFFFF)
        | ((base & 0xFF_FFFF) << 16)
        | (0x89 << 40) // P=1, DPL=0, type=0b1001
        | (((limit >> 16) & 0xF) << 48)
        | (((base >> 24) & 0xFF) << 56);
    let high = base >> 32;
    (low, high)
}

/// Installs the kernel GDT and TSS on the current CPU and reloads all
/// segment registers. Only the first call does anything.
///
/// Phase 9 (SMP) will need one GDT/TSS per CPU.
pub fn init() {
    if INITIALIZED.swap(true, Ordering::AcqRel) {
        return;
    }

    let stacks = TABLES.ist_stacks.get().cast::<[u8; IST_STACK_SIZE]>();
    let tss_ptr = TABLES.tss.get();
    let gdt_ptr = TABLES.gdt.get();

    let mut ist = [0u64; 7];
    for (slot, top) in ist.iter_mut().take(IST_COUNT).enumerate() {
        // Stacks grow down: the IST entry is the *end* of each array.
        // `wrapping_add` keeps this pointer arithmetic safe; the result stays
        // inside (one past the end of) the `IstStacks` allocation.
        *top = stacks.wrapping_add(slot + 1).addr() as u64;
    }

    // SAFETY: we won the INITIALIZED flag, so no other code accesses the TSS
    // and the CPU does not know about it yet (no `ltr` executed). The
    // pointer comes from a live static, so it is valid and aligned.
    unsafe {
        (*tss_ptr).ist = ist;
    }

    let (tss_low, tss_high) = tss_descriptor(tss_ptr.addr() as u64);
    let gdt = [
        0,
        KERNEL_CODE,
        KERNEL_DATA,
        USER_DATA,
        USER_CODE,
        tss_low,
        tss_high,
    ];
    // SAFETY: same exclusivity argument as for the TSS: the GDT is not
    // loaded yet and only this call can reach this line.
    unsafe {
        gdt_ptr.write(gdt);
    }

    let pointer = DescriptorTablePointer {
        limit: (size_of::<[u64; 7]>() - 1) as u16,
        base: gdt_ptr.addr() as u64,
    };

    // SAFETY: `pointer` describes a valid, 'static GDT whose kernel code and
    // data descriptors match what the kernel already runs with (ring 0,
    // 64-bit), so reloading CS/SS/DS cannot change privilege or mode.
    // The far return (`retfq`) is the only way to reload CS in long mode:
    // it pops RIP then CS. The TSS descriptor is valid and not busy.
    unsafe {
        asm!(
            "lgdt [{ptr}]",
            "push {cs}",
            "lea {tmp}, [rip + 2f]",
            "push {tmp}",
            "retfq",
            "2:",
            "mov ds, {ds:x}",
            "mov es, {ds:x}",
            "mov ss, {ds:x}",
            "mov fs, {ds:x}",
            "mov gs, {ds:x}",
            "ltr {tss:x}",
            ptr = in(reg) &pointer,
            cs = in(reg) u64::from(KERNEL_CS),
            ds = in(reg) u64::from(KERNEL_DS),
            tss = in(reg) u64::from(TSS_SEL),
            // `out`, not `lateout`: tmp is written while inputs are still live.
            tmp = out(reg) _,
            options(preserves_flags),
        );
    }
}
