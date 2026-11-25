//! Statically allocated kernel stacks, each with a **guard page** below it.
//!
//! Stacks grow downwards. If one overflows, the next push lands in the guard
//! page, which is left *unmapped* in the kernel page tables (see
//! `mm::vmm`). The access faults immediately instead of silently
//! overwriting whatever happens to sit below the stack — a classic way to
//! turn a deep recursion bug into memory corruption.
//!
//! ```text
//!   [ guard 4 KiB | main stack 64 KiB ]   boot CPU kernel stack
//!   [ guard 4 KiB | IST1 16 KiB ]          double fault
//!   [ guard 4 KiB | IST2 16 KiB ]          NMI
//!   [ guard 4 KiB | IST3 16 KiB ]          machine check
//! ```
//!
//! The guard pages are real (zeroed, in .bss) memory that we simply do not
//! map: 16 KiB of physical RAM is a small price for this protection.
//! Thread stacks (Phase 5) will be allocated dynamically with the same layout.

use core::arch::asm;
use core::cell::UnsafeCell;

use crate::mm::addr::PAGE_SIZE;

const GUARD: usize = PAGE_SIZE as usize;
const MAIN_STACK_SIZE: usize = 64 * 1024;
const IST_STACK_SIZE: usize = 16 * 1024;
/// Number of IST stacks (double fault, NMI, machine check).
pub const IST_COUNT: usize = 3;

#[repr(C, align(4096))]
struct Guarded<const N: usize> {
    guard: [u8; GUARD],
    stack: [u8; N],
}

impl<const N: usize> Guarded<N> {
    const fn new() -> Self {
        Self {
            guard: [0; GUARD],
            stack: [0; N],
        }
    }
}

#[repr(C)]
struct KernelStacks {
    main: Guarded<MAIN_STACK_SIZE>,
    ist: [Guarded<IST_STACK_SIZE>; IST_COUNT],
}

struct StacksCell(UnsafeCell<KernelStacks>);

// SAFETY: Rust code never reads or writes the stack memory through this
// static; it only computes addresses. The memory is used by the CPU as
// stacks (each one by a single execution context at a time).
unsafe impl Sync for StacksCell {}

static STACKS: StacksCell = StacksCell(UnsafeCell::new(KernelStacks {
    main: Guarded::new(),
    ist: [Guarded::new(), Guarded::new(), Guarded::new()],
}));

fn base() -> *mut KernelStacks {
    STACKS.0.get()
}

/// Address one past the end of `stack` (the initial stack pointer).
fn top_of<const N: usize>(g: *mut Guarded<N>) -> u64 {
    // `wrapping_add(1)` = one past the end of the `Guarded` object, which is
    // exactly the end of its `stack` array (the struct has no tail padding:
    // its size, GUARD + N, is a multiple of 4096).
    g.wrapping_add(1).addr() as u64
}

/// Initial stack pointer of IST stack `index` (0-based).
pub fn ist_top(index: usize) -> Option<u64> {
    if index >= IST_COUNT {
        return None;
    }
    let ist = base()
        .cast::<u8>()
        .wrapping_add(core::mem::offset_of!(KernelStacks, ist));
    let slot = ist.cast::<Guarded<IST_STACK_SIZE>>().wrapping_add(index);
    Some(top_of(slot))
}

/// Initial stack pointer of the boot CPU's main kernel stack.
pub fn main_top() -> u64 {
    top_of(base().cast::<Guarded<MAIN_STACK_SIZE>>())
}

/// Virtual addresses of the guard pages, which must stay unmapped.
pub fn guard_pages() -> [u64; 1 + IST_COUNT] {
    let main = base().addr() as u64;
    let mut pages = [main; 1 + IST_COUNT];
    for (i, page) in pages.iter_mut().skip(1).enumerate() {
        // The guard is the first page of each `Guarded` block, i.e. 4 KiB
        // below the bottom of its stack.
        *page = ist_top(i).map_or(0, |top| top - (IST_STACK_SIZE + GUARD) as u64);
    }
    pages
}

/// Switches to the main kernel stack and calls `entry`, never returning.
///
/// The bootloader's stack (in bootloader-reclaimable memory, no guard page)
/// is abandoned; nothing may reference it afterwards, which `entry` taking
/// no arguments guarantees.
pub fn run_on_main_stack(entry: extern "C" fn() -> !) -> ! {
    let top = main_top();
    // SAFETY: `top` is the 16-byte aligned end of a 64 KiB stack reserved
    // for exactly this purpose and mapped read/write by the kernel page
    // tables. We never return, so no frame of the old stack is used again.
    // RBP is cleared so stack walkers stop here. `call` pushes a return
    // address, leaving RSP ≡ 8 (mod 16) at entry as the ABI requires.
    unsafe {
        asm!(
            "mov rsp, {top}",
            "xor ebp, ebp",
            "call {entry}",
            "ud2",
            top = in(reg) top,
            entry = in(reg) entry,
            options(noreturn),
        );
    }
}
