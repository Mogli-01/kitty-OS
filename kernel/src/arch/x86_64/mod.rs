//! x86_64 support: boot hand-off, CPU control, I/O ports, early serial.

pub mod boot;
pub mod cpu;
mod exceptions;
mod gdt;
mod idt;
pub mod port;
#[cfg(feature = "qemu-exit")]
pub mod qemu;
#[cfg(any(feature = "selftest-page-fault", feature = "selftest-double-fault"))]
pub mod selftest;
pub mod serial;
mod trap;

/// Brings up the CPU tables on the boot CPU: GDT + TSS (with the IST
/// stacks), then the IDT. After this, CPU exceptions produce a report
/// instead of a triple fault.
pub fn init() {
    gdt::init();
    idt::init();
}
