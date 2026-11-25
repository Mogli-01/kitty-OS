//! x86_64 support: boot hand-off, CPU control, I/O ports, early serial.

pub mod boot;
pub mod cpu;
mod exceptions;
pub mod gdt;
pub mod idt;
pub mod port;
#[cfg(feature = "qemu-exit")]
pub mod qemu;
#[cfg(any(
    feature = "selftest-page-fault",
    feature = "selftest-double-fault",
    feature = "selftest-stack-overflow",
    feature = "selftest-write-text",
    feature = "selftest-exec-data",
    feature = "selftest-write-idt",
    feature = "selftest-heap-overflow"
))]
pub mod selftest;
pub mod serial;
pub mod stacks;
mod trap;

/// Brings up the CPU tables on the boot CPU: GDT + TSS (with the IST
/// stacks), then the IDT. After this, CPU exceptions produce a report
/// instead of a triple fault.
pub fn init() {
    gdt::init();
    idt::init();
}
