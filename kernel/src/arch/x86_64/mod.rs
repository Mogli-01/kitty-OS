//! x86_64 support: boot hand-off, CPU control, I/O ports, early serial.

pub mod boot;
pub mod cpu;
pub mod port;
#[cfg(feature = "qemu-exit")]
pub mod qemu;
pub mod serial;
