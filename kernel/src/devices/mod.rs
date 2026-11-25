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

/// Starts every driver. Currently none: the PS/2 keyboard transport (issue
/// #13) is the first one expected here.
pub fn init() {}
