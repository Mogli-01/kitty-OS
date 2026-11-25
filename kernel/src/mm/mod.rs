//! Memory management: physical frames (PMM), paging (VMM) and the kernel
//! heap.
//!
//! `unsafe` is allowed here (see `docs/UNSAFE.md`): page tables and raw
//! physical memory cannot be handled in safe Rust.

pub mod addr;
pub mod hhdm;
pub mod pmm;

pub use addr::{PhysAddr, VirtAddr};
pub use pmm::{FrameError, PhysFrame, PmmStats, alloc_frame};

use crate::boot_info::BootInfo;

/// Initialises memory management from the bootloader information.
pub fn init(boot: &BootInfo) {
    hhdm::init(boot.hhdm_offset);
    pmm::init(&boot.memory_map);
    pmm::selftest();
}
