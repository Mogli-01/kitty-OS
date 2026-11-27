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
        }
        for c in self.extra.iter_mut() {
            c.write_str(s);
        }
        Ok(())
    }
}

static CONSOLES: IrqSpinLock<Consoles> = IrqSpinLock::new(Consoles {
    serial: None,
    fb: None,
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

// ------------------------------------------------- user-space console (6c) ---

/// Text written by user programs (`console_write`, D-052, D-063).
///
/// User bytes are untrusted: invalid UTF-8 becomes `?`, and so does every
/// control character except `\n` and `\t` (C0, DEL and C1, including ESC
/// and the 8-bit CSI U+009B). A program can therefore neither drive the
/// terminal of whoever reads the serial port with escape sequences, nor
/// rewrite the current line with `\r` to fake a kernel message.
pub fn write_user_text(bytes: &[u8]) {
    let mut consoles = CONSOLES.lock();
    for chunk in bytes.utf8_chunks() {
        let mut buf = [0u8; 4];
        for c in chunk.valid().chars() {
            let c = if c.is_control() && c != '\n' && c != '\t' {
                '?'
            } else {
                c
            };
            let _ = consoles.write_str(c.encode_utf8(&mut buf));
        }
        if !chunk.invalid().is_empty() {
            let _ = consoles.write_str("?");
        }
    }
}

/// Bytes of keyboard input kept for `console_read`.
const INPUT_CAPACITY: usize = 4096;

struct InputRing {
    bytes: [u8; INPUT_CAPACITY],
    head: usize,
    len: usize,
    /// Bytes dropped because the ring was full (diagnostics).
    dropped: u64,
}

static INPUT: IrqSpinLock<InputRing> = IrqSpinLock::new(InputRing {
    bytes: [0; INPUT_CAPACITY],
    head: 0,
    len: 0,
    dropped: 0,
});
static INPUT_WAIT: crate::sched::WaitQueue = crate::sched::WaitQueue::new();

/// Called by the keyboard driver (Codex) with already-translated bytes
/// (docs/FASE6.md §6). Safe in interrupt handlers: no heap, one short
/// lock. When the ring is full the **new** bytes are dropped and counted.
pub fn input_push(bytes: &[u8]) {
    {
        let mut ring = INPUT.lock();
        for &b in bytes {
            if ring.len == INPUT_CAPACITY {
                ring.dropped += 1;
                continue;
            }
            let tail = (ring.head + ring.len) % INPUT_CAPACITY;
            ring.bytes[tail] = b;
            ring.len += 1;
        }
    }
    // Publish first, then wake (no lost wake-up, D-046).
    INPUT_WAIT.wake_all();
}

/// Bytes dropped so far because nobody read the console fast enough.
pub fn input_dropped() -> u64 {
    INPUT.lock().dropped
}

/// Blocks until at least one input byte is available (or `cancel()`), then
/// moves up to `dst.len()` bytes into `dst`. Returns how many. Thread
/// context only, interrupts enabled.
pub fn input_read(dst: &mut [u8], mut cancel: impl FnMut() -> bool) -> usize {
    if dst.is_empty() {
        return 0;
    }
    INPUT_WAIT.wait_until(|| {
        // Release the ring lock before `cancel` (it may take other locks).
        let ready = INPUT.lock().len > 0;
        ready || cancel()
    });
    let mut ring = INPUT.lock();
    let n = ring.len.min(dst.len());
    for slot in dst.iter_mut().take(n) {
        *slot = ring.bytes[ring.head];
        ring.head = (ring.head + 1) % INPUT_CAPACITY;
        ring.len -= 1;
    }
    n
}

/// Wakes every blocked `input_read` so it can re-check its `cancel`
/// (a process being killed while it waits for a key).
pub fn wake_input_readers() {
    INPUT_WAIT.wake_all();
}
