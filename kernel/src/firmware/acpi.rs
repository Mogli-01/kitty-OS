//! ACPI table discovery: RSDP → RSDT/XSDT → MADT (docs/DECISIONS.md D-041).
//!
//! The kernel only does what must be trusted: find the tables, check that
//! they are internally consistent, and **copy** the MADT into kernel memory
//! so the driver parses a stable, immutable buffer (no time-of-check /
//! time-of-use window on firmware memory). Decoding the MADT is the job of
//! the Codex driver (`kitty-acpi`). The kernel additionally extracts the
//! I/O APIC register addresses, because it only grants MMIO capabilities for
//! addresses the validated MADT declares (`arch::apic`).

use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::mm::vmm::map_firmware_ro;

/// Size of the common ACPI table header.
const SDT_HEADER_LEN: usize = 36;
/// Largest table we accept (same limit as the Codex MADT parser).
pub const MAX_TABLE_LEN: usize = 64 * 1024;
/// Most I/O APICs we track.
const MAX_IO_APICS: usize = 16;

/// Why ACPI discovery stopped. Any error means "no APIC topology": the
/// kernel keeps the 8259 PIC.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AcpiError {
    /// The bootloader did not report an RSDP.
    NoRsdp,
    /// A firmware range could not be mapped (overlaps kernel RAM, too big…).
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
    /// Validated copy of the MADT (signature, length and checksum checked).
    pub madt: Box<[u8]>,
    /// I/O APIC register base addresses declared by the MADT.
    pub io_apics: Vec<u64>,
}

fn read_bytes(phys: u64, len: usize) -> Result<&'static [u8], AcpiError> {
    map_firmware_ro(phys, len as u64).map_err(|_| AcpiError::Unmappable)
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

/// Maps a whole system description table and validates its header.
fn read_table(phys: u64, signature: &[u8; 4]) -> Result<&'static [u8], AcpiError> {
    let header = read_bytes(phys, SDT_HEADER_LEN)?;
    if header.get(0..4) != Some(&signature[..]) {
        return Err(AcpiError::BadSignature);
    }
    let len = u32_at(header, 4).ok_or(AcpiError::BadLength)? as usize;
    if !(SDT_HEADER_LEN..=MAX_TABLE_LEN).contains(&len) {
        return Err(AcpiError::BadLength);
    }
    let table = read_bytes(phys, len)?;
    if !checksum_ok(table) {
        return Err(AcpiError::BadChecksum);
    }
    Ok(table)
}

fn read_rsdt(rsdp: &[u8]) -> Result<&'static [u8], AcpiError> {
    let rsdt = u32_at(rsdp, 16)
        .filter(|&a| a != 0)
        .ok_or(AcpiError::NoMadt)?;
    read_table(u64::from(rsdt), b"RSDT")
}

/// Follows the RSDP to the MADT and validates every step.
pub fn discover(rsdp_phys: Option<u64>) -> Result<AcpiTables, AcpiError> {
    let rsdp_phys = rsdp_phys.ok_or(AcpiError::NoRsdp)?;

    // RSDP v1 is 20 bytes; v2+ is `length` bytes (36) with an extended
    // checksum and a 64-bit XSDT address.
    let v1 = read_bytes(rsdp_phys, 20)?;
    if v1.get(0..8) != Some(b"RSD PTR ") {
        return Err(AcpiError::BadSignature);
    }
    if !checksum_ok(v1) {
        return Err(AcpiError::BadChecksum);
    }
    let revision = v1.get(15).copied().unwrap_or(0);
    let (root, entry_size) = if revision >= 2 {
        let v2 = read_bytes(rsdp_phys, 36)?;
        let len = u32_at(v2, 20).ok_or(AcpiError::BadLength)? as usize;
        if !(36..=4096).contains(&len) {
            return Err(AcpiError::BadLength);
        }
        if !checksum_ok(read_bytes(rsdp_phys, len)?) {
            return Err(AcpiError::BadChecksum);
        }
        match u64_at(v2, 24).filter(|&a| a != 0) {
            Some(xsdt) => (read_table(xsdt, b"XSDT")?, 8),
            None => (read_rsdt(v1)?, 4),
        }
    } else {
        (read_rsdt(v1)?, 4)
    };

    let entries = root.get(SDT_HEADER_LEN..).unwrap_or(&[]);
    for entry in entries.chunks_exact(entry_size) {
        let phys = if entry_size == 8 {
            u64_at(entry, 0)
        } else {
            u32_at(entry, 0).map(u64::from)
        };
        let Some(phys) = phys.filter(|&a| a != 0) else {
            continue;
        };
        // Peek at the signature only; skip tables we cannot map.
        let Ok(header) = read_bytes(phys, SDT_HEADER_LEN) else {
            continue;
        };
        if header.get(0..4) == Some(b"APIC") {
            let madt = read_table(phys, b"APIC")?;
            let copy: Box<[u8]> = madt.into();
            let io_apics = io_apic_addresses(&copy);
            return Ok(AcpiTables {
                madt: copy,
                io_apics,
            });
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
