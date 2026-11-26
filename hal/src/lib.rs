//! Hardware abstraction traits shared by the kernel and the drivers.
//!
//! Drivers (`drivers/`, Codex) are `#![forbid(unsafe_code)]` crates that do
//! not depend on the kernel. They talk to hardware only through these
//! traits; the kernel implements them on top of its audited `unsafe` port
//! and MMIO layers and hands drivers *capability objects* for exactly the
//! registers they are allowed to touch (docs/DECISIONS.md D-038).
//!
//! All methods return `Option`: `None` means the offset is not part of the
//! granted range (or is misaligned). Drivers must treat that as a bug in
//! their own logic, never as a reason to fall back to something unsafe.

#![no_std]
#![forbid(unsafe_code)]

/// A small set of x86 I/O ports granted to one driver, addressed by offset
/// from the device's base port.
pub trait PortBlock {
    /// Reads one byte from `base + offset`.
    fn read8(&self, offset: u16) -> Option<u8>;
    /// Writes one byte to `base + offset`.
    fn write8(&self, offset: u16, value: u8) -> Option<()>;
}

/// A memory-mapped register window granted to one driver, addressed by byte
/// offset. Accesses are volatile and naturally aligned.
pub trait RegisterBlock {
    /// Reads a 32-bit register.
    fn read32(&self, offset: u64) -> Option<u32>;
    /// Writes a 32-bit register.
    fn write32(&self, offset: u64, value: u32) -> Option<()>;
    /// Reads a 64-bit register.
    fn read64(&self, offset: u64) -> Option<u64>;
    /// Writes a 64-bit register.
    fn write64(&self, offset: u64, value: u64) -> Option<()>;
}
