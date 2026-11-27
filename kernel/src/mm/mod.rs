//! Memory management: physical frames (PMM), paging (VMM) and the kernel
//! heap.
//!
//! `unsafe` is allowed here (see `docs/UNSAFE.md`): page tables and raw
//! physical memory cannot be handled in safe Rust.

pub mod addr;
pub mod heap;
pub mod hhdm;
pub mod paging;
pub mod pmm;
pub mod stack;
pub mod vmm;

pub use addr::{PhysAddr, VirtAddr};
pub use paging::{CacheType, MapError, MapFlags};
pub use pmm::{FrameError, PhysFrame, PmmStats, alloc_frame};

use crate::boot_info::BootInfo;

/// Initialises memory management: frame allocator, then the kernel's own
/// page tables (switching CR3). Returns what the VMM did, for the boot log.
pub fn init(boot: &BootInfo) -> vmm::VmmReport {
    hhdm::init(boot.hhdm_offset);
    pmm::init(&boot.memory_map);
    pmm::selftest();
    vmm::init(boot)
}
