//! Limine boot protocol hand-off.
//!
//! The requests below are placed in the `.requests` section, which the linker
//! script maps **read-only**. Limine fills in the response pointers while it
//! still owns the machine (before jumping to us), so the kernel never needs
//! to write them and a bug can never redirect them later.
//!
//! This is the only module that reads raw Limine structures. It validates
//! them and converts them into [`crate::boot_info`] types.

use core::fmt::Write;
use core::sync::atomic::{AtomicBool, Ordering};

use limine::BaseRevision;
use limine::framebuffer::FRAMEBUFFER_RGB;
use limine::memmap::{self, Entry};
use limine::request::{
    BootloaderInfoRequest, ExecutableAddressRequest, FramebufferRequest, HhdmRequest,
    MemmapRequest, ModulesRequest, RsdpRequest,
};
use limine::{RequestsEndMarker, RequestsStartMarker};

use super::{cpu, serial::SerialPort};
use crate::boot_info::{
    BootInfo, BootloaderInfo, ChannelMask, FrameBuffer, InitrdModule, KernelImage, MemoryKind,
    MemoryRegion,
};

/// Protocol base revision we are written against. See docs/DECISIONS.md
/// (D-006): we ask for a fixed, well-known revision instead of "the newest the
/// crate knows", so a crate bump never silently changes boot semantics.
const LIMINE_BASE_REVISION: u64 = 3;

// SAFETY (link_section): `.requests*` are dedicated sections that only hold
// these Limine structures; the linker script keeps them (KEEP) in a read-only
// segment. Nothing else is placed there.
#[used]
#[unsafe(link_section = ".requests_start_marker")]
static REQUESTS_START: RequestsStartMarker = RequestsStartMarker::new();

#[used]
#[unsafe(link_section = ".requests")]
static BASE_REVISION: BaseRevision = BaseRevision::with_revision(LIMINE_BASE_REVISION);

#[used]
#[unsafe(link_section = ".requests")]
static FRAMEBUFFER_REQUEST: FramebufferRequest = FramebufferRequest::new();

#[used]
#[unsafe(link_section = ".requests")]
static BOOTLOADER_INFO_REQUEST: BootloaderInfoRequest = BootloaderInfoRequest::new();

#[used]
#[unsafe(link_section = ".requests")]
static HHDM_REQUEST: HhdmRequest = HhdmRequest::new();

#[used]
#[unsafe(link_section = ".requests")]
static MEMMAP_REQUEST: MemmapRequest = MemmapRequest::new();

#[used]
#[unsafe(link_section = ".requests")]
static EXECUTABLE_ADDRESS_REQUEST: ExecutableAddressRequest = ExecutableAddressRequest::new();

#[used]
#[unsafe(link_section = ".requests")]
static RSDP_REQUEST: RsdpRequest = RsdpRequest::new();

#[used]
#[unsafe(link_section = ".requests")]
static MODULES_REQUEST: ModulesRequest = ModulesRequest::new();

#[used]
#[unsafe(link_section = ".requests_end_marker")]
static REQUESTS_END: RequestsEndMarker = RequestsEndMarker::new();

/// ELF entry point. Limine jumps here in 64-bit long mode, interrupts off,
/// with a valid stack and our segments mapped.
///
/// `no_mangle` so the linker script's `ENTRY(kernel_entry)` can find it.
#[unsafe(no_mangle)]
extern "C" fn kernel_entry() -> ! {
    if !BASE_REVISION.is_supported() {
        // We cannot trust any response. Report on serial (which needs no
        // bootloader data) and stop.
        if let Some(mut s) = SerialPort::init_com1() {
            let _ = writeln!(s, "kitty-OS: unsupported Limine base revision");
        }
        cpu::halt_forever();
    }

    // Without these three the kernel cannot manage memory at all.
    let (Some(hhdm), Some(memmap), Some(image)) = (
        HHDM_REQUEST.response(),
        MEMMAP_REQUEST.response(),
        EXECUTABLE_ADDRESS_REQUEST.response(),
    ) else {
        if let Some(mut s) = SerialPort::init_com1() {
            let _ = writeln!(s, "kitty-OS: bootloader did not provide HHDM/memory map");
        }
        cpu::halt_forever();
    };

    let info = BootInfo {
        hhdm_offset: hhdm.offset,
        memory_map: MemoryMap {
            entries: memmap.entries(),
        },
        kernel_image: KernelImage {
            phys_base: image.physical_base,
            virt_base: image.virtual_base,
        },
        // Base revision 3: the RSDP address is *physical* (not an HHDM
        // pointer). It is only a hint; `firmware::acpi` validates
        // everything it points to.
        rsdp_phys: RSDP_REQUEST
            .response()
            .map(|r| r.address.addr() as u64)
            .filter(|&a| a != 0),
        initrd: find_initrd(),
        framebuffer: take_framebuffer(),
        bootloader: BOOTLOADER_INFO_REQUEST.response().map(|r| BootloaderInfo {
            name: r.name(),
            version: r.version(),
        }),
    };
    crate::kmain(info)
}

/// Builds the safe [`FrameBuffer`] from the first Limine framebuffer.
///
/// Returns `None` if there is none, if it is not 32 bpp RGB, or if its
/// geometry is inconsistent. Can only succeed once (it hands out a
/// `&'static mut`).
fn take_framebuffer() -> Option<FrameBuffer> {
    static TAKEN: AtomicBool = AtomicBool::new(false);
    if TAKEN.swap(true, Ordering::AcqRel) {
        return None;
    }

    let response = FRAMEBUFFER_REQUEST.response()?;
    let fb = response.framebuffers().first()?;
    if fb.bpp != 32 || fb.memory_model != FRAMEBUFFER_RGB {
        return None;
    }

    let width = usize::try_from(fb.width).ok()?;
    let height = usize::try_from(fb.height).ok()?;
    let pitch = usize::try_from(fb.pitch).ok()?;
    let len = pitch.checked_mul(height)?;
    let base = fb.address().cast::<u8>();
    if base.is_null() || base.addr() % 4 != 0 || isize::try_from(len).is_err() {
        return None;
    }

    // SAFETY:
    // - Limine guarantees the framebuffer is mapped (through the HHDM),
    //   writable and `pitch * height` bytes long, and that the memory is not
    //   used by anything else once the kernel runs.
    // - `base` is non-null and `len` fits in `isize` (checked above).
    // - `TAKEN` guarantees this `&'static mut` is created only once, so it
    //   cannot alias another reference.
    // - The memory is plain pixel RAM: normal (non-volatile) writes are fine;
    //   the device only reads it.
    let pixels = unsafe { core::slice::from_raw_parts_mut(base, len) };

    let mask = |shift: u8, size: u8| ChannelMask { shift, size };
    FrameBuffer::new(
        pixels,
        width,
        height,
        pitch,
        mask(fb.red_mask_shift, fb.red_mask_size),
        mask(fb.green_mask_shift, fb.green_mask_size),
        mask(fb.blue_mask_shift, fb.blue_mask_size),
    )
}

/// The bootloader memory map. Lives in bootloader-reclaimable memory, which
/// the kernel never reuses in Phase 3, so the slice stays valid.
#[derive(Clone, Copy)]
pub struct MemoryMap {
    entries: &'static [&'static Entry],
}

impl MemoryMap {
    /// Iterates over the regions, in the bootloader's order (sorted by
    /// address, non-overlapping per the Limine specification).
    pub fn iter(&self) -> impl Iterator<Item = MemoryRegion> + '_ {
        self.entries.iter().map(|e| MemoryRegion {
            base: e.base,
            len: e.length,
            kind: match e.type_ {
                memmap::MEMMAP_USABLE => MemoryKind::Usable,
                memmap::MEMMAP_BOOTLOADER_RECLAIMABLE => MemoryKind::BootloaderReclaimable,
                memmap::MEMMAP_EXECUTABLE_AND_MODULES => MemoryKind::KernelAndModules,
                memmap::MEMMAP_ACPI_RECLAIMABLE => MemoryKind::AcpiReclaimable,
                memmap::MEMMAP_ACPI_NVS => MemoryKind::AcpiNvs,
                memmap::MEMMAP_FRAMEBUFFER => MemoryKind::Framebuffer,
                memmap::MEMMAP_BAD_MEMORY => MemoryKind::Bad,
                _ => MemoryKind::Reserved,
            },
        })
    }
}

/// String that marks the initrd module (`module_string: kitty-initrd` in
/// limine.conf; it reaches us in the file's `cmdline` field).
const INITRD_TAG: &[u8] = b"kitty-initrd";
/// Most bytes of a module string we look at.
const MAX_TAG_SCAN: usize = 64;
/// Most modules we look at.
const MAX_MODULES: usize = 64;

/// Byte offsets of the fields we need in `limine::file::File`, which is
/// `#[repr(C)]` (limine crate 0.6.5, pinned in Cargo.lock): `revision: u64`
/// at 0, `address: *mut ()` at 8, `size: u64` at 16, `path` at 24,
/// `cmdline: *const c_char` at 32. We read them directly instead of calling
/// `File::data()` (creates a slice from unchecked values) or
/// `File::cmdline()` (`unwrap()`s on invalid UTF-8 → panic at boot).
const FILE_ADDRESS: usize = 8;
const FILE_SIZE: usize = 16;
const FILE_CMDLINE: usize = 32;

fn file_field(file: &limine::file::File, offset: usize) -> u64 {
    let base = core::ptr::from_ref(file).cast::<u8>();
    // SAFETY: `file` is a valid `File` provided by Limine (a `#[repr(C)]`
    // struct at least 40 bytes long, see the offsets above), so
    // `base + offset .. + 8` is inside it. `read_unaligned` makes no
    // alignment assumption; every bit pattern is a valid `u64`.
    let field = base.wrapping_add(offset).cast::<u64>();
    // SAFETY: (see above) `field` points 8 readable bytes inside `file`.
    unsafe { field.read_unaligned() }
}

/// Whether the NUL-terminated module string at `ptr` is exactly
/// `kitty-initrd`. Reads at most `MAX_TAG_SCAN` bytes, never panics.
fn is_initrd_tag(ptr: u64) -> bool {
    if ptr == 0 {
        return false;
    }
    for i in 0..MAX_TAG_SCAN {
        let Some(addr) = (ptr as usize).checked_add(i) else {
            return false;
        };
        let p = core::ptr::with_exposed_provenance::<u8>(addr);
        // SAFETY: we trust the bootloader's string pointer (Limine is in the
        // trusted computing base, docs/SECURITY.md): Limine places module
        // strings in bootloader-reclaimable memory mapped by its HHDM (still
        // active: we run on Limine's page tables here) and terminates them
        // with NUL. We stop at the first NUL or after MAX_TAG_SCAN bytes and
        // only read bytes (any value is a valid `u8`).
        let byte = unsafe { p.read_volatile() };
        match (INITRD_TAG.get(i), byte) {
            (None, 0) => return true, // matched the whole tag, then NUL
            (Some(&want), got) if want == got => {}
            _ => return false,
        }
    }
    false
}

/// Finds the module tagged `kitty-initrd`. Only records raw values; the
/// memory-map validation happens in `mm::vmm::initrd_slice` (D-043).
fn find_initrd() -> InitrdModule {
    let Some(response) = MODULES_REQUEST.response() else {
        return InitrdModule::Absent;
    };
    let modules = response.modules();
    if modules.len() > MAX_MODULES {
        return InitrdModule::TooManyModules;
    }
    let mut found = InitrdModule::Absent;
    for file in modules {
        if !is_initrd_tag(file_field(file, FILE_CMDLINE)) {
            continue;
        }
        found = match found {
            InitrdModule::Absent => InitrdModule::Found {
                virt: file_field(file, FILE_ADDRESS),
                size: file_field(file, FILE_SIZE),
            },
            _ => InitrdModule::Ambiguous,
        };
    }
    found
}
