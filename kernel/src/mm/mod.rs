//! Memory management: physical frames (PMM), paging (VMM) and the kernel
//! heap. Phase 1 only defines the address types; the rest arrives in Phase 3.
//!
//! `unsafe` is allowed here (see `docs/UNSAFE.md`): page tables and raw
//! physical memory cannot be handled in safe Rust.

pub mod addr;

pub use addr::{PhysAddr, VirtAddr};
