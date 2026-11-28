//! x86_64 support: boot hand-off, CPU control, I/O ports, early serial.

pub mod apic;
pub mod boot;
pub mod context;
pub mod cpu;
mod exceptions;
pub mod gdt;
pub mod idt;
pub mod irq;
pub mod legacy;
mod pic;
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
    feature = "selftest-heap-overflow",
    feature = "selftest-irq-pit",
    feature = "selftest-preempt",
    feature = "selftest-thread-stack-overflow"
))]
pub mod selftest;
pub mod serial;
pub mod stacks;
mod trap;
pub mod usercopy;
pub mod usermode;

pub use exceptions::swapgs_checks;

/// Brings up the CPU tables on the boot CPU: GDT + TSS (with the IST
/// stacks), then the IDT, then the interrupt core (legacy PIC remapped and
/// fully masked). After this, CPU exceptions produce a report instead of a
/// triple fault. Interrupts stay disabled until `kmain` enables them.
pub fn init() {
    gdt::init();
    idt::init();
    irq::init();
}
