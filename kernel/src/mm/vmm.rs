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
            let mapped = VirtAddr::new(virt_start + off)
                .zip(PhysAddr::new(start + off))
                .ok_or(MapError::Misaligned)
                .and_then(|(v, p)| space.map_4k(v, p, flags));
            if let Err(e) = mapped {
                // Undo the pages mapped so far: no half-mapped window stays
                // behind. They are not PMM frames, so nothing is freed.
                let mut undo = 0;
                while undo < off {
                    if let Some(v) = VirtAddr::new(virt_start + undo) {
                        let _ = space.unmap_4k(v);
                    }
                    undo += PAGE_SIZE;
                }
                return Err(e);
            }
            off += PAGE_SIZE;
        }
        Ok(())
    })
    .ok_or(MapError::NotMapped)??;
    Ok(virt_start)
}

/// Largest firmware range `copy_from_firmware` accepts in one call.
pub const MAX_FIRMWARE_COPY: u64 = 64 * 1024;

/// Legacy BIOS area (EBDA + BIOS ROM) where a BIOS RSDP may live even if
/// the memory map does not describe it.
const LEGACY_BIOS_AREA: core::ops::Range<u64> = 0x8_0000..0x10_0000;

/// Physical range conventionally used by the I/O APICs, HPET and local APIC
/// (0xFEC00000–0xFEFFFFFF). UEFI firmware often reports such MMIO as
/// "reserved", so it is refused explicitly.
const APIC_MMIO_RANGE: core::ops::Range<u64> = 0xFEC0_0000..0xFF00_0000;

/// Whether every page of `[start, end)` is firmware memory: inside an ACPI
/// or reserved region of the memory map, or in the legacy BIOS area, and
/// **never** overlapping kernel-owned RAM, the APIC/HPET MMIO range or the
/// local APIC page. Holes (usually MMIO: PCI BARs) are refused.
fn is_firmware_range(start: u64, end: u64) -> bool {
    let map = MEMORY_MAP.lock();
    let Some(map) = map.as_ref() else {
        return false;
    };
    let lapic = crate::arch::x86_64::cpu::local_apic_base();
    let mut page = start;
    while page < end {
        let page_end = page + PAGE_SIZE;
        // Deny first: kernel-owned RAM (the BIOS area below 640 KiB is often
        // ordinary RAM handed out by the PMM), APIC/HPET MMIO, LAPIC page.
        let kernel_owned = map.iter().any(|r| {
            matches!(
                r.kind,
                MemoryKind::Usable
                    | MemoryKind::BootloaderReclaimable
                    | MemoryKind::KernelAndModules
                    | MemoryKind::Framebuffer
                    | MemoryKind::Bad
            ) && r.base < page_end
                && r.base.saturating_add(r.len) > page
        });
        let apic_mmio = APIC_MMIO_RANGE.start < page_end && page < APIC_MMIO_RANGE.end
            || (lapic..lapic.saturating_add(PAGE_SIZE)).contains(&page);
        if kernel_owned || apic_mmio {
            return false;
        }
        let in_bios_area = LEGACY_BIOS_AREA.start <= page && page_end <= LEGACY_BIOS_AREA.end;
        let in_firmware_region = map.iter().any(|r| {
            matches!(
                r.kind,
                MemoryKind::AcpiReclaimable | MemoryKind::AcpiNvs | MemoryKind::Reserved
            ) && r.base <= page
                && r.base.saturating_add(r.len) >= page_end
        });
        if !(in_bios_area || in_firmware_region) {
            return false;
        }
        page = page_end;
    }
    true
}

/// **Copies** `len` bytes of firmware memory (ACPI tables, BIOS area) at
/// physical `phys` into a kernel buffer (D-041, revised after review).
///
/// - Only firmware memory is accepted (ACPI/reserved regions, legacy BIOS
///   area): never kernel-owned RAM, never memory-map holes (MMIO, where a
///   read can have side effects and a WB alias of an UC page is undefined).
/// - The bytes are read **once**, volatile, through a temporary read-only NX
///   mapping that is removed afterwards. Callers validate the copy, so
///   firmware/SMM changing the table later cannot affect what was checked
///   (no double fetch), and no long-lived reference to firmware memory
///   exists.
pub fn copy_from_firmware(phys: u64, len: u64) -> Result<alloc::boxed::Box<[u8]>, MapError> {
    if len == 0 || len > MAX_FIRMWARE_COPY {
        return Err(MapError::Misaligned);
    }
    let end_exact = phys.checked_add(len).ok_or(MapError::Misaligned)?;
    let start = page_down(phys);
    let end = page_up(end_exact);
    if !is_firmware_range(start, end) {
        return Err(MapError::AlreadyMapped);
    }
    let size = end - start;
    let virt_start = map_window(start, size, MapFlags::KERNEL_RO)?;
    let len = usize::try_from(len).map_err(|_| MapError::Misaligned)?;
    let base = virt_start + (phys - start);
    let mut copy = alloc::vec![0u8; len].into_boxed_slice();
    for (i, byte) in copy.iter_mut().enumerate() {
        let ptr = core::ptr::with_exposed_provenance::<u8>(base as usize + i);
        // SAFETY: `base .. base + len` lies inside the read-only mapping
        // created just above and not yet removed. Volatile: firmware memory
        // is not Rust-owned and may change; we read each byte exactly once
        // and never create a reference to it. Any bit pattern is a `u8`.
        *byte = unsafe { ptr.read_volatile() };
    }
    // Remove the temporary mapping. The frames are firmware memory, not
    // PMM frames, so nothing is freed.
    with_kernel_space(|space| {
        let mut off = 0;
        while off < size {
            if let Some(v) = VirtAddr::new(virt_start + off) {
                let _ = space.unmap_4k(v);
            }
            off += PAGE_SIZE;
        }
    });
    Ok(copy)
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

// --------------------------------------------------------------- initrd ---

/// Largest initrd we accept.
pub const MAX_INITRD: u64 = 128 * 1024 * 1024;

/// Why the initrd module was refused (docs/INTERFACES.md, "initrd").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InitrdError {
    /// More than one module is tagged `kitty-initrd`.
    Ambiguous,
    /// Size is 0 or larger than [`MAX_INITRD`].
    BadSize,
    /// The address is not inside the HHDM.
    NotInDirectMap,
    /// The bytes are not entirely inside one "kernel and modules" region of
    /// the memory map.
    OutsideModuleMemory,
    /// The range overlaps the kernel image (same memory-map type as
    /// modules), which has writable pages: refused.
    OverlapsKernel,
    /// A page of the range is mapped writable somewhere we checked.
    WritableAlias,
    /// More boot modules than the kernel inspects.
    TooManyModules,
}

/// Validates the initrd module reported by the bootloader and returns it as
/// an immutable slice valid for the rest of the kernel's life (D-043).
///
/// `Ok(None)` means "no module" (a boot without initrd is allowed).
pub fn initrd_slice(
    module: crate::boot_info::InitrdModule,
) -> Result<Option<&'static [u8]>, InitrdError> {
    use crate::boot_info::InitrdModule;
    let (virt, size) = match module {
        InitrdModule::Absent => return Ok(None),
        InitrdModule::Ambiguous => return Err(InitrdError::Ambiguous),
        InitrdModule::TooManyModules => return Err(InitrdError::TooManyModules),
        InitrdModule::Found { virt, size } => (virt, size),
    };
    if size == 0 || size > MAX_INITRD {
        return Err(InitrdError::BadSize);
    }
    let hhdm = super::hhdm::offset();
    let phys = virt.checked_sub(hhdm).ok_or(InitrdError::NotInDirectMap)?;
    let phys_end = phys.checked_add(size).ok_or(InitrdError::BadSize)?;
    let inside_modules = MEMORY_MAP.lock().is_some_and(|map| {
        map.iter().any(|r| {
            r.kind == MemoryKind::KernelAndModules
                && r.base <= phys
                && r.base.saturating_add(r.len) >= phys_end
        })
    });
    if !inside_modules {
        return Err(InitrdError::OutsideModuleMemory);
    }
    // The kernel image lives in the same memory-map type as modules and
    // has writable pages (.data/.bss): a "module" overlapping it would be an
    // immutable alias of memory the kernel writes, and would leak kernel
    // data to the VFS. Check every image page's *physical* target in our own
    // tables (the image is not assumed to be physically contiguous).
    let image_start = page_down(sym(&raw const __kernel_start));
    let image_end = page_up(sym(&raw const __data_end));
    let first_page = page_down(phys);
    let last_page_end = page_up(phys_end);
    let overlaps_kernel = with_kernel_space(|space| {
        let mut virt = image_start;
        while virt < image_end {
            if let Some(p) = VirtAddr::new(virt).and_then(|v| space.translate(v)) {
                let page = p.as_u64() & !(PAGE_SIZE - 1);
                if (first_page..last_page_end).contains(&page) {
                    return true;
                }
            }
            virt += PAGE_SIZE;
        }
        false
    })
    .unwrap_or(true);
    if overlaps_kernel {
        return Err(InitrdError::OverlapsKernel);
    }
    // Every HHDM page of the range must be mapped read-only (a page shared
    // with a neighbouring RW region, e.g. ACPI, could have been mapped RW
    // first: "first mapping wins" in `map_hhdm`).
    let writable = with_kernel_space(|space| {
        let mut page = first_page;
        while page < last_page_end {
            let mapping = VirtAddr::new(page.saturating_add(hhdm))
                .and_then(|v| paging::translate_in(space.root(), v));
            match mapping {
                Some((_, false)) => {}
                _ => return true, // writable or unmapped: refuse
            }
            page += PAGE_SIZE;
        }
        false
    })
    .unwrap_or(true);
    if writable {
        return Err(InitrdError::WritableAlias);
    }
    let start = PhysAddr::new(phys).ok_or(InitrdError::NotInDirectMap)?;
    let ptr = core::ptr::with_exposed_provenance::<u8>(
        super::hhdm::phys_to_virt(start).as_u64() as usize
    );
    let len = usize::try_from(size).map_err(|_| InitrdError::BadSize)?;
    // SAFETY: `[phys, phys + size)` lies entirely inside one
    // KernelAndModules region, does not overlap any physical page of the
    // kernel image (the only other mapping of such memory, checked page by
    // page above), and every one of its HHDM pages is mapped read-only and
    // NX (checked above). The PMM never hands out these frames (it only
    // manages Usable memory) and nothing remaps them, so no writable alias
    // exists and the bytes are never freed: an immutable `&'static [u8]` is
    // sound. Every bit pattern is a valid `u8`.
    Ok(Some(unsafe { core::slice::from_raw_parts(ptr, len) }))
}

/// Boot self-test of [`initrd_slice`] with forged modules: one pointing at
/// the kernel's own `.text`, one too large, one outside the direct map.
/// Every one must be refused with the right error.
pub fn initrd_selftest() -> bool {
    use crate::boot_info::InitrdModule;
    let hhdm = super::hhdm::offset();
    let text = VirtAddr::new(sym(&raw const __text_start));
    let Some(text_phys) = text.and_then(|v| with_kernel_space(|s| s.translate(v)).flatten()) else {
        return false;
    };
    let over_kernel = InitrdModule::Found {
        virt: hhdm + text_phys.as_u64(),
        size: PAGE_SIZE,
    };
    let too_big = InitrdModule::Found {
        virt: hhdm + text_phys.as_u64(),
        size: MAX_INITRD + 1,
    };
    let not_hhdm = InitrdModule::Found {
        virt: hhdm.wrapping_sub(PAGE_SIZE),
        size: PAGE_SIZE,
    };
    initrd_slice(over_kernel) == Err(InitrdError::OverlapsKernel)
        && initrd_slice(too_big) == Err(InitrdError::BadSize)
        && initrd_slice(not_hhdm) == Err(InitrdError::NotInDirectMap)
        && initrd_slice(InitrdModule::Ambiguous) == Err(InitrdError::Ambiguous)
        && initrd_slice(InitrdModule::Absent) == Ok(None)
}
