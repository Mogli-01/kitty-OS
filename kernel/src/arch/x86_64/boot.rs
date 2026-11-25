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
use limine::request::{BootloaderInfoRequest, FramebufferRequest};
use limine::{RequestsEndMarker, RequestsStartMarker};

use super::{cpu, serial::SerialPort};
use crate::boot_info::{BootInfo, BootloaderInfo, ChannelMask, FrameBuffer};

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

    let info = BootInfo {
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
