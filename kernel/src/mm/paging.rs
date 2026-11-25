//! x86_64 4-level page tables.
//!
//! A virtual address is split into four 9-bit indices plus a 12-bit offset:
//!
//! ```text
//!  63    48 47   39 38   30 29   21 20   12 11       0
//! [ sign  ][ PML4 ][ PDPT ][  PD  ][  PT  ][ offset ]
//! ```
//!
//! Each table is one 4 KiB frame of 512 entries. An entry holds the
//! physical address of the next table (or of the final page) plus flags.
//! A PD entry with the PS bit maps a whole 2 MiB page directly.
//!
//! Security rules enforced here, by construction:
//! - **W^X**: a mapping cannot be both writable and executable
//!   ([`MapError::WriteAndExecute`]).
//! - **No silent remaps**: mapping over an existing mapping is an error.
//! - **User bit never in the kernel half**.
//!
//! Intermediate entries are permissive (present + writable, + user for user
//! mappings); the leaf entry alone decides the final permissions, which
//! keeps the rules in one place.

use core::fmt;

use super::addr::{PAGE_SIZE, PhysAddr, VirtAddr};
use super::hhdm;
use super::pmm;
use crate::arch::x86_64::cpu;

const PRESENT: u64 = 1 << 0;
const WRITABLE: u64 = 1 << 1;
const USER: u64 = 1 << 2;
const PWT: u64 = 1 << 3;
const PCD: u64 = 1 << 4;
const HUGE: u64 = 1 << 7;
const GLOBAL: u64 = 1 << 8;
const NO_EXECUTE: u64 = 1 << 63;
const ADDR_MASK: u64 = 0x000F_FFFF_FFFF_F000;

/// Size of a 2 MiB large page.
pub const LARGE_PAGE_SIZE: u64 = 2 * 1024 * 1024;
const ENTRIES: usize = 512;
/// First PML4 index of the kernel (upper) half.
pub const KERNEL_PML4_START: usize = 256;

/// Memory type of a mapping, selected through the PAT (see
/// `cpu::PAT_VALUE`, D-020).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CacheType {
    /// Normal RAM: cached (PAT index 0).
    WriteBack,
    /// Framebuffers: writes are buffered and sent in bursts (PAT index 1).
    WriteCombining,
    /// Device registers: every access goes to the device (PAT index 3).
    Uncached,
}

/// Permissions and attributes of a mapping.
#[derive(Clone, Copy, Debug)]
pub struct MapFlags {
    /// Writes allowed.
    pub writable: bool,
    /// Instruction fetches allowed.
    pub executable: bool,
    /// Accessible from ring 3.
    pub user: bool,
    /// Kept in the TLB across CR3 switches (kernel mappings only).
    pub global: bool,
    /// Memory type.
    pub cache: CacheType,
}

impl MapFlags {
    /// Kernel read-only data.
    pub const KERNEL_RO: Self = Self {
        writable: false,
        executable: false,
        user: false,
        global: true,
        cache: CacheType::WriteBack,
    };
    /// Kernel read/write data.
    pub const KERNEL_RW: Self = Self {
        writable: true,
        ..Self::KERNEL_RO
    };
    /// Kernel code.
    pub const KERNEL_RX: Self = Self {
        executable: true,
        ..Self::KERNEL_RO
    };

    fn bits(self) -> Result<u64, MapError> {
        if self.writable && self.executable {
            return Err(MapError::WriteAndExecute);
        }
        let mut bits = PRESENT;
        if self.writable {
            bits |= WRITABLE;
        }
        if !self.executable {
            bits |= NO_EXECUTE;
        }
        if self.user {
            bits |= USER;
        }
        if self.global && !self.user {
            bits |= GLOBAL;
        }
        bits |= match self.cache {
            CacheType::WriteBack => 0,
            CacheType::WriteCombining => PWT,
            CacheType::Uncached => PWT | PCD,
        };
        Ok(bits)
    }
}

/// Why a mapping operation failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapError {
    /// Requested writable + executable (W^X violation).
    WriteAndExecute,
    /// Something is already mapped there.
    AlreadyMapped,
    /// Nothing is mapped there.
    NotMapped,
    /// A 2 MiB page covers the range where a 4 KiB mapping was requested.
    LargePageInTheWay,
    /// Address not aligned to the page size.
    Misaligned,
    /// User mapping requested in the kernel half.
    UserInKernelHalf,
    /// No free frame for a page table.
    OutOfMemory,
}

impl fmt::Display for MapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

/// Index into the table at `level` (4 = PML4 … 1 = PT).
fn index(virt: VirtAddr, level: u32) -> usize {
    ((virt.as_u64() >> (12 + 9 * (level - 1))) & 0x1FF) as usize
}

/// Pointer to entry `idx` of the table in frame `table`.
fn entry_ptr(table: PhysAddr, idx: usize) -> *mut u64 {
    let base = hhdm::phys_to_virt(table).as_u64() as *mut u64;
    base.wrapping_add(idx % ENTRIES)
}

/// Reads an entry of a page table frame.
///
/// Callers only pass frames that are page tables of a live address space
/// (owned by it) or of the bootloader's tables (read-only inspection).
fn read_entry(table: PhysAddr, idx: usize) -> u64 {
    // SAFETY: `table` is a page-table frame (see above), mapped in the HHDM,
    // 4 KiB aligned; `idx % 512` keeps the access inside it. Volatile: the
    // CPU may update accessed/dirty bits behind the compiler's back.
    unsafe { entry_ptr(table, idx).read_volatile() }
}

fn write_entry(table: PhysAddr, idx: usize, value: u64) {
    // SAFETY: as in `read_entry`; additionally the table belongs to an
    // `AddressSpace` borrowed mutably by the caller, so no one else writes
    // it concurrently.
    unsafe { entry_ptr(table, idx).write_volatile(value) }
}

fn entry_addr(entry: u64) -> Option<PhysAddr> {
    PhysAddr::new(entry & ADDR_MASK)
}

/// A set of page tables (one PML4 and everything below it).
pub struct AddressSpace {
    root: PhysAddr,
}

impl AddressSpace {
    /// Creates an empty address space.
    pub fn new() -> Result<Self, MapError> {
        let frame = pmm::alloc_frame().ok_or(MapError::OutOfMemory)?;
        Ok(Self {
            root: frame.into_raw(),
        })
    }

    /// Physical address of the PML4, for CR3.
    pub fn root(&self) -> PhysAddr {
        self.root
    }

    /// Makes sure PML4 entry `idx` points to a PDPT. Used to pre-create the
    /// whole kernel half, so that every future address space can share the
    /// same kernel PDPTs (Phase 6) and kernel mappings made later are seen
    /// by all of them.
    pub fn ensure_pml4_entry(&mut self, idx: usize) -> Result<(), MapError> {
        self.next_table(self.root, idx % ENTRIES, false).map(|_| ())
    }

    /// Returns the table that entry `idx` of `table` points to, creating an
    /// empty one if needed.
    fn next_table(
        &mut self,
        table: PhysAddr,
        idx: usize,
        user: bool,
    ) -> Result<PhysAddr, MapError> {
        let entry = read_entry(table, idx);
        if entry & PRESENT != 0 {
            if entry & HUGE != 0 {
                return Err(MapError::LargePageInTheWay);
            }
            if user && entry & USER == 0 {
                write_entry(table, idx, entry | USER);
            }
            return entry_addr(entry).ok_or(MapError::NotMapped);
        }
        let frame = pmm::alloc_frame().ok_or(MapError::OutOfMemory)?;
        // Ownership of the (zeroed) frame moves into the page table.
        let addr = frame.into_raw();
        let mut bits = addr.as_u64() | PRESENT | WRITABLE;
        if user {
            bits |= USER;
        }
        write_entry(table, idx, bits);
        Ok(addr)
    }

    fn check(virt: VirtAddr, flags: MapFlags) -> Result<u64, MapError> {
        if flags.user && !virt.is_user() {
            return Err(MapError::UserInKernelHalf);
        }
        flags.bits()
    }

    /// Maps one 4 KiB page.
    pub fn map_4k(
        &mut self,
        virt: VirtAddr,
        phys: PhysAddr,
        flags: MapFlags,
    ) -> Result<(), MapError> {
        if !virt.as_u64().is_multiple_of(PAGE_SIZE) || !phys.is_page_aligned() {
            return Err(MapError::Misaligned);
        }
        let bits = Self::check(virt, flags)?;
        let pdpt = self.next_table(self.root, index(virt, 4), flags.user)?;
        let pd = self.next_table(pdpt, index(virt, 3), flags.user)?;
        let pt = self.next_table(pd, index(virt, 2), flags.user)?;
        let idx = index(virt, 1);
        if read_entry(pt, idx) & PRESENT != 0 {
            return Err(MapError::AlreadyMapped);
        }
        write_entry(pt, idx, phys.as_u64() | bits);
        Ok(())
    }

    /// Maps one 2 MiB page (fewer TLB entries for large linear ranges).
    pub fn map_2m(
        &mut self,
        virt: VirtAddr,
        phys: PhysAddr,
        flags: MapFlags,
    ) -> Result<(), MapError> {
        if !virt.as_u64().is_multiple_of(LARGE_PAGE_SIZE)
            || !phys.as_u64().is_multiple_of(LARGE_PAGE_SIZE)
        {
            return Err(MapError::Misaligned);
        }
        let bits = Self::check(virt, flags)?;
        let pdpt = self.next_table(self.root, index(virt, 4), flags.user)?;
        let pd = self.next_table(pdpt, index(virt, 3), flags.user)?;
        let idx = index(virt, 2);
        if read_entry(pd, idx) & PRESENT != 0 {
            return Err(MapError::AlreadyMapped);
        }
        // With PS set, bit 12 would be the PAT bit; we only use PAT indices
        // 0–3, so it stays clear.
        write_entry(pd, idx, phys.as_u64() | bits | HUGE);
        Ok(())
    }

    /// Removes a 4 KiB mapping, returning the physical page it pointed to.
    /// Flushes the TLB entry (only meaningful if this space is active).
    pub fn unmap_4k(&mut self, virt: VirtAddr) -> Result<PhysAddr, MapError> {
        let mut table = self.root;
        for level in (2..=4).rev() {
            let entry = read_entry(table, index(virt, level));
            if entry & PRESENT == 0 {
                return Err(MapError::NotMapped);
            }
            if entry & HUGE != 0 {
                return Err(MapError::LargePageInTheWay);
            }
            table = entry_addr(entry).ok_or(MapError::NotMapped)?;
        }
        let idx = index(virt, 1);
        let entry = read_entry(table, idx);
        if entry & PRESENT == 0 {
            return Err(MapError::NotMapped);
        }
        write_entry(table, idx, 0);
        cpu::invlpg(virt.as_u64());
        entry_addr(entry).ok_or(MapError::NotMapped)
    }

    /// Physical address `virt` maps to, or `None`.
    pub fn translate(&self, virt: VirtAddr) -> Option<PhysAddr> {
        translate_in(self.root, virt).map(|(p, _)| p)
    }

    /// Counts leaf mappings in the kernel half that break the security
    /// rules: writable *and* executable, or user-accessible.
    pub fn audit_kernel_half(&self) -> AuditReport {
        let mut report = AuditReport::default();
        for i in KERNEL_PML4_START..ENTRIES {
            let e = read_entry(self.root, i);
            if e & PRESENT != 0
                && let Some(t) = entry_addr(e)
            {
                audit(t, 3, &mut report);
            }
        }
        report
    }
}

/// Result of [`AddressSpace::audit_kernel_half`].
#[derive(Clone, Copy, Debug, Default)]
pub struct AuditReport {
    /// Leaf mappings checked.
    pub mappings: u64,
    /// Writable and executable at the same time.
    pub write_and_exec: u64,
    /// User-accessible pages in the kernel half.
    pub user_in_kernel: u64,
}

fn audit(table: PhysAddr, level: u32, r: &mut AuditReport) {
    for i in 0..ENTRIES {
        let e = read_entry(table, i);
        if e & PRESENT == 0 {
            continue;
        }
        let leaf = level == 1 || e & HUGE != 0;
        if leaf {
            r.mappings += 1;
            if e & WRITABLE != 0 && e & NO_EXECUTE == 0 {
                r.write_and_exec += 1;
            }
            if e & USER != 0 {
                r.user_in_kernel += 1;
            }
        } else if let Some(next) = entry_addr(e) {
            audit(next, level - 1, r);
        }
    }
}

/// Walks the tables rooted at `root` (any valid PML4, including the
/// bootloader's) and returns the physical address and whether it is
/// writable.
pub fn translate_in(root: PhysAddr, virt: VirtAddr) -> Option<(PhysAddr, bool)> {
    let mut table = root;
    for level in (1..=4).rev() {
        let entry = read_entry(table, index(virt, level));
        if entry & PRESENT == 0 {
            return None;
        }
        let page_size = 1u64 << (12 + 9 * (level - 1));
        if level == 1 || entry & HUGE != 0 {
            // Large pages: the frame address is aligned to the page size.
            let base = entry & ADDR_MASK & !(page_size - 1);
            let phys = base + (virt.as_u64() & (page_size - 1));
            return PhysAddr::new(phys).map(|p| (p, entry & WRITABLE != 0));
        }
        table = entry_addr(entry)?;
    }
    None
}

/// Physical address of the page table currently in CR3.
pub fn active_root() -> Option<PhysAddr> {
    PhysAddr::new(cpu::read_cr3())
}
