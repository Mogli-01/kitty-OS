//! Kernel heap: the `#[global_allocator]` behind `Box`, `Vec`, `String`, ….
//!
//! Two paths (docs/DECISIONS.md D-035):
//!
//! - **Small** (≤ 2048 bytes and alignment ≤ size): segregated free lists,
//!   one per size class (16, 32, …, 2048). A class grabs a fresh 4 KiB page
//!   when empty and cuts it into equal chunks. Allocation and free are O(1)
//!   and chunks are naturally aligned to their size.
//! - **Large** (anything else): whole pages mapped on demand, followed by an
//!   **unmapped guard page**, so overflowing a large buffer faults instead of
//!   corrupting the next allocation. Freed pages are unmapped and returned
//!   to the PMM (which zeroes them).
//!
//! Hardening of the small path:
//! - Every freed chunk is **zeroed**, so stale data (keys, user buffers)
//!   never lingers; as a consequence every allocation is already zeroed.
//! - The free-list link stored in a freed chunk is **encoded** ("safe
//!   linking"): `next ^ (chunk >> 12) ^ secret`. A use-after-free write that
//!   overwrites it decodes to garbage, which is detected (range/alignment
//!   check) and stops the kernel instead of handing out an attacker-chosen
//!   address. The secret comes from the kernel RNG (Phase 8a), which is
//!   seeded before the heap exists (it needs no allocation).
//! - Freeing the chunk that is already at the head of its list (the most
//!   common double free) is detected.
//!
//! Lock order: heap → kernel address space → PMM.

use core::alloc::{GlobalAlloc, Layout};
use core::ptr::{self, null_mut};

use super::addr::{PAGE_SIZE, VirtAddr};
use super::paging::MapFlags;
use super::pmm::{self, PhysFrame};
use super::vmm;
use crate::sync::IrqSpinLock;

const HEAP_BASE: u64 = 0xFFFF_C000_0000_0000;
const HEAP_END: u64 = HEAP_BASE + (1 << 40);

const CLASSES: [usize; 8] = [16, 32, 64, 128, 256, 512, 1024, 2048];
const MAX_SMALL: usize = 2048;

struct Heap {
    /// Encoded heads of the per-class free lists (0 = empty).
    free: [u64; CLASSES.len()],
    /// Next unused virtual address in the heap region.
    next_va: u64,
    secret: u64,
    /// Pages currently mapped (small slabs + large allocations).
    pages: u64,
    ready: bool,
}

/// The kernel's global allocator.
pub struct KernelHeap {
    inner: IrqSpinLock<Heap>,
}

#[global_allocator]
static HEAP: KernelHeap = KernelHeap {
    inner: IrqSpinLock::new(Heap {
        free: [0; CLASSES.len()],
        next_va: HEAP_BASE,
        secret: 0,
        pages: 0,
        ready: false,
    }),
};

/// Heap statistics (no addresses).
#[derive(Clone, Copy, Debug)]
pub struct HeapStats {
    /// 4 KiB pages currently mapped for the heap.
    pub pages: u64,
}

/// Enables the heap. Must run after the VMM switched to the kernel tables
/// and after `rng::init` (the link secret comes from the kernel RNG).
pub fn init() {
    // Taken before the heap lock: the RNG has its own lock and never
    // allocates, so there is no lock-order question.
    let secret = crate::rng::u64() | 1;
    let mut heap = HEAP.inner.lock();
    heap.secret = secret;
    heap.ready = true;
}

/// Current statistics.
pub fn stats() -> HeapStats {
    HeapStats {
        pages: HEAP.inner.lock().pages,
    }
}

fn class_index(layout: Layout) -> Option<usize> {
    let size = layout.size().max(layout.align()).max(1);
    if size > MAX_SMALL {
        return None;
    }
    CLASSES.iter().position(|&c| c >= size)
}

impl Heap {
    fn encode(&self, chunk: u64, next: u64) -> u64 {
        next ^ (chunk >> 12) ^ self.secret
    }

    /// Reserves `pages` pages of heap virtual space plus `guard` unmapped
    /// pages after them, and maps the first `pages` to fresh frames.
    fn map_pages(&mut self, pages: u64, guard: u64) -> Option<u64> {
        let bytes = pages.checked_add(guard)?.checked_mul(PAGE_SIZE)?;
        let start = self.next_va;
        let end = start.checked_add(bytes).filter(|&e| e <= HEAP_END)?;
        let ok = vmm::with_kernel_space(|space| {
            for i in 0..pages {
                let virt = VirtAddr::new(start + i * PAGE_SIZE)?;
                let frame = pmm::alloc_frame()?;
                let phys = frame.addr();
                if space.map_4k(virt, phys, MapFlags::KERNEL_RW).is_err() {
                    return None; // `frame` is dropped and freed
                }
                // The page table now owns the frame; reclaimed in `unmap`.
                let _ = frame.into_raw();
            }
            Some(())
        })
        .flatten();
        // On failure the pages mapped so far stay mapped but unused: the
        // address range is skipped, never handed out twice.
        self.next_va = end;
        ok?;
        self.pages += pages;
        Some(start)
    }

    fn unmap_pages(&mut self, start: u64, pages: u64) {
        vmm::with_kernel_space(|space| {
            for i in 0..pages {
                let Some(virt) = VirtAddr::new(start + i * PAGE_SIZE) else {
                    continue;
                };
                if let Ok(phys) = space.unmap_4k(virt) {
                    // SAFETY: the frame was turned into a raw page-table
                    // reference by `map_pages` and this is its only owner;
                    // it was just unmapped, so nothing can reach it.
                    drop(unsafe { PhysFrame::from_raw(phys) });
                }
            }
        });
        self.pages -= pages;
    }

    fn refill(&mut self, class: usize) -> Option<()> {
        let size = CLASSES[class] as u64;
        let page = self.map_pages(1, 0)?;
        // Push chunks from the end so the lowest address is handed out first.
        let mut offset = PAGE_SIZE;
        while offset >= size {
            offset -= size;
            self.push(class, page + offset);
        }
        Some(())
    }

    fn push(&mut self, class: usize, chunk: u64) {
        let head = self.free[class];
        let next = if head == 0 { 0 } else { head ^ self.secret };
        let link = self.encode(chunk, next);
        let p = ptr::with_exposed_provenance_mut::<u64>(chunk as usize);
        // SAFETY: `chunk` is a chunk of this class inside a mapped heap page
        // (it comes from `refill` or from `dealloc` of a pointer this
        // allocator returned), 8-byte aligned, and no longer in use.
        unsafe { p.write(link) };
        self.free[class] = chunk ^ self.secret;
    }

    fn pop(&mut self, class: usize) -> Option<u64> {
        if self.free[class] == 0 {
            self.refill(class)?;
        }
        let chunk = self.free[class] ^ self.secret;
        let p = ptr::with_exposed_provenance_mut::<u64>(chunk as usize);
        // SAFETY: `chunk` was validated when it became the head (below, or
        // in `push`), so it is a free chunk inside a mapped heap page.
        let link = unsafe { p.read() };
        let next = link ^ (chunk >> 12) ^ self.secret;
        let size = CLASSES[class] as u64;
        let valid =
            next == 0 || ((HEAP_BASE..self.next_va).contains(&next) && next.is_multiple_of(size));
        assert!(valid, "heap: free list corrupted (use-after-free?)");
        self.free[class] = if next == 0 { 0 } else { next ^ self.secret };
        // Clear the link: the whole chunk is now zero.
        // SAFETY: as above; the chunk now belongs to the caller.
        unsafe { p.write(0) };
        Some(chunk)
    }
}

// SAFETY: implements the `GlobalAlloc` contract:
// - small chunks are aligned to their size class, which is ≥ the layout's
//   size and alignment; large allocations are page aligned and we refuse
//   alignments above one page;
// - a chunk/page is never handed out twice while allocated (free lists
//   only contain freed memory; large allocations get fresh address space);
// - `dealloc` receives the same layout as `alloc`, so we find the same
//   class / page count.
unsafe impl GlobalAlloc for KernelHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let mut heap = self.inner.lock();
        if !heap.ready {
            return null_mut();
        }
        match class_index(layout) {
            Some(class) => heap
                .pop(class)
                .map_or(null_mut(), |a| ptr::with_exposed_provenance_mut(a as usize)),
            None => {
                if layout.align() > PAGE_SIZE as usize {
                    return null_mut();
                }
                let pages = (layout.size() as u64).div_ceil(PAGE_SIZE).max(1);
                heap.map_pages(pages, 1)
                    .map_or(null_mut(), |a| ptr::with_exposed_provenance_mut(a as usize))
            }
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let mut heap = self.inner.lock();
        let addr = ptr.addr() as u64;
        match class_index(layout) {
            Some(class) => {
                let size = CLASSES[class];
                assert!(
                    heap.free[class] ^ heap.secret != addr,
                    "heap: double free detected"
                );
                // SAFETY: the caller guarantees `ptr` was returned by
                // `alloc` with this layout, so it is a live chunk of `size`
                // bytes that nobody uses any more. Zeroing removes stale data.
                unsafe { ptr::write_bytes(ptr, 0, size) };
                heap.push(class, addr);
            }
            None => {
                let pages = (layout.size() as u64).div_ceil(PAGE_SIZE).max(1);
                heap.unmap_pages(addr, pages);
            }
        }
    }
}

/// Boot self-test: small and large allocations, zeroing, reuse and the
/// return of large allocations to the PMM.
pub fn selftest() {
    use alloc::boxed::Box;
    use alloc::string::String;
    use alloc::vec::Vec;

    let b = Box::new(0x1234_5678_u64);
    assert!(*b == 0x1234_5678, "heap selftest: box");
    drop(b);

    let mut v: Vec<u64> = Vec::new();
    for i in 0..10_000 {
        v.push(i); // grows through every small class into the large path
    }
    assert!(v.iter().sum::<u64>() == 49_995_000, "heap selftest: vec");
    let peak = stats().pages;
    drop(v);
    assert!(
        stats().pages < peak,
        "heap selftest: large pages not returned"
    );

    let mut s = String::new();
    for _ in 0..100 {
        s.push_str("kitty");
    }
    assert!(s.len() == 500, "heap selftest: string");
    drop(s);

    // A freed chunk must come back zeroed (LIFO: same chunk again). We call
    // our allocator directly: calls through `alloc::alloc` may be optimised
    // away by LLVM as an unused allocation, which would skip the test.
    let layout = Layout::new::<[u64; 8]>();
    // SAFETY: non-zero-size layout; the pointer is checked for null, written
    // within its 64 bytes, and freed with the same layout.
    let first = unsafe { HEAP.alloc(layout) }.cast::<[u64; 8]>();
    assert!(!first.is_null(), "heap selftest: alloc failed");
    // SAFETY: `first` is a valid, aligned, exclusive allocation of [u64; 8].
    unsafe { first.write([0xAB; 8]) };
    // SAFETY: allocated above with `layout`, not used afterwards.
    unsafe { HEAP.dealloc(first.cast(), layout) };
    // SAFETY: as for `first`.
    let second = unsafe { HEAP.alloc(layout) }.cast::<[u64; 8]>();
    assert!(second == first, "heap selftest: chunk not reused");
    // SAFETY: valid allocation of [u64; 8] (just returned by `alloc`).
    let contents = unsafe { second.read() };
    assert!(contents == [0; 8], "heap selftest: freed chunk not zeroed");
    // SAFETY: allocated above with `layout`.
    unsafe { HEAP.dealloc(second.cast(), layout) };
}
