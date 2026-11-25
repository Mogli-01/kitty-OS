//! Kernel console: fans text out to every available output.
//!
//! Phase 1 outputs: the early COM1 serial port and a text console drawn on
//! the Limine framebuffer. Both are fixed slots because there is no heap yet;
//! Phase 3 will add dynamic registration of [`Console`] implementations from
//! `drivers/` (see docs/INTERFACES.md).
//!
//! Security note: never print kernel addresses through this console once user
//! space exists — it would defeat KASLR (docs/SECURITY.md).

mod font;
mod framebuffer;

use core::fmt::{self, Write};

use crate::arch::x86_64::serial::SerialPort;
use crate::boot_info::FrameBuffer;
use crate::sync::SpinLock;
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
}

impl Write for Consoles {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        if let Some(serial) = self.serial.as_mut() {
            Console::write_str(serial, s);
        }
        if let Some(fb) = self.fb.as_mut() {
            fb.write_str(s);
        }
        Ok(())
    }
}

static CONSOLES: SpinLock<Consoles> = SpinLock::new(Consoles {
    serial: None,
    fb: None,
});

/// Brings up the serial port and, if present, the framebuffer console.
pub fn init(framebuffer: Option<FrameBuffer>) {
    let mut consoles = CONSOLES.lock();
    consoles.serial = SerialPort::init_com1();
    consoles.fb = framebuffer.map(FbConsole::new);
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
