//! Higher-Half Direct Map: every physical address `p` is also mapped at
//! virtual address `p + offset`. This is how the kernel reads and writes
//! physical memory (page tables, frames) without mapping it first.
//!
//! The offset is a kernel address: it must never be printed or exposed to
//! user space (with KASLR in Phase 8 it is randomised).

use core::sync::atomic::{AtomicU64, Ordering};

use super::addr::{PhysAddr, VirtAddr};

static OFFSET: AtomicU64 = AtomicU64::new(0);

/// Records the HHDM offset reported by the bootloader. First call wins.
pub fn init(offset: u64) {
    let _ = OFFSET.compare_exchange(0, offset, Ordering::AcqRel, Ordering::Acquire);
}

/// The HHDM offset (a kernel address: never print it).
pub fn offset() -> u64 {
    OFFSET.load(Ordering::Acquire)
}

/// Virtual address at which physical address `phys` is mapped.
///
/// Panics if called before [`init`] or if the result is not canonical,
/// both kernel bugs.
pub fn phys_to_virt(phys: PhysAddr) -> VirtAddr {
    let offset = OFFSET.load(Ordering::Acquire);
    assert!(offset != 0, "hhdm: used before init");
    match offset.checked_add(phys.as_u64()).and_then(VirtAddr::new) {
        Some(v) => v,
        None => panic!("hhdm: physical address outside the direct map"),
    }
}
