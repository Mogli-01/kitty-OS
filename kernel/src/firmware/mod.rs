//! Parsing of firmware-provided data (ACPI). Safe Rust only: raw firmware
//! memory is obtained through `mm::vmm::copy_from_firmware`, which returns
//! *copies*. Everything read here is treated as untrusted
//! input and validated (signatures, checksums, lengths, bounds).

pub mod acpi;
