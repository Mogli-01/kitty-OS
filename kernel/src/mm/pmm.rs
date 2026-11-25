//! Physical Memory Manager: hands out 4 KiB frames of RAM.
//!
//! Design (see docs/DECISIONS.md D-028):
//! - **Frame database**: one [`FrameInfo`] per physical frame, stored in a
//!   chunk of usable RAM reserved at boot. It holds the frame's reference
//!   count and the free-list link.
//! - **Out-of-band free list**: the "next free frame" link lives in the
//!   frame database, *not* inside the free frame itself. A use-after-free
//!   bug that scribbles on a freed frame therefore cannot corrupt the
//!   allocator and make it hand out an arbitrary address.
//! - **Reference counts** (GUI-PREP): one frame can be mapped in several
//!   address spaces (shared buffers, Phase 12). It is only freed when the
//!   last reference goes away.
//! - **Zeroing**: a frame is cleared when its last reference is dropped, so
//!   stale data never lingers in free memory, and every allocated frame is
//!   guaranteed to be all zeros. Frames that come straight from the
//!   bootloader are zeroed lazily on their first allocation (fast boot).
//! - **Double free / refcount overflow** are detected and treated as fatal
//!   kernel bugs instead of silently corrupting memory.
//!
//! Allocation and free are O(1).

use core::fmt;
use core::mem::{self, size_of};

use super::addr::{PAGE_SIZE, PhysAddr};
use super::hhdm;
use crate::boot_info::{MemoryKind, MemoryMap};
use crate::sync::SpinLock;

/// Marker in `refcount` for frames that are not RAM we manage (firmware,
/// kernel image, the frame database itself, …).
const RESERVED: u32 = u32::MAX;
/// End of the free list.
const NONE: u32 = u32::MAX;
/// Maximum number of references to one frame.
const MAX_REFS: u32 = RESERVED - 1;
/// Frame indices are u32: at most 2^32 - 2 frames (~16 TiB) are managed.
const MAX_FRAMES: u64 = (u32::MAX - 1) as u64;

#[derive(Clone, Copy)]
#[repr(C)]
struct FrameInfo {
    refcount: u32,
    next_free: u32,
    /// Contents unknown (never zeroed by us): zero before handing out.
    dirty: bool,
}

impl FrameInfo {
    const RESERVED: Self = Self {
        refcount: RESERVED,
        next_free: NONE,
        dirty: true,
    };
}

/// An owned reference to one physical frame.
///
/// Dropping it releases the reference; when the last one is released the
/// frame is zeroed and returned to the free list. Because `PhysFrame` is not
/// `Copy`, the compiler prevents double frees of the same reference.
pub struct PhysFrame {
    addr: PhysAddr,
}

impl PhysFrame {
    /// Physical address of the frame (4 KiB aligned).
    pub fn addr(&self) -> PhysAddr {
        self.addr
    }

    /// Takes another reference to the same frame (for sharing it between
    /// address spaces).
    pub fn share(&self) -> Result<PhysFrame, FrameError> {
        PMM.lock()
            .as_mut()
            .ok_or(FrameError::NotInitialised)?
            .add_ref(self.addr)?;
        Ok(PhysFrame { addr: self.addr })
    }

    /// Gives up ownership without releasing the reference. Used by page
    /// tables, which store the address and release it later via
    /// [`PhysFrame::from_raw`].
    pub fn into_raw(self) -> PhysAddr {
        let addr = self.addr;
        mem::forget(self);
        addr
    }

    /// Re-creates an owned reference from [`PhysFrame::into_raw`].
    ///
    /// # Safety
    /// `addr` must come from `into_raw` and that reference must not have
    /// been re-created before (otherwise it would be released twice).
    pub unsafe fn from_raw(addr: PhysAddr) -> PhysFrame {
        PhysFrame { addr }
    }
}

impl Drop for PhysFrame {
    fn drop(&mut self) {
        if let Some(pmm) = PMM.lock().as_mut() {
            pmm.release(self.addr);
        }
    }
}

impl fmt::Debug for PhysFrame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("PhysFrame").field(&self.addr).finish()
    }
}

/// Errors from frame operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameError {
    /// `pmm::init` has not run.
    NotInitialised,
    /// The frame already has the maximum number of references.
    TooManyReferences,
}

/// Physical memory statistics (no addresses: safe to show anywhere).
#[derive(Debug, Clone, Copy)]
pub struct PmmStats {
    /// Frames managed by the allocator (usable RAM minus the database).
    pub total_frames: u64,
    /// Frames currently free.
    pub free_frames: u64,
}

struct Pmm {
    frames: &'static mut [FrameInfo],
    free_head: u32,
    total: u64,
    free: u64,
}

// SAFETY: `Pmm` owns the frame database exclusively (it is only reachable
// through the `PMM` lock). Moving it between CPUs is fine.
unsafe impl Send for Pmm {}

static PMM: SpinLock<Option<Pmm>> = SpinLock::new(None);

impl Pmm {
    fn index(&self, addr: PhysAddr) -> Option<usize> {
        let idx = usize::try_from(addr.as_u64() / PAGE_SIZE).ok()?;
        (idx < self.frames.len()).then_some(idx)
    }

    fn alloc(&mut self) -> Option<PhysAddr> {
        let head = self.free_head;
        let info = self.frames.get_mut(head as usize)?;
        // The free list is out of band, so this can only fail on a kernel
        // bug in this file; check anyway (defence in depth).
        assert!(info.refcount == 0, "pmm: free list points at a used frame");
        self.free_head = info.next_free;
        info.next_free = NONE;
        info.refcount = 1;
        let dirty = mem::replace(&mut info.dirty, false);
        self.free -= 1;

        let addr = PhysAddr::new(u64::from(head) * PAGE_SIZE)?;
        if dirty {
            zero_frame(addr);
        }
        Some(addr)
    }

    fn add_ref(&mut self, addr: PhysAddr) -> Result<(), FrameError> {
        let idx = self.index(addr).ok_or(FrameError::NotInitialised)?;
        let info = &mut self.frames[idx];
        assert!(
            info.refcount != 0 && info.refcount != RESERVED,
            "pmm: share of a frame that is not allocated"
        );
        if info.refcount >= MAX_REFS {
            return Err(FrameError::TooManyReferences);
        }
        info.refcount += 1;
        Ok(())
    }

    fn release(&mut self, addr: PhysAddr) {
        let Some(idx) = self.index(addr) else {
            panic!("pmm: release of a frame outside managed memory");
        };
        let info = &mut self.frames[idx];
        // Double free or freeing reserved memory: a kernel bug that would
        // otherwise let two owners alias the same memory. Stop here.
        assert!(
            info.refcount != 0 && info.refcount != RESERVED,
            "pmm: double free or free of reserved frame"
        );
        info.refcount -= 1;
        if info.refcount > 0 {
            return;
        }
        zero_frame(addr);
        info.dirty = false;
        info.next_free = self.free_head;
        self.free_head = idx as u32;
        self.free += 1;
    }
}

/// Fills a frame with zeros through the HHDM.
fn zero_frame(addr: PhysAddr) {
    let ptr = hhdm::phys_to_virt(addr).as_u64() as *mut u8;
    // SAFETY: the frame is RAM managed by the PMM (it is in the database),
    // mapped writable in the HHDM, 4 KiB long and page aligned. Its last
    // owner is gone (refcount 0) or it was just taken off the free list, so
    // nobody else accesses it while we hold the PMM lock.
    unsafe {
        core::ptr::write_bytes(ptr, 0, PAGE_SIZE as usize);
    }
}

/// Rounds `x` up to a multiple of the page size (saturating).
fn page_up(x: u64) -> u64 {
    x.saturating_add(PAGE_SIZE - 1) & !(PAGE_SIZE - 1)
}

/// Builds the frame database from the memory map. Must run once, after
/// [`hhdm::init`]. Panics if there is no usable memory (nothing could work).
pub fn init(map: &MemoryMap) {
    let mut guard = PMM.lock();
    if guard.is_some() {
        return;
    }

    // Usable regions, shrunk inward to whole pages; ignore overflowing ones.
    let usable = || {
        map.iter().filter_map(|r| {
            if r.kind != MemoryKind::Usable {
                return None;
            }
            let start = page_up(r.base);
            let end = r.base.checked_add(r.len)? & !(PAGE_SIZE - 1);
            (end > start).then_some((start, end))
        })
    };

    let max_end = usable().map(|(_, end)| end).max().unwrap_or(0);
    let frame_count = (max_end / PAGE_SIZE).min(MAX_FRAMES);
    assert!(frame_count > 0, "pmm: no usable memory");

    // Where to put the database: the first usable region big enough.
    let db_bytes = page_up(frame_count * size_of::<FrameInfo>() as u64);
    let Some((db_start, _)) = usable().find(|&(s, e)| e - s >= db_bytes) else {
        panic!("pmm: no region large enough for the frame database");
    };
    let db_end = db_start + db_bytes;

    let Some(db_phys) = PhysAddr::new(db_start) else {
        panic!("pmm: database address out of range");
    };
    let db_ptr = hhdm::phys_to_virt(db_phys).as_u64() as *mut FrameInfo;
    let Ok(len) = usize::try_from(frame_count) else {
        panic!("pmm: frame count does not fit in usize");
    };
    for i in 0..len {
        let slot = db_ptr.wrapping_add(i);
        // SAFETY: `db_ptr .. db_ptr + len` lies inside a usable RAM region
        // (checked above: the region holds `db_bytes >= len * size_of`
        // bytes), mapped writable by the HHDM and owned by nobody yet.
        // `FrameInfo` is plain data; writing initialises the slot.
        unsafe {
            slot.write(FrameInfo::RESERVED);
        }
    }
    // SAFETY: all `len` elements were initialised just above; the memory is
    // reserved for the database for the rest of the kernel's life (it is
    // marked RESERVED below and never handed out), so `'static mut` is
    // exclusive.
    let frames: &'static mut [FrameInfo] = unsafe { core::slice::from_raw_parts_mut(db_ptr, len) };

    let mut pmm = Pmm {
        frames,
        free_head: NONE,
        total: 0,
        free: 0,
    };
    // Within each region, push from the top so lower addresses come out
    // first (purely cosmetic: any order is correct).
    for (start, end) in usable() {
        let mut addr = end;
        while addr > start {
            addr -= PAGE_SIZE;
            if (db_start..db_end).contains(&addr) {
                continue;
            }
            let idx = (addr / PAGE_SIZE) as usize;
            let Some(info) = pmm.frames.get_mut(idx) else {
                continue; // beyond MAX_FRAMES
            };
            *info = FrameInfo {
                refcount: 0,
                next_free: pmm.free_head,
                dirty: true,
            };
            pmm.free_head = idx as u32;
            pmm.total += 1;
            pmm.free += 1;
        }
    }
    *guard = Some(pmm);
}

/// Allocates one zeroed frame, or `None` if memory is exhausted.
pub fn alloc_frame() -> Option<PhysFrame> {
    let addr = PMM.lock().as_mut()?.alloc()?;
    Some(PhysFrame { addr })
}

/// Current statistics.
pub fn stats() -> PmmStats {
    PMM.lock().as_ref().map_or(
        PmmStats {
            total_frames: 0,
            free_frames: 0,
        },
        |p| PmmStats {
            total_frames: p.total,
            free_frames: p.free,
        },
    )
}

/// Boot self-test: allocation, zeroing, sharing and release.
pub fn selftest() {
    let before = stats().free_frames;
    let a = alloc_frame();
    let b = alloc_frame();
    let (Some(a), Some(b)) = (a, b) else {
        panic!("pmm selftest: allocation failed");
    };
    assert!(a.addr() != b.addr(), "pmm selftest: same frame twice");
    assert!(read_first_word(&a) == 0, "pmm selftest: frame not zeroed");

    write_first_word(&a, 0xDEAD_BEEF_CAFE_F00D);
    let a2 = match a.share() {
        Ok(f) => f,
        Err(e) => panic!("pmm selftest: share failed: {:?}", e),
    };
    let addr = a.addr();
    drop(a);
    // Still referenced by `a2`: must not have been zeroed or freed.
    assert!(
        read_first_word(&a2) == 0xDEAD_BEEF_CAFE_F00D,
        "pmm selftest: shared frame freed early"
    );
    drop(b);
    drop(a2); // last reference: zeroed and pushed on top of the free list
    assert!(stats().free_frames == before, "pmm selftest: frames leaked");

    // The frame we dirtied must come back zeroed (LIFO: it is on top).
    let Some(again) = alloc_frame() else {
        panic!("pmm selftest: re-allocation failed");
    };
    assert!(again.addr() == addr, "pmm selftest: free list order");
    assert!(
        read_first_word(&again) == 0,
        "pmm selftest: freed frame not zeroed"
    );
}

fn read_first_word(frame: &PhysFrame) -> u64 {
    let ptr = hhdm::phys_to_virt(frame.addr()).as_u64() as *const u64;
    // SAFETY: we own a reference to the frame; it is mapped in the HHDM and
    // aligned. Volatile so the test really reads memory.
    unsafe { ptr.read_volatile() }
}

fn write_first_word(frame: &PhysFrame, value: u64) {
    let ptr = hhdm::phys_to_virt(frame.addr()).as_u64() as *mut u64;
    // SAFETY: as in `read_first_word`; the frame is not mapped anywhere else
    // (it was just allocated by this test).
    unsafe { ptr.write_volatile(value) }
}
