//! x86 I/O port access (`in` / `out` instructions).
//!
//! Design: the *constructor* [`Port::new`] is `unsafe`, `read`/`write` are
//! safe. Port I/O itself never dereferences a Rust pointer, but some devices
//! can be programmed through ports to do DMA into arbitrary physical memory,
//! which *would* break memory safety. So whoever creates a `Port` promises
//! that the port is harmless (or only used in a harmless way) and the rest of
//! the code can use it freely. Only device-access layers create ports.

use core::arch::asm;
use core::marker::PhantomData;

mod sealed {
    pub trait Sealed {}
    impl Sealed for u8 {}
    impl Sealed for u16 {}
    impl Sealed for u32 {}
}

/// Integer widths that can be transferred through an I/O port.
///
/// Sealed: only `u8`, `u16` and `u32` exist on x86.
pub trait PortWidth: sealed::Sealed + Copy {
    /// Reads a value from `port`.
    ///
    /// # Safety
    /// The caller must uphold the contract of [`Port::new`] for `port`.
    unsafe fn port_in(port: u16) -> Self;

    /// Writes `value` to `port`.
    ///
    /// # Safety
    /// The caller must uphold the contract of [`Port::new`] for `port`.
    unsafe fn port_out(port: u16, value: Self);
}

// Note on asm options: we deliberately do NOT pass `nomem`. A port write can
// start a device operation that reads memory we wrote just before (e.g. a
// DMA descriptor). Without `nomem`, the asm block acts as a compiler memory
// barrier, so those memory writes cannot be moved after the `out`.

impl PortWidth for u8 {
    #[inline]
    unsafe fn port_in(port: u16) -> u8 {
        let value: u8;
        // SAFETY: forwarded from the caller (Port::new contract). `in al, dx`
        // touches no stack and no flags.
        unsafe {
            asm!("in al, dx", out("al") value, in("dx") port, options(nostack, preserves_flags));
        }
        value
    }

    #[inline]
    unsafe fn port_out(port: u16, value: u8) {
        // SAFETY: forwarded from the caller (Port::new contract).
        unsafe {
            asm!("out dx, al", in("dx") port, in("al") value, options(nostack, preserves_flags));
        }
    }
}

impl PortWidth for u16 {
    #[inline]
    unsafe fn port_in(port: u16) -> u16 {
        let value: u16;
        // SAFETY: forwarded from the caller (Port::new contract).
        unsafe {
            asm!("in ax, dx", out("ax") value, in("dx") port, options(nostack, preserves_flags));
        }
        value
    }

    #[inline]
    unsafe fn port_out(port: u16, value: u16) {
        // SAFETY: forwarded from the caller (Port::new contract).
        unsafe {
            asm!("out dx, ax", in("dx") port, in("ax") value, options(nostack, preserves_flags));
        }
    }
}

impl PortWidth for u32 {
    #[inline]
    unsafe fn port_in(port: u16) -> u32 {
        let value: u32;
        // SAFETY: forwarded from the caller (Port::new contract).
        unsafe {
            asm!("in eax, dx", out("eax") value, in("dx") port, options(nostack, preserves_flags));
        }
        value
    }

    #[inline]
    unsafe fn port_out(port: u16, value: u32) {
        // SAFETY: forwarded from the caller (Port::new contract).
        unsafe {
            asm!("out dx, eax", in("dx") port, in("eax") value, options(nostack, preserves_flags));
        }
    }
}

/// A typed handle to one I/O port.
pub struct Port<T: PortWidth> {
    port: u16,
    _width: PhantomData<T>,
}

impl<T: PortWidth> Port<T> {
    /// Creates a handle to I/O port `port`.
    ///
    /// # Safety
    /// Reading or writing values of type `T` to this port, in any order and
    /// any number of times, must not be able to violate memory safety (for
    /// example by starting DMA into memory the kernel uses, or by changing the
    /// memory map).
    pub const unsafe fn new(port: u16) -> Self {
        Self {
            port,
            _width: PhantomData,
        }
    }

    /// Reads from the port.
    #[inline]
    pub fn read(&self) -> T {
        // SAFETY: the contract of `Port::new` makes any access to this port
        // memory-safe.
        unsafe { T::port_in(self.port) }
    }

    /// Writes to the port.
    #[inline]
    pub fn write(&self, value: T) {
        // SAFETY: the contract of `Port::new` makes any access to this port
        // memory-safe.
        unsafe { T::port_out(self.port, value) }
    }
}
