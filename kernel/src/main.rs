//! kitty-OS kernel.
//!
//! Boot flow (Phase 1):
//!
//! 1. Limine loads this ELF into the higher half, maps every segment with the
//!    permissions from `linker-x86_64.ld`, fills in our boot requests and jumps
//!    to [`arch::x86_64::boot`]'s `kernel_entry`.
//! 2. The arch layer checks the protocol revision, turns the raw bootloader
//!    structures into the safe [`boot_info::BootInfo`] and calls [`kmain`].
//! 3. [`kmain`] brings up the consoles and prints the greeting.
//!
//! Unsafe code policy: `unsafe` is denied for the whole crate and only allowed
//! in the modules listed in `docs/UNSAFE.md` (`arch`, `mm`, `sync`). Every
//! other module must be written in safe Rust on top of their APIs.

#![no_std]
#![no_main]
#![deny(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(clippy::undocumented_unsafe_blocks)]
#![deny(clippy::missing_safety_doc)]
#![deny(clippy::multiple_unsafe_ops_per_block)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

#[allow(unsafe_code)]
pub mod arch;
pub mod boot_info;
pub mod console;
#[allow(unsafe_code)]
pub mod mm;
mod panic;
pub mod sched;
pub mod security;
#[allow(unsafe_code)]
pub mod sync;
pub mod syscall;

use boot_info::BootInfo;

/// Architecture-independent kernel entry point, called by the arch layer once
/// the bootloader hand-off has been validated.
pub fn kmain(boot: BootInfo) -> ! {
    console::init(boot.framebuffer);

    kprintln!("Hello from kitty-OS");
    if let Some(loader) = boot.bootloader {
        kprintln!("[boot] loaded by {} {}", loader.name, loader.version);
    }
    kprintln!("[boot] phase 1 complete, halting");

    #[cfg(feature = "qemu-exit")]
    arch::qemu::exit(arch::qemu::ExitCode::Success);

    #[cfg(not(feature = "qemu-exit"))]
    arch::halt_forever()
}
