//! The address space of one process (Phase 6b, docs/FASE6.md §1, D-060).
//!
//! A [`UserSpace`] owns a PML4 whose kernel half (entries 256-511) is the
//! kernel's own, shared by every process, and whose user half (0-255) holds
//! only this process's pages. It is the only way to build user mappings,
//! and it enforces the user-half rules on every mapping:
//! - only `[USER_MIN, USER_END)`: the first 1 MiB is never mapped;
//! - every entry has the user bit (the invariant of D-054: no supervisor
//!   page in the lower half, or `copy_*_user` could reach it);
//! - W^X, never global;
//! - every page is a fresh frame, zeroed by the PMM before the contents are
//!   copied in, so no data from the kernel or another process remains.
//!
//! Dropping a `UserSpace` frees every page and table of the user half.

use super::addr::{PAGE_SIZE, VirtAddr};
use super::hhdm;
use super::paging::{AddressSpace, CacheType, MapError, MapFlags, UserAudit};
use super::pmm;
use super::vmm;

/// Permissions of a user page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UserPerm {
    /// Read-only data.
    Read,
    /// Read/write data (never executable).
    ReadWrite,
    /// Code (read + execute, never writable).
    ReadExec,
}

impl UserPerm {
    fn flags(self) -> MapFlags {
        MapFlags {
            writable: self == UserPerm::ReadWrite,
            executable: self == UserPerm::ReadExec,
            user: true,
            global: false,
            cache: CacheType::WriteBack,
        }
    }
}

/// A process address space.
pub struct UserSpace {
    /// `None` only during `drop`.
    space: Option<AddressSpace>,
}

impl UserSpace {
    /// A new address space with an empty user half.
    pub fn new() -> Result<Self, MapError> {
        let space = vmm::with_kernel_space(|kernel| AddressSpace::new_user(kernel))
            .ok_or(MapError::OutOfMemory)??;
        Ok(Self { space: Some(space) })
    }

    /// Physical address of the PML4 (what CR3 gets).
    pub fn root(&self) -> u64 {
        self.space.as_ref().map_or(0, |s| s.root().as_u64())
    }

    /// Maps a fresh, zeroed page at `virt` with `perm`, and copies
    /// `contents` (at most one page) to its start.
    pub fn map_page(&mut self, virt: u64, perm: UserPerm, contents: &[u8]) -> Result<(), MapError> {
        if contents.len() > PAGE_SIZE as usize {
            return Err(MapError::TooLarge);
        }
        let space = self.space.as_mut().ok_or(MapError::OutOfMemory)?;
        let virt = VirtAddr::new(virt).ok_or(MapError::UserInKernelHalf)?;
        let mut frame = pmm::alloc_frame().ok_or(MapError::OutOfMemory)?;
        let n = contents.len().min(PAGE_SIZE as usize);
        if let (Some(src), Some(dst)) =
            (contents.get(..n), frame_bytes_mut(&mut frame).get_mut(..n))
        {
            dst.copy_from_slice(src);
        }
        space.map_user_frame(virt, frame, perm.flags())
    }

    /// Checks the user half against the rules (see the module docs).
    pub fn audit(&self) -> UserAudit {
        self.space
            .as_ref()
            .map_or_else(UserAudit::default, AddressSpace::audit_user_half)
    }

    /// Reads a `u64` of this process's memory through the kernel's own view
    /// of the frame (no CR3 switch). For self-tests and diagnostics; normal
    /// kernel access to the *current* process uses `copy_from_user`.
    pub fn peek_u64(&self, virt: u64) -> Option<u64> {
        if !virt.is_multiple_of(8) {
            return None;
        }
        let space = self.space.as_ref()?;
        let phys = space.translate(VirtAddr::new(virt)?)?;
        // Not a user-half address: refuse (translate also walks the shared
        // kernel half).
        if virt >= super::paging::USER_END {
            return None;
        }
        let ptr = hhdm::phys_to_virt(phys).as_u64() as *const u64;
        // SAFETY: `phys` is the frame of a present user page of this space
        // (owned by its tables, which `&self` keeps alive), mapped in the
        // HHDM; `virt` is 8-aligned, so the u64 stays inside that page.
        // Volatile: user code may be writing it concurrently.
        Some(unsafe { ptr.read_volatile() })
    }
}

impl Drop for UserSpace {
    fn drop(&mut self) {
        if let Some(space) = self.space.take() {
            space.destroy_user();
        }
    }
}

/// The kernel's (HHDM) view of a frame we own exclusively.
fn frame_bytes_mut(frame: &mut pmm::PhysFrame) -> &mut [u8] {
    let ptr = hhdm::phys_to_virt(frame.addr()).as_u64() as *mut u8;
    // SAFETY: the frame was just allocated and is not mapped anywhere else
    // yet (the `&mut` borrow of its only reference proves exclusivity); the
    // HHDM maps all of it writable, and the slice covers exactly one page.
    unsafe { core::slice::from_raw_parts_mut(ptr, PAGE_SIZE as usize) }
}
