//! xAPIC addresses and ISA interrupt overrides from an ACPI MADT.
//!
//! See ACPI §5.2.12. The parser treats physical addresses as numbers.
//! The platform must establish the bounds and ownership of MMIO separately.

const HEADER_LEN: usize = 44;
const MAX_TABLE_LEN: usize = 64 * 1024;
const PC_AT_COMPAT: u32 = 1;
const ISA_LINES: usize = 16;
const APIC_PAGE_SIZE: u64 = 4096;
const LOCAL_APIC_TYPE: u8 = 0;
const IO_APIC_TYPE: u8 = 1;
const ISA_OVERRIDE_TYPE: u8 = 2;
const IO_APIC_NMI_TYPE: u8 = 3;
const LOCAL_NMI_TYPE: u8 = 4;
const LOCAL_ADDRESS_OVERRIDE_TYPE: u8 = 5;
const X2APIC_NMI_TYPE: u8 = 10;
const PROCESSOR_ENABLED: u32 = 1;
const PROCESSOR_ONLINE_CAPABLE: u32 = 1 << 1;
const ALL_PROCESSORS: u8 = 0xff;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Truncated,
    BadSignature,
    BadTableLength,
    BadChecksum,
    BadRecordLength,
    BadRecord { kind: u8 },
    BadAddress,
    BadFlags,
    BadIsaSource,
    BadLocalApicId,
    DuplicateIoApicId,
    DuplicateIsaSource,
    DuplicateLocalProcessor,
    DuplicateLocalAddressOverride,
    MissingLocalProcessor,
    ConflictingLocalNmi { lint: u8 },
    UnsupportedNmiSource,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IoApic {
    pub id: u8,
    pub address: u32,
    pub gsi_base: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Polarity {
    ActiveHigh,
    ActiveLow,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TriggerMode {
    Edge,
    Level,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IsaRoute {
    pub gsi: u32,
    pub polarity: Polarity,
    pub trigger: TriggerMode,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalNmi {
    pub polarity: Polarity,
    pub trigger: TriggerMode,
}

/// An enabled processor from a Local APIC record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalProcessor {
    pub apic_id: u8,
    pub acpi_uid: u8,
}

/// A validated view of the MADT records used for xAPIC discovery.
#[derive(Debug)]
pub struct Madt<'a> {
    records: &'a [u8],
    local_apic_address: u64,
    pc_at_compat: bool,
    overrides: [Option<IsaRoute>; ISA_LINES],
}

impl<'a> Madt<'a> {
    /// Validates the table of at most 64 KiB. Bytes after its declared length
    /// can be part of the caller's surrounding firmware buffer.
    pub fn parse(bytes: &'a [u8]) -> Result<Self, Error> {
        let header = bytes.first_chunk::<HEADER_LEN>().ok_or(Error::Truncated)?;
        if &header[..4] != b"APIC" {
            return Err(Error::BadSignature);
        }
        let length = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
        let length = usize::try_from(length).map_err(|_| Error::BadTableLength)?;
        if !(HEADER_LEN..=MAX_TABLE_LEN).contains(&length) {
            return Err(Error::BadTableLength);
        }
        let table = bytes.get(..length).ok_or(Error::Truncated)?;
        if table.iter().copied().fold(0u8, u8::wrapping_add) != 0 {
            return Err(Error::BadChecksum);
        }

        let local_address = u32::from_le_bytes([header[36], header[37], header[38], header[39]]);
        let flags = u32::from_le_bytes([header[40], header[41], header[42], header[43]]);
        if flags & !PC_AT_COMPAT != 0 {
            return Err(Error::BadFlags);
        }
        let entries = &table[HEADER_LEN..];
        let mut local_override = None;
        let mut overrides = [None; ISA_LINES];
        let mut io_apic_ids = [false; 256];
        let mut local_apic_ids = [false; 256];
        let mut processor_uids = [false; 256];
        for entry in records(entries) {
            match entry? {
                Record::LocalApic {
                    id,
                    uid,
                    enabled: true,
                } => {
                    let id_seen = &mut local_apic_ids[usize::from(id)];
                    let uid_seen = &mut processor_uids[usize::from(uid)];
                    if *id_seen || *uid_seen {
                        return Err(Error::DuplicateLocalProcessor);
                    }
                    *id_seen = true;
                    *uid_seen = true;
                }
                Record::IoApic(apic) => {
                    let seen = &mut io_apic_ids[usize::from(apic.id)];
                    if core::mem::replace(seen, true) {
                        return Err(Error::DuplicateIoApicId);
                    }
                }
                Record::IsaOverride { irq, route } => {
                    let slot = overrides
                        .get_mut(usize::from(irq))
                        .ok_or(Error::BadIsaSource)?;
                    if slot.replace(route).is_some() {
                        return Err(Error::DuplicateIsaSource);
                    }
                }
                Record::LocalAddressOverride(address) => {
                    if local_override.replace(address).is_some() {
                        return Err(Error::DuplicateLocalAddressOverride);
                    }
                }
                Record::LocalApic { enabled: false, .. }
                | Record::LocalNmi { .. }
                | Record::UnsupportedNmi
                | Record::Other => {}
            }
        }
        let local_apic_address = local_override.unwrap_or(u64::from(local_address));
        if !is_apic_address(local_apic_address) {
            return Err(Error::BadAddress);
        }
        Ok(Self {
            records: entries,
            local_apic_address,
            pc_at_compat: flags & PC_AT_COMPAT != 0,
            overrides,
        })
    }

    /// Effective physical address, using the 64-bit override if present.
    pub const fn local_apic_address(&self) -> u64 {
        self.local_apic_address
    }

    /// The platform reports a PC-AT dual 8259 PIC.
    pub const fn has_legacy_pic(&self) -> bool {
        self.pc_at_compat
    }

    /// Enabled Local APIC (type 0) records in firmware order.
    pub fn local_processors(&self) -> impl Iterator<Item = LocalProcessor> + '_ {
        records(self.records).filter_map(|record| match record {
            Ok(Record::LocalApic {
                id,
                uid,
                enabled: true,
            }) => Some(LocalProcessor {
                apic_id: id,
                acpi_uid: uid,
            }),
            _ => None,
        })
    }

    /// I/O APIC records in firmware order. The controller must probe the
    /// number of inputs before it can validate GSI ranges.
    pub fn io_apics(&self) -> impl Iterator<Item = IoApic> + '_ {
        records(self.records).filter_map(|record| match record {
            Ok(Record::IoApic(apic)) => Some(apic),
            _ => None,
        })
    }

    /// A firmware override of an ISA line. Conforming ISA flags decode to
    /// active high, edge triggered. Invalid ISA line numbers yield `None`.
    pub fn isa_override(&self, irq: u8) -> Option<IsaRoute> {
        self.overrides.get(usize::from(irq)).copied().flatten()
    }

    /// Resolves the NMI settings for an enabled xAPIC ID, in LINT0, LINT1
    /// order. An I/O APIC or x2APIC NMI needs a platform policy before the
    /// interrupt controller can be replaced.
    pub fn local_nmis(&self, apic_id: u8) -> Result<[Option<LocalNmi>; 2], Error> {
        let uid = self
            .local_processors()
            .find(|processor| processor.apic_id == apic_id)
            .map(|processor| processor.acpi_uid)
            .ok_or(Error::MissingLocalProcessor)?;

        let mut pins = [None; 2];
        for entry in records(self.records) {
            match entry? {
                Record::LocalNmi {
                    uid: target,
                    lint,
                    config,
                } if target == ALL_PROCESSORS || target == uid => {
                    let slot = &mut pins[usize::from(lint)];
                    if slot.is_some_and(|old| old != config) {
                        return Err(Error::ConflictingLocalNmi { lint });
                    }
                    *slot = Some(config);
                }
                Record::UnsupportedNmi => return Err(Error::UnsupportedNmiSource),
                _ => {}
            }
        }
        Ok(pins)
    }
}

enum Record {
    LocalApic { id: u8, uid: u8, enabled: bool },
    IoApic(IoApic),
    IsaOverride { irq: u8, route: IsaRoute },
    LocalNmi { uid: u8, lint: u8, config: LocalNmi },
    LocalAddressOverride(u64),
    UnsupportedNmi,
    Other,
}

fn records(mut bytes: &[u8]) -> impl Iterator<Item = Result<Record, Error>> + '_ {
    core::iter::from_fn(move || {
        if bytes.is_empty() {
            return None;
        }
        if let [kind, len, rest @ ..] = bytes
            && let Some(payload_len) = len.checked_sub(2)
            && let Some((payload, tail)) = rest.split_at_checked(usize::from(payload_len))
        {
            bytes = tail;
            return Some(decode_record(*kind, payload));
        }
        bytes = &[];
        Some(Err(Error::BadRecordLength))
    })
}

fn decode_record(kind: u8, payload: &[u8]) -> Result<Record, Error> {
    match (kind, payload) {
        (LOCAL_APIC_TYPE, [uid, id, a, b, c, d]) => {
            let flags = u32::from_le_bytes([*a, *b, *c, *d]);
            if !matches!(flags, 0 | PROCESSOR_ENABLED | PROCESSOR_ONLINE_CAPABLE) {
                return Err(Error::BadFlags);
            }
            let enabled = flags == PROCESSOR_ENABLED;
            if enabled && *id == 0xff {
                return Err(Error::BadLocalApicId);
            }
            Ok(Record::LocalApic {
                id: *id,
                uid: *uid,
                enabled,
            })
        }
        (IO_APIC_TYPE, [id, 0, a, b, c, d, e, f, g, h]) => {
            let address = u32::from_le_bytes([*a, *b, *c, *d]);
            if !is_apic_address(u64::from(address)) {
                return Err(Error::BadAddress);
            }
            Ok(Record::IoApic(IoApic {
                id: *id,
                address,
                gsi_base: u32::from_le_bytes([*e, *f, *g, *h]),
            }))
        }
        (ISA_OVERRIDE_TYPE, [0, irq, a, b, c, d, low, high]) => {
            let gsi = u32::from_le_bytes([*a, *b, *c, *d]);
            let (polarity, trigger) = decode_inti_flags(u16::from_le_bytes([*low, *high]))?;
            Ok(Record::IsaOverride {
                irq: *irq,
                route: IsaRoute {
                    gsi,
                    polarity,
                    trigger,
                },
            })
        }
        (LOCAL_NMI_TYPE, [uid, low, high, lint @ (0 | 1)]) => {
            let (polarity, trigger) = decode_inti_flags(u16::from_le_bytes([*low, *high]))?;
            Ok(Record::LocalNmi {
                uid: *uid,
                lint: *lint,
                config: LocalNmi { polarity, trigger },
            })
        }
        (LOCAL_ADDRESS_OVERRIDE_TYPE, [0, 0, a, b, c, d, e, f, g, h]) => {
            Ok(Record::LocalAddressOverride(u64::from_le_bytes([
                *a, *b, *c, *d, *e, *f, *g, *h,
            ])))
        }
        (IO_APIC_NMI_TYPE | X2APIC_NMI_TYPE, _) => Ok(Record::UnsupportedNmi),
        (
            LOCAL_APIC_TYPE
            | IO_APIC_TYPE
            | ISA_OVERRIDE_TYPE
            | LOCAL_NMI_TYPE
            | LOCAL_ADDRESS_OVERRIDE_TYPE,
            _,
        ) => Err(Error::BadRecord { kind }),
        _ => Ok(Record::Other),
    }
}

fn is_apic_address(address: u64) -> bool {
    address != 0 && address.is_multiple_of(APIC_PAGE_SIZE) && address <= u64::MAX - APIC_PAGE_SIZE
}

fn decode_inti_flags(flags: u16) -> Result<(Polarity, TriggerMode), Error> {
    // Bits 0–1 are polarity, 2–3 trigger mode. Encoding 2 is reserved in
    // each field. Use active-high, edge-triggered defaults for conforming
    // ISA interrupts and xAPIC LINT NMIs.
    if flags & !0x0f != 0 {
        return Err(Error::BadFlags);
    }
    let polarity = match flags & 3 {
        0 | 1 => Polarity::ActiveHigh,
        3 => Polarity::ActiveLow,
        _ => return Err(Error::BadFlags),
    };
    let trigger = match (flags >> 2) & 3 {
        0 | 1 => TriggerMode::Edge,
        3 => TriggerMode::Level,
        _ => return Err(Error::BadFlags),
    };
    Ok((polarity, trigger))
}

#[cfg(test)]
mod tests;
