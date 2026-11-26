//! ACPI table discovery: RSDP → RSDT/XSDT → MADT (docs/DECISIONS.md D-041).
//!
//! The kernel only does what must be trusted: find the tables, check that
//! they are internally consistent, and hand the driver a stable, immutable
//! **copy** of the MADT. Every table is copied out of firmware memory first
//! (`mm::vmm::copy_from_firmware`) and validated **on the copy**, so a
//! table that changes underneath us (firmware/SMM) cannot make us validate
//! one version and use another. Decoding the MADT is the job of the Codex
//! driver (`kitty-acpi`). The kernel additionally extracts the I/O APIC
//! register addresses, because it only grants MMIO capabilities for
//! addresses the validated MADT declares (`arch::apic`).

use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::mm::vmm::copy_from_firmware;

/// Size of the common ACPI table header.
const SDT_HEADER_LEN: usize = 36;
/// Largest table we accept (same limit as the Codex MADT parser).
pub const MAX_TABLE_LEN: usize = 64 * 1024;
/// Most RSDT/XSDT entries we look at.
const MAX_ROOT_ENTRIES: usize = 256;
/// Most I/O APICs we track.
const MAX_IO_APICS: usize = 16;

/// Why ACPI discovery stopped. Any error means "no APIC topology": the
/// kernel keeps the 8259 PIC.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AcpiError {
    /// The bootloader did not report an RSDP.
    NoRsdp,
    /// A range is not firmware memory, or could not be copied.
    Unmappable,
    /// Wrong signature.
    BadSignature,
    /// Checksum mismatch.
    BadChecksum,
    /// Length field out of range.
    BadLength,
    /// The RSDT/XSDT has no MADT ("APIC") entry.
    NoMadt,
}

/// What discovery found.
pub struct AcpiTables {
    /// Validated copy of the MADT (signature, length and checksum checked
    /// on this very copy).
    pub madt: Box<[u8]>,
    /// I/O APIC register base addresses declared by the MADT.
    pub io_apics: Vec<u64>,
}

fn copy(phys: u64, len: usize) -> Result<Box<[u8]>, AcpiError> {
    copy_from_firmware(phys, len as u64).map_err(|_| AcpiError::Unmappable)
}

fn checksum_ok(bytes: &[u8]) -> bool {
    bytes.iter().fold(0u8, |acc, &b| acc.wrapping_add(b)) == 0
}

fn u32_at(bytes: &[u8], off: usize) -> Option<u32> {
    let raw = bytes.get(off..off.checked_add(4)?)?;
    Some(u32::from_le_bytes(raw.try_into().ok()?))
}

fn u64_at(bytes: &[u8], off: usize) -> Option<u64> {
    let raw = bytes.get(off..off.checked_add(8)?)?;
    Some(u64::from_le_bytes(raw.try_into().ok()?))
}

/// Copies a whole system description table and validates the copy.
fn read_table(phys: u64, signature: &[u8; 4]) -> Result<Box<[u8]>, AcpiError> {
    let header = copy(phys, SDT_HEADER_LEN)?;
    if header.get(0..4) != Some(&signature[..]) {
        return Err(AcpiError::BadSignature);
    }
    let len = u32_at(&header, 4).ok_or(AcpiError::BadLength)? as usize;
    if !(SDT_HEADER_LEN..=MAX_TABLE_LEN).contains(&len) {
        return Err(AcpiError::BadLength);
    }
    // Copy again at full length and re-check everything on this copy: the
    // header we looked at first is not trusted to still be the same.
    let table = copy(phys, len)?;
    if table.get(0..4) != Some(&signature[..]) || u32_at(&table, 4).map(|l| l as usize) != Some(len)
    {
        return Err(AcpiError::BadLength);
    }
    if !checksum_ok(&table) {
        return Err(AcpiError::BadChecksum);
    }
    Ok(table)
}

fn read_rsdt(rsdp: &[u8]) -> Result<Box<[u8]>, AcpiError> {
    let rsdt = u32_at(rsdp, 16)
        .filter(|&a| a != 0)
        .ok_or(AcpiError::NoMadt)?;
    read_table(u64::from(rsdt), b"RSDT")
}

/// Follows the RSDP to the MADT and validates every step.
pub fn discover(rsdp_phys: Option<u64>) -> Result<AcpiTables, AcpiError> {
    let rsdp_phys = rsdp_phys.ok_or(AcpiError::NoRsdp)?;

    // RSDP v1 is 20 bytes; v2+ is `length` bytes (36) with an extended
    // checksum and a 64-bit XSDT address. One copy of the largest version
    // is validated, so v1 and v2 fields come from the same read.
    let v1 = copy(rsdp_phys, 20)?;
    if v1.get(0..8) != Some(b"RSD PTR ") {
        return Err(AcpiError::BadSignature);
    }
    let revision = v1.get(15).copied().unwrap_or(0);
    let (root, entry_size) = if revision >= 2 {
        let len = u32_at(&copy(rsdp_phys, 36)?, 20).ok_or(AcpiError::BadLength)? as usize;
        if !(36..=4096).contains(&len) {
            return Err(AcpiError::BadLength);
        }
        let rsdp = copy(rsdp_phys, len)?;
        let first20 = rsdp.get(..20).ok_or(AcpiError::BadLength)?;
        if rsdp.get(0..8) != Some(b"RSD PTR ")
            || u32_at(&rsdp, 20).map(|l| l as usize) != Some(len)
            || !checksum_ok(first20)
            || !checksum_ok(&rsdp)
        {
            return Err(AcpiError::BadChecksum);
        }
        match u64_at(&rsdp, 24).filter(|&a| a != 0) {
            Some(xsdt) => (read_table(xsdt, b"XSDT")?, 8),
            None => (read_rsdt(&rsdp)?, 4),
        }
    } else {
        if !checksum_ok(&v1) {
            return Err(AcpiError::BadChecksum);
        }
        (read_rsdt(&v1)?, 4)
    };

    let entries = root.get(SDT_HEADER_LEN..).unwrap_or(&[]);
    for entry in entries.chunks_exact(entry_size).take(MAX_ROOT_ENTRIES) {
        let phys = if entry_size == 8 {
            u64_at(entry, 0)
        } else {
            u32_at(entry, 0).map(u64::from)
        };
        let Some(phys) = phys.filter(|&a| a != 0) else {
            continue;
        };
        // Peek at the signature only; skip tables that are not firmware
        // memory or cannot be copied.
        let Ok(header) = copy(phys, SDT_HEADER_LEN) else {
            continue;
        };
        if header.get(0..4) == Some(b"APIC") {
            let madt = read_table(phys, b"APIC")?;
            let io_apics = io_apic_addresses(&madt);
            return Ok(AcpiTables { madt, io_apics });
        }
    }
    Err(AcpiError::NoMadt)
}

/// Extracts the I/O APIC register addresses (MADT entry type 1) from a
/// validated MADT copy. A malformed entry ends the scan.
fn io_apic_addresses(madt: &[u8]) -> Vec<u64> {
    // Header (36) + local APIC address (4) + flags (4), then entries of
    // `[type: u8, length: u8, ...]`; type 1 = I/O APIC, address at +4.
    let mut found = Vec::new();
    let mut off = SDT_HEADER_LEN + 8;
    while let (Some(&kind), Some(&len)) = (madt.get(off), madt.get(off + 1)) {
        let len = usize::from(len);
        if len < 2 || off + len > madt.len() {
            break;
        }
        if kind == 1
            && len >= 12
            && found.len() < MAX_IO_APICS
            && let Some(addr) = u32_at(madt, off + 4)
            && !found.contains(&u64::from(addr))
        {
            found.push(u64::from(addr));
        }
        off += len;
    }
    found
}
