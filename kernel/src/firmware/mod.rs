//! Parsing of firmware-provided data (ACPI). Safe Rust only: raw firmware
//! memory is obtained through `mm::vmm::map_firmware_ro`, which returns
//! immutable byte slices. Everything read here is treated as untrusted
//! input and validated (signatures, checksums, lengths, bounds).

pub mod acpi;
