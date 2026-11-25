//! Physical and virtual address newtypes.
//!
//! Using distinct types (instead of bare `u64`) makes it a compile error to
//! pass a physical address where a virtual one is expected — a classic
//! source of kernel memory-corruption bugs.

use core::fmt;

/// Size of a 4 KiB page / frame.
pub const PAGE_SIZE: u64 = 4096;

/// Bits 52..63 of a physical address must be zero on x86_64 (MAXPHYADDR is
/// at most 52).
const PHYS_MASK: u64 = (1 << 52) - 1;

/// A physical memory address.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct PhysAddr(u64);

impl PhysAddr {
    /// Creates a physical address, or `None` if it has bits set above bit 51.
    pub const fn new(addr: u64) -> Option<Self> {
        if addr & !PHYS_MASK == 0 {
            Some(Self(addr))
        } else {
            None
        }
    }

    /// The raw value.
    pub const fn as_u64(self) -> u64 {
        self.0
    }

    /// Rounds down to the start of the containing 4 KiB frame.
    pub const fn align_down(self) -> Self {
        Self(self.0 & !(PAGE_SIZE - 1))
    }

    /// Whether the address is 4 KiB aligned.
    pub const fn is_page_aligned(self) -> bool {
        self.0.is_multiple_of(PAGE_SIZE)
    }
}

/// A canonical virtual address.
///
/// With 4-level paging, bits 48..63 must be copies of bit 47; any other
/// value faults (#GP) when used. The constructor rejects non-canonical
/// values so they can never be stored in a `VirtAddr`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct VirtAddr(u64);

impl VirtAddr {
    /// Creates a virtual address, or `None` if it is not canonical.
    pub const fn new(addr: u64) -> Option<Self> {
        // Sign-extend from bit 47 and compare.
        let canonical = (((addr << 16) as i64) >> 16) as u64;
        if canonical == addr {
            Some(Self(addr))
        } else {
            None
        }
    }

    /// The raw value.
    pub const fn as_u64(self) -> u64 {
        self.0
    }

    /// Whether the address is in the lower (user) half.
    pub const fn is_user(self) -> bool {
        self.0 < 0x0000_8000_0000_0000
    }

    /// Rounds down to the start of the containing 4 KiB page.
    pub const fn align_down(self) -> Self {
        Self(self.0 & !(PAGE_SIZE - 1))
    }
}

// SECURITY: these Debug impls print the raw value. Only use them on the
// early serial log; never in output user space can read (it would leak the
// kernel layout and defeat KASLR, Phase 8).
impl fmt::Debug for PhysAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PhysAddr({:#x})", self.0)
    }
}

impl fmt::Debug for VirtAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "VirtAddr({:#x})", self.0)
    }
}
