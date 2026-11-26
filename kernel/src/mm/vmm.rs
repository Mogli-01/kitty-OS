//! Virtual Memory Manager: builds the kernel's own page tables and switches
//! to them, replacing the bootloader's.
//!
//! What the new kernel address space contains (docs/ARCHITECTURE.md):
//! - the kernel image, **one permission per section** (W^X):
//!   requests R, `.text` R+X, `.rodata` R, `.data`/`.bss` R+W — with the
//!   stack guard pages left unmapped and the GDT/IDT pages read-only;
//! - the HHDM: every RAM/firmware region from the memory map, never
//!   executable; the kernel image's alias read-only (otherwise `.text` could
//!   be rewritten through the HHDM, bypassing W^X); the framebuffer
//!   write-combining;
//! - an MMIO window for device registers ([`map_mmio`]).
//!
//! Before switching, an audit walks the new tables and refuses to continue
//! if any kernel page is writable+executable or user-accessible.

use core::sync::atomic::{AtomicU64, Ordering};

use super::addr::{PAGE_SIZE, PhysAddr, VirtAddr};
use super::paging::{
    self, AddressSpace, AuditReport, CacheType, KERNEL_PML4_START, LARGE_PAGE_SIZE, MapError,
    MapFlags,
};
use crate::arch::x86_64::{cpu, gdt, idt, stacks};
use crate::boot_info::{BootInfo, MemoryKind, MemoryMap};
use crate::sync::IrqSpinLock;

/// Start of the MMIO window (1 TiB, see the memory map in ARCHITECTURE.md).
const MMIO_BASE: u64 = 0xFFFF_D000_0000_0000;
const MMIO_END: u64 = MMIO_BASE + (1 << 40);

static KERNEL_SPACE: IrqSpinLock<Option<AddressSpace>> = IrqSpinLock::new(None);
static MEMORY_MAP: IrqSpinLock<Option<MemoryMap>> = IrqSpinLock::new(None);
static MMIO_NEXT: AtomicU64 = AtomicU64::new(MMIO_BASE);

// Section boundaries from kernel/linker-x86_64.ld.
unsafe extern "C" {
    static __kernel_start: u8;
    static __text_start: u8;
    static __text_end: u8;
    static __rodata_start: u8;
    static __rodata_end: u8;
    static __data_start: u8;
    static __data_end: u8;
}

fn sym(s: *const u8) -> u64 {
    s.addr() as u64
}

fn page_down(x: u64) -> u64 {
    x & !(PAGE_SIZE - 1)
}

fn page_up(x: u64) -> u64 {
    x.saturating_add(PAGE_SIZE - 1) & !(PAGE_SIZE - 1)
}

/// What the kernel switch produced (no addresses: printable).
#[derive(Clone, Copy, Debug)]
pub struct VmmReport {
    /// Kernel image pages mapped.
    pub image_pages: u64,
    /// HHDM bytes mapped.
    pub hhdm_bytes: u64,
    /// Result of the W^X / user-bit audit.
    pub audit: AuditReport,
}

/// Builds the kernel address space and switches to it. Runs once, on the
/// boot CPU, with interrupts disabled.
pub fn init(boot: &BootInfo) -> VmmReport {
    assert!(cpu::has_nx(), "vmm: CPU without NX is not supported");
    let Some(limine_root) = paging::active_root() else {
        panic!("vmm: invalid CR3");
    };

    let mut space = match AddressSpace::new() {
        Ok(s) => s,
        Err(e) => panic!("vmm: cannot allocate PML4: {}", e),
    };
    for i in KERNEL_PML4_START..512 {
        if let Err(e) = space.ensure_pml4_entry(i) {
            panic!("vmm: cannot pre-allocate kernel half: {}", e);
        }
    }

    let image_pages = map_kernel_image(&mut space, limine_root);
    let hhdm_bytes = map_hhdm(&mut space, boot);

    let audit = space.audit_kernel_half();
    assert!(
        audit.write_and_exec == 0 && audit.user_in_kernel == 0,
        "vmm: audit failed: {:?}",
        audit
    );
    for guard in stacks::guard_pages() {
        let mapped = VirtAddr::new(guard).and_then(|v| space.translate(v));
        assert!(mapped.is_none(), "vmm: a stack guard page is mapped");
    }
    // Last line of defence before CR3: the code we are running and the stack
    // we are on must be mapped, or loading CR3 would triple-fault.
    let here = VirtAddr::new(init as fn(&BootInfo) -> VmmReport as usize as u64);
    let local = 0u8;
    let stack = VirtAddr::new((&raw const local).addr() as u64);
    assert!(
        here.and_then(|v| space.translate(v)).is_some()
            && stack.and_then(|v| space.translate(v)).is_some(),
        "vmm: new tables do not map the running code/stack"
    );

    cpu::enable_nx();
    // SAFETY: the new tables are built for the kernel PAT layout (WB = 0,
    // WC = 1, UC = 3) and are loaded immediately below; nothing touches a
    // mapping whose type changed in between.
    unsafe { cpu::program_pat() };
    // SAFETY: `space` maps the running code, the current stack (checked
    // above), every kernel section and all RAM through the HHDM at the same
    // virtual addresses the kernel already uses, so every live pointer stays
    // valid. NXE is on, so NX bits are honoured instead of reserved.
    unsafe { cpu::write_cr3(space.root().as_u64()) };
    cpu::enable_write_protect();
    cpu::enable_global_pages();

    *MEMORY_MAP.lock() = Some(boot.memory_map);
    *KERNEL_SPACE.lock() = Some(space);
    VmmReport {
        image_pages,
        hhdm_bytes,
        audit,
    }
}

/// Maps every page of the kernel image with its section's permissions.
/// Physical addresses are read from the bootloader's page tables, so no
/// assumption about how the image was laid out in RAM is needed.
fn map_kernel_image(space: &mut AddressSpace, limine_root: PhysAddr) -> u64 {
    let start = sym(&raw const __kernel_start);
    let text = (
        sym(&raw const __text_start),
        page_up(sym(&raw const __text_end)),
    );
    let rodata = (
        sym(&raw const __rodata_start),
        page_up(sym(&raw const __rodata_end)),
    );
    let data = (
        sym(&raw const __data_start),
        page_up(sym(&raw const __data_end)),
    );
    let guards = stacks::guard_pages();
    let read_only_tables = [gdt::table_page(), idt::table_page()];

    let mut pages = 0;
    let mut virt = page_down(start);
    while virt < data.1 {
        let flags = if (text.0..text.1).contains(&virt) {
            Some(MapFlags::KERNEL_RX)
        } else if (data.0..data.1).contains(&virt) {
            if guards.contains(&virt) {
                None
            } else if read_only_tables.contains(&virt) {
                Some(MapFlags::KERNEL_RO)
            } else {
                Some(MapFlags::KERNEL_RW)
            }
        } else if (rodata.0..rodata.1).contains(&virt) || virt < text.0 {
            Some(MapFlags::KERNEL_RO)
        } else {
            None // alignment gap between sections
        };

        if let (Some(flags), Some(v)) = (flags, VirtAddr::new(virt)) {
            let Some((phys, _)) = paging::translate_in(limine_root, v) else {
                panic!("vmm: kernel page not mapped by the bootloader");
            };
            if let Err(e) = space.map_4k(v, phys.align_down(), flags) {
                panic!("vmm: mapping the kernel image failed: {}", e);
            }
            pages += 1;
        }
        virt += PAGE_SIZE;
    }
    pages
}

/// Rebuilds the HHDM with per-region permissions. Uses 2 MiB pages where
/// possible: a 4 GiB machine needs ~2000 entries instead of ~1 million.
fn map_hhdm(space: &mut AddressSpace, boot: &BootInfo) -> u64 {
    let mut bytes = 0;
    for region in boot.memory_map.iter() {
        let flags = match region.kind {
            MemoryKind::Usable
            | MemoryKind::BootloaderReclaimable
            | MemoryKind::AcpiReclaimable
            | MemoryKind::AcpiNvs => MapFlags {
                global: true,
                ..MapFlags::KERNEL_RW
            },
            // The kernel image and boot modules: readable, never writable
            // through the HHDM (would bypass W^X on .text/.rodata).
            MemoryKind::KernelAndModules => MapFlags::KERNEL_RO,
            MemoryKind::Framebuffer => MapFlags {
                cache: CacheType::WriteCombining,
                ..MapFlags::KERNEL_RW
            },
            MemoryKind::Bad | MemoryKind::Reserved => continue,
        };
        let start = page_down(region.base);
        let Some(end) = region.base.checked_add(region.len).map(page_up) else {
            continue;
        };
        let mut phys = start;
        while phys < end {
            let large = phys.is_multiple_of(LARGE_PAGE_SIZE) && end - phys >= LARGE_PAGE_SIZE;
            let step = if large { LARGE_PAGE_SIZE } else { PAGE_SIZE };
            let (Some(p), Some(v)) = (
                PhysAddr::new(phys),
                phys.checked_add(boot.hhdm_offset).and_then(VirtAddr::new),
            ) else {
                break;
            };
            let result = if large {
                space.map_2m(v, p, flags)
            } else {
                space.map_4k(v, p, flags)
            };
            match result {
                Ok(()) => bytes += step,
                // Regions that are not page aligned can share a page with
                // their neighbour: the first mapping wins.
                Err(MapError::AlreadyMapped | MapError::LargePageInTheWay) => {}
                Err(e) => panic!("vmm: HHDM mapping failed: {}", e),
            }
            phys += step;
        }
    }
    bytes
}

/// Runs `f` with the kernel address space (for later phases: thread
/// stacks, heap growth).
pub fn with_kernel_space<R>(f: impl FnOnce(&mut AddressSpace) -> R) -> Option<R> {
    KERNEL_SPACE.lock().as_mut().map(f)
}

/// Checks whether `virt` is mapped in the kernel address space.
pub fn is_mapped(virt: VirtAddr) -> bool {
    with_kernel_space(|s| s.translate(virt).is_some()).unwrap_or(false)
}

// ------------------------------------------------------------------ MMIO ---

impl kitty_hal::RegisterBlock for MmioRegion {
    fn read32(&self, offset: u64) -> Option<u32> {
        self.read(offset)
    }
    fn write32(&self, offset: u64, value: u32) -> Option<()> {
        self.write(offset, value)
    }
    fn read64(&self, offset: u64) -> Option<u64> {
        self.read(offset)
    }
    fn write64(&self, offset: u64, value: u64) -> Option<()> {
        self.write(offset, value)
    }
}

mod sealed {
    pub trait Sealed {}
    impl Sealed for u8 {}
    impl Sealed for u16 {}
    impl Sealed for u32 {}
    impl Sealed for u64 {}
}

/// Register widths accessible through [`MmioRegion`].
pub trait MmioValue: sealed::Sealed + Copy {}
impl MmioValue for u8 {}
impl MmioValue for u16 {}
impl MmioValue for u32 {}
impl MmioValue for u64 {}

/// A mapped range of device registers. All accesses are volatile, bounds
/// checked and naturally aligned; out-of-range accesses return `None`.
pub struct MmioRegion {
    base: u64,
    len: u64,
}

impl MmioRegion {
    /// Size of the region in bytes.
    pub fn len(&self) -> u64 {
        self.len
    }

    /// Whether the region is empty.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn ptr<T: MmioValue>(&self, offset: u64) -> Option<*mut T> {
        let size = core::mem::size_of::<T>() as u64;
        let end = offset.checked_add(size)?;
        if end > self.len || !offset.is_multiple_of(size) {
            return None;
        }
        Some(core::ptr::with_exposed_provenance_mut::<T>(
            (self.base + offset) as usize,
        ))
    }

    /// Reads the register at `offset`.
    pub fn read<T: MmioValue>(&self, offset: u64) -> Option<T> {
        let p = self.ptr::<T>(offset)?;
        // SAFETY: `p` is inside this region (bounds checked), naturally
        // aligned, and mapped uncached/WC by `map_mmio`. The contract of
        // `map_mmio` makes device accesses memory safe.
        Some(unsafe { p.read_volatile() })
    }

    /// Writes the register at `offset`. Returns `None` if out of range.
    pub fn write<T: MmioValue>(&self, offset: u64, value: T) -> Option<()> {
        let p = self.ptr::<T>(offset)?;
        // SAFETY: as in `read`.
        unsafe { p.write_volatile(value) };
        Some(())
    }
}

/// Maps `len` bytes of device registers at physical address `phys` into the
/// MMIO window, never executable, with the given memory type.
///
/// Refuses ranges that overlap RAM or firmware memory from the memory map
/// (an MMIO alias of RAM would bypass every other protection).
///
/// # Safety
/// The range must be device memory for which no sequence of reads/writes
/// can break memory safety (e.g. by programming DMA to arbitrary RAM), or
/// the caller must restrict its accesses so that it cannot.
pub unsafe fn map_mmio(phys: PhysAddr, len: u64, cache: CacheType) -> Result<MmioRegion, MapError> {
    if len == 0 {
        return Err(MapError::Misaligned);
    }
    let start = page_down(phys.as_u64());
    let end = phys
        .as_u64()
        .checked_add(len)
        .map(page_up)
        .ok_or(MapError::Misaligned)?;

    let overlaps_ram = MEMORY_MAP.lock().is_some_and(|map| {
        map.iter().any(|r| {
            r.kind != MemoryKind::Reserved && r.base < end && r.base.saturating_add(r.len) > start
        })
    });
    if overlaps_ram {
        return Err(MapError::AlreadyMapped);
    }

    let flags = MapFlags {
        cache,
        ..MapFlags::KERNEL_RW
    };
    let virt_start = map_window(start, end - start, flags)?;
    Ok(MmioRegion {
        base: virt_start + (phys.as_u64() - start),
        len,
    })
}

/// Maps physical `[start, start + size)` (page aligned) at fresh addresses of
/// the MMIO window and returns the virtual start.
fn map_window(start: u64, size: u64, flags: MapFlags) -> Result<u64, MapError> {
    let virt_start = MMIO_NEXT.fetch_add(size, Ordering::AcqRel);
    if virt_start.checked_add(size).is_none_or(|e| e > MMIO_END) {
        return Err(MapError::OutOfMemory);
    }
    with_kernel_space(|space| {
        let mut off = 0;
        while off < size {
            let v = VirtAddr::new(virt_start + off).ok_or(MapError::Misaligned)?;
            let p = PhysAddr::new(start + off).ok_or(MapError::Misaligned)?;
            space.map_4k(v, p, flags)?;
            off += PAGE_SIZE;
        }
        Ok(())
    })
    .ok_or(MapError::NotMapped)??;
    Ok(virt_start)
}

/// Largest firmware range `map_firmware_ro` accepts in one call.
pub const MAX_FIRMWARE_MAP: u64 = 1024 * 1024;

/// Maps `len` bytes of **firmware memory** (ACPI tables, BIOS area) at
/// physical `phys` read-only, never executable, and returns them as an
/// immutable byte slice that stays valid for the rest of the kernel's life.
///
/// Refuses ranges that overlap memory the kernel owns or hands out (usable
/// RAM, bootloader/kernel memory, the framebuffer) or bad RAM, and ranges
/// larger than [`MAX_FIRMWARE_MAP`]. Firmware tables are input from outside
/// the kernel: callers must still validate their contents (D-041).
pub fn map_firmware_ro(phys: u64, len: u64) -> Result<&'static [u8], MapError> {
    if len == 0 || len > MAX_FIRMWARE_MAP {
        return Err(MapError::Misaligned);
    }
    let end_exact = phys.checked_add(len).ok_or(MapError::Misaligned)?;
    let start = page_down(phys);
    let end = page_up(end_exact);
    let owned_by_kernel = MEMORY_MAP.lock().is_some_and(|map| {
        map.iter().any(|r| {
            matches!(
                r.kind,
                MemoryKind::Usable
                    | MemoryKind::BootloaderReclaimable
                    | MemoryKind::KernelAndModules
                    | MemoryKind::Framebuffer
                    | MemoryKind::Bad
            ) && r.base < end
                && r.base.saturating_add(r.len) > start
        })
    });
    if owned_by_kernel {
        return Err(MapError::AlreadyMapped);
    }
    let virt_start = map_window(start, end - start, MapFlags::KERNEL_RO)?;
    let ptr = core::ptr::with_exposed_provenance::<u8>((virt_start + (phys - start)) as usize);
    let len = usize::try_from(len).map_err(|_| MapError::Misaligned)?;
    // SAFETY: `ptr .. ptr + len` was just mapped read-only in the kernel
    // address space and is never unmapped (the MMIO window is not reused),
    // so the bytes stay readable for `'static`. Every bit pattern is a valid
    // `u8`. The kernel never writes this memory (the mapping is read-only)
    // and the range is not RAM the kernel hands out (checked above), so no
    // `&mut` alias can exist.
    Ok(unsafe { core::slice::from_raw_parts(ptr, len) })
}

/// Boot self-test of [`map_mmio`]: maps the local APIC (present on every
/// x86_64 CPU) and reads its version register (offset 0x30). Returns the
/// version byte. Read-only access to an identification register cannot
/// affect memory.
pub fn mmio_selftest() -> Option<u8> {
    let phys = PhysAddr::new(cpu::local_apic_base())?;
    // SAFETY: the local APIC has no DMA capability; we only read its
    // version register, which has no side effects.
    let apic = unsafe { map_mmio(phys, 0x1000, CacheType::Uncached) }.ok()?;
    // Bounds/alignment checks must reject bad offsets.
    if apic.read::<u32>(0x1000).is_some() || apic.read::<u32>(0x31).is_some() {
        return None;
    }
    apic.read::<u32>(0x30).map(|v| (v & 0xFF) as u8)
}
