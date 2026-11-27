//! Kernel console: fans text out to every available output.
//!
//! Built-in outputs: the early COM1 serial port and a text console drawn on
//! the Limine framebuffer. Drivers add more with [`register`] (needs the
//! heap; see docs/INTERFACES.md).
//!
//! Security note: never print kernel addresses through this console once user
//! space exists — it would defeat KASLR (docs/SECURITY.md).

mod font;
mod framebuffer;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::{self, Write};

use crate::arch::x86_64::serial::SerialPort;
use crate::boot_info::FrameBuffer;
use crate::sync::IrqSpinLock;
use framebuffer::FbConsole;

/// A text output device. Implemented by kernel consoles and, later, by
/// console drivers from `drivers/`.
pub trait Console: Send {
    /// Writes UTF-8 text. Characters the device cannot show are replaced.
    fn write_str(&mut self, s: &str);
}

impl Console for SerialPort {
    fn write_str(&mut self, s: &str) {
        // Serial writes cannot fail (bytes are dropped on timeout).
        let _ = Write::write_str(self, s);
    }
}

struct Consoles {
    serial: Option<SerialPort>,
    fb: Option<FbConsole>,
    /// Whether the framebuffer is lent out (`lend_framebuffer`).
    lent: bool,
    /// Text written while the framebuffer was lent, replayed on return
    /// (bounded: the oldest text is dropped first).
    pending: String,
    /// Consoles registered at runtime (drivers, Phase 4+).
    extra: Vec<Box<dyn Console>>,
}

impl Write for Consoles {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        if let Some(serial) = self.serial.as_mut() {
            Console::write_str(serial, s);
        }
        if let Some(fb) = self.fb.as_mut() {
            fb.write_str(s);
        } else if self.lent {
            // The buffer was reserved by `lend_framebuffer` (thread
            // context): never allocate here, kprintln may run in an IRQ.
            let cap = self.pending.capacity();
            if s.len() <= cap {
                if self.pending.len() + s.len() > cap {
                    let excess = self.pending.len() + s.len() - cap;
                    let cut = (excess..=self.pending.len())
                        .find(|&i| self.pending.is_char_boundary(i))
                        .unwrap_or(self.pending.len());
                    self.pending.drain(..cut);
                }
                self.pending.push_str(s);
            }
        }
        for c in self.extra.iter_mut() {
            c.write_str(s);
        }
        Ok(())
    }
}

/// Most text kept while the framebuffer is lent.
const PENDING_MAX: usize = 16 * 1024;

static CONSOLES: IrqSpinLock<Consoles> = IrqSpinLock::new(Consoles {
    serial: None,
    fb: None,
    lent: false,
    pending: String::new(),
    extra: Vec::new(),
});

/// Adds a console that will receive all kernel output from now on.
/// Needs the heap (Phase 3+). Never pass a console that is visible to user
/// space *and* not meant to show kernel logs.
pub fn register(console: Box<dyn Console>) {
    CONSOLES.lock().extra.push(console);
}

/// Brings up the serial port and, if present, the framebuffer console.
pub fn init(framebuffer: Option<FrameBuffer>) {
    let mut consoles = CONSOLES.lock();
    consoles.serial = SerialPort::init_com1();
    consoles.fb = framebuffer.map(FbConsole::new);
}

/// Pixel layout of the lent framebuffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelFormat {
    /// 32 bits per pixel, bytes in memory B, G, R, unused (red at bit 16,
    /// green at 8, blue at 0, 8 bits each): what Limine gives on QEMU and
    /// on almost every UEFI GOP.
    Bgra8888,
    /// Any other 32-bpp layout; use `FramebufferLease::channels`.
    Other,
}

/// Temporary, exclusive use of the boot framebuffer (issue #87, D-062).
///
/// While a lease exists the text console does not draw: kernel messages
/// still go to the serial port and are kept (up to 16 KiB) to be shown when
/// the lease ends. Dropping the lease gives the screen back to the console,
/// which clears it and replays those messages.
///
/// The lease hands out the pixel memory as a plain `&mut [u8]` with a
/// validated geometry, never a pointer: code using it stays safe Rust and
/// cannot write outside the visible framebuffer.
///
/// Limits: during a lease a kernel panic is reported on the serial port
/// only (the panic path cannot take the screen from the lease holder).
pub struct FramebufferLease {
    fb: Option<FrameBuffer>,
}

impl FramebufferLease {
    /// Width in pixels.
    pub fn width(&self) -> usize {
        self.fb.as_ref().map_or(0, FrameBuffer::width)
    }

    /// Height in pixels.
    pub fn height(&self) -> usize {
        self.fb.as_ref().map_or(0, FrameBuffer::height)
    }

    /// Bytes per row (at least `width * 4`).
    pub fn pitch(&self) -> usize {
        self.fb.as_ref().map_or(0, FrameBuffer::pitch)
    }

    /// Pixel layout.
    pub fn format(&self) -> PixelFormat {
        let Some(fb) = self.fb.as_ref() else {
            return PixelFormat::Other;
        };
        let (r, g, b) = fb.channels();
        if (r.shift, r.size, g.shift, g.size, b.shift, b.size) == (16, 8, 8, 8, 0, 8) {
            PixelFormat::Bgra8888
        } else {
            PixelFormat::Other
        }
    }

    /// Channel layout (for [`PixelFormat::Other`]).
    pub fn channels(
        &self,
    ) -> Option<(
        crate::boot_info::ChannelMask,
        crate::boot_info::ChannelMask,
        crate::boot_info::ChannelMask,
    )> {
        self.fb.as_ref().map(FrameBuffer::channels)
    }

    /// The visible pixel memory: `pitch * height` bytes, row by row.
    pub fn pixels_mut(&mut self) -> &mut [u8] {
        match self.fb.as_mut() {
            Some(fb) => fb.pixels_mut(),
            None => &mut [],
        }
    }
}

impl Drop for FramebufferLease {
    fn drop(&mut self) {
        let Some(fb) = self.fb.take() else { return };
        // Clearing the screen and redrawing text is slow (WC memory): do it
        // *outside* the console lock, which disables interrupts (security
        // review: a long replay under it lost timer ticks and keystrokes).
        // 1. Take what was printed so far; keep collecting into a fresh
        //    buffer while we redraw.
        let mut fresh = String::new();
        let _ = fresh.try_reserve_exact(PENDING_MAX);
        let earlier = core::mem::replace(&mut CONSOLES.lock().pending, fresh);
        // 2. Redraw without the lock: only the last screenful of text, so
        //    there is no scrolling.
        let mut console = FbConsole::new(fb);
        console.write_str(last_lines(&earlier, console.rows()));
        drop(earlier);
        // 3. Under the lock: the little that arrived meanwhile, then hand
        //    the screen back.
        let mut consoles = CONSOLES.lock();
        let late = core::mem::take(&mut consoles.pending);
        console.write_str(&late);
        consoles.fb = Some(console);
        consoles.lent = false;
        drop(consoles);
        drop(late);
    }
}

/// The last `rows` lines of `text` (a line may still wrap: at worst the top
/// one scrolls away).
fn last_lines(text: &str, rows: usize) -> &str {
    let trimmed = text.strip_suffix('\n').unwrap_or(text);
    let start = trimmed
        .match_indices('\n')
        .rev()
        .nth(rows.saturating_sub(2))
        .map_or(0, |(i, _)| i + 1);
    text.get(start..).unwrap_or(text)
}

/// Whether the framebuffer is currently lent out.
pub fn framebuffer_lent() -> bool {
    CONSOLES.lock().lent
}

/// Takes the framebuffer away from the text console until the returned
/// lease is dropped. `None` if there is no framebuffer or it is already
/// lent. Thread context only (it allocates); never from a registered
/// `Console`'s `write_str` (the console lock is not re-entrant).
pub fn lend_framebuffer() -> Option<FramebufferLease> {
    let mut consoles = CONSOLES.lock();
    consoles.fb.as_ref()?;
    let mut pending = String::new();
    // Best effort: without memory the messages only go to the serial port.
    let _ = pending.try_reserve_exact(PENDING_MAX);
    consoles.pending = pending;
    let fb = consoles.fb.take()?.into_framebuffer();
    consoles.lent = true;
    Some(FramebufferLease { fb: Some(fb) })
}

/// Implementation detail of [`kprint!`]/[`kprintln!`].
#[doc(hidden)]
pub fn _print(args: fmt::Arguments<'_>) {
    let _ = CONSOLES.lock().write_fmt(args);
}

/// Last-resort output used by the panic handler.
///
/// It must never deadlock: if the panic happened *while* the console lock
/// was held, `lock()` would spin forever. So we only try for a while and
/// then fall back to a freshly initialised serial port, which needs no lock.
pub fn emergency_print(args: fmt::Arguments<'_>) {
    const ATTEMPTS: u32 = 1_000_000;
    for _ in 0..ATTEMPTS {
        if let Some(mut consoles) = CONSOLES.try_lock() {
            let _ = consoles.write_fmt(args);
            return;
        }
        core::hint::spin_loop();
    }
    if let Some(mut serial) = SerialPort::init_com1() {
        let _ = serial.write_fmt(args);
    }
}

/// Debug output that goes **only** to the serial port, never to the screen.
///
/// Use it for anything containing kernel addresses (crash reports): the
/// serial port is the developer's debug channel and will never be exposed to
/// user space, while the screen will be. Never blocks on the console lock.
pub fn debug_print(args: fmt::Arguments<'_>) {
    const ATTEMPTS: u32 = 1_000_000;
    for _ in 0..ATTEMPTS {
        if let Some(mut consoles) = CONSOLES.try_lock() {
            if let Some(serial) = consoles.serial.as_mut() {
                let _ = serial.write_fmt(args);
            }
            return;
        }
        core::hint::spin_loop();
    }
    if let Some(mut serial) = SerialPort::init_com1() {
        let _ = serial.write_fmt(args);
    }
}

/// Prints to every kernel console.
#[macro_export]
macro_rules! kprint {
    ($($arg:tt)*) => ($crate::console::_print(format_args!($($arg)*)));
}

/// Prints to every kernel console, followed by a newline.
#[macro_export]
macro_rules! kprintln {
    () => ($crate::kprint!("\n"));
    ($($arg:tt)*) => ($crate::console::_print(format_args!("{}\n", format_args!($($arg)*))));
}
