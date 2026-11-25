//! Architecture-specific code. Only x86_64 is supported for now.
//!
//! This is one of the few modules where `unsafe` is allowed (see
//! `docs/UNSAFE.md`). Everything it exports is a safe API.

pub mod x86_64;

pub use x86_64::cpu::{breakpoint, halt_forever};
pub use x86_64::init;
#[cfg(feature = "qemu-exit")]
pub use x86_64::qemu;
