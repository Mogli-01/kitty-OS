//! Kernel thread stacks (Phase 5), each with an unmapped guard page.
//!
//! The stack region (`0xFFFF_E000_0000_0000`, see docs/ARCHITECTURE.md) is
//! cut into fixed 64 KiB slots:
//!
//! ```text
//!   slot base                                       slot base + 64 KiB
//!   [ guard 4 KiB | stack 32 KiB (mapped RW, NX) | unmapped 28 KiB ]
//! ```
//!
//! A stack overflow runs into the guard page below the stack and faults
//! (the fault cannot be delivered on the same broken stack, so it becomes a
//! double fault on the IST stack, which reports it). The unmapped gap above
//! the stack also catches stray writes past the top. Slots are reused after
//! a thread dies; the frames are returned to the PMM, which zeroes them, so
//! no data leaks from one thread's stack to the next.

use alloc::vec::Vec;

use super::addr::{PAGE_SIZE, VirtAddr};
use super::paging::{MapError, MapFlags};
use super::pmm::{self, PhysFrame};
use super::vmm;
use crate::sync::IrqSpinLock;

const STACK_REGION: u64 = 0xFFFF_E000_0000_0000;
const STACK_REGION_SIZE: u64 = 1 << 40;
const SLOT_SIZE: u64 = 64 * 1024;
const GUARD_SIZE: u64 = PAGE_SIZE;
/// Usable size of each thread stack.
pub const STACK_SIZE: u64 = 32 * 1024;
const STACK_PAGES: u64 = STACK_SIZE / PAGE_SIZE;
const MAX_SLOTS: u64 = STACK_REGION_SIZE / SLOT_SIZE;

struct Slots {
    next: u64,
    free: Vec<u64>,
}

static SLOTS: IrqSpinLock<Slots> = IrqSpinLock::new(Slots {
    next: 0,
    free: Vec::new(),
});

/// A mapped kernel stack. Dropping it unmaps the pages, returns the frames
/// to the PMM (zeroed there) and frees the slot.
pub struct KernelStack {
    slot: u64,
}

impl KernelStack {
    fn base(&self) -> u64 {
        STACK_REGION + self.slot * SLOT_SIZE + GUARD_SIZE
    }

    /// Initial stack pointer (one past the highest byte; 16-byte aligned).
    pub fn top(&self) -> u64 {
        self.base() + STACK_SIZE
    }

    /// Address of the guard page (unmapped), for tests.
    pub fn bottom(&self) -> u64 {
        self.base()
    }

    /// Address of the guard page right below the stack.
    pub fn guard_page(&self) -> u64 {
        self.base() - GUARD_SIZE
    }

    /// Allocates a stack and writes `initial` into its highest words (the
    /// last element of `initial` ends at `top()`). Writing happens before
    /// anyone can run on the stack, which is why it is part of creation.
    pub fn new(initial: &[u64]) -> Result<Self, MapError> {
        if initial.len() as u64 * 8 > STACK_SIZE {
            return Err(MapError::Misaligned);
        }
        let slot = {
            let mut s = SLOTS.lock();
            let slot = match s.free.pop() {
                Some(slot) => slot,
                None if s.next < MAX_SLOTS => {
                    s.next += 1;
                    s.next - 1
                }
                None => return Err(MapError::OutOfMemory),
            };
            // Keep room in the free list for every slot ever handed out, so
            // that `Drop` (which can run at the end of an IRQ, when the
            // scheduler reaps a dead thread) never allocates (review of #39).
            let needed = usize::try_from(s.next).unwrap_or(usize::MAX);
            let additional = needed.saturating_sub(s.free.len());
            if s.free.try_reserve(additional).is_err() {
                s.free.push(slot); // capacity for it already exists
                return Err(MapError::OutOfMemory);
            }
            slot
        };
        let stack = KernelStack { slot };
        let base = stack.base();
        // Map the stack pages. On failure, `stack` is dropped, which unmaps
        // whatever was mapped and returns the slot.
        vmm::with_kernel_space(|space| -> Result<(), MapError> {
            for i in 0..STACK_PAGES {
                let virt = VirtAddr::new(base + i * PAGE_SIZE).ok_or(MapError::Misaligned)?;
                let frame = pmm::alloc_frame().ok_or(MapError::OutOfMemory)?;
                space.map_4k(virt, frame.addr(), MapFlags::KERNEL_RW)?;
                // Ownership of the frame moves into the page table; it is
                // taken back in `Drop`.
                let _ = frame.into_raw();
            }
            Ok(())
        })
        .ok_or(MapError::NotMapped)??;

        let words = core::ptr::with_exposed_provenance_mut::<u64>(
            (stack.top() - initial.len() as u64 * 8) as usize,
        );
        // SAFETY: the destination is the top `initial.len()` words of the
        // stack pages mapped read/write just above (length checked against
        // STACK_SIZE). The stack is brand new: no thread runs on it and no
        // other reference to it exists. The frames came zeroed from the PMM.
        unsafe {
            core::ptr::copy_nonoverlapping(initial.as_ptr(), words, initial.len());
        }
        Ok(stack)
    }
}

impl Drop for KernelStack {
    fn drop(&mut self) {
        let base = self.base();
        vmm::with_kernel_space(|space| {
            for i in 0..STACK_PAGES {
                let Some(virt) = VirtAddr::new(base + i * PAGE_SIZE) else {
                    continue;
                };
                if let Ok(phys) = space.unmap_4k(virt) {
                    // SAFETY: every mapped stack page holds a frame turned
                    // into a raw page-table reference by `new`; it has just
                    // been unmapped, so this is its only owner.
                    drop(unsafe { PhysFrame::from_raw(phys) });
                }
            }
        });
        // Never reallocates: `new` reserved room for every slot.
        SLOTS.lock().free.push(self.slot);
    }
}
