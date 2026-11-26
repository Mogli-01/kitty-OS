//! Device bring-up: the point in boot where drivers are connected.
//!
//! `devices::init` runs once, on the boot CPU, after memory management,
//! the heap and the interrupt core are ready and **before** interrupts are
//! enabled. A driver started here can therefore allocate, claim its
//! hardware (`arch::legacy::claim`, `mm::vmm::map_mmio`) and register its
//! IRQ (`arch::irq::register_isa`) without racing its own first interrupt.
//!
//! Driver *logic* lives in `drivers/` (Codex): `no_std`,
//! `#![forbid(unsafe_code)]` crates that only see `kitty_hal` traits. The
//! glue that wires a driver to the kernel goes in this module (one file per
//! device) and is written in safe Rust — `unsafe` is denied here by the
//! crate-level lint. See docs/INTERFACES.md ("Interrupções (Fase 4)").

/// Platform description handed to the drivers.
pub struct Platform<'a> {
    /// The ACPI MADT, already validated by the kernel (signature, length,
    /// checksum; at most 64 KiB) and **copied** into kernel memory: an
    /// immutable buffer that cannot change while the driver parses it.
    /// `None` when there is no usable MADT: keep the 8259 PIC then.
    /// Valid for the duration of `init` only.
    pub madt: Option<&'a [u8]>,
    /// The initrd module (`module_string: kitty-initrd`), validated by the
    /// kernel (single module, size 1 B–128 MiB, entirely inside "kernel and
    /// modules" memory) and mapped read-only + NX for the kernel's whole
    /// life. `None` when there is no module or it was refused (the reason
    /// is logged). The VFS must still validate the archive format.
    pub initrd: Option<&'static [u8]>,
}

/// Starts every driver. Order matters (docs/INTERFACES.md, "Transição PIC
/// para APIC"): the interrupt controller first, then the devices that
/// register IRQ lines.
pub fn init(_platform: &Platform<'_>) {}
