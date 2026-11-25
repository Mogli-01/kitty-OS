extern crate std;

use super::*;
use std::vec::Vec;

fn table(records: &[u8]) -> Vec<u8> {
    let mut table = [0; HEADER_LEN].to_vec();
    table[..4].copy_from_slice(b"APIC");
    table[4..8].copy_from_slice(&((HEADER_LEN + records.len()) as u32).to_le_bytes());
    table[8] = 6;
    table[36..40].copy_from_slice(&0xfee0_0000u32.to_le_bytes());
    table[40] = 1;
    table.extend_from_slice(records);
    checksum(&mut table);
    table
}

fn checksum(table: &mut [u8]) {
    table[9] = 0;
    table[9] = 0u8.wrapping_sub(table.iter().copied().fold(0u8, u8::wrapping_add));
}

const IO_APIC0: [u8; 12] = [1, 12, 0, 0, 0, 0, 0xc0, 0xfe, 0, 0, 0, 0];
const IO_APIC1: [u8; 12] = [1, 12, 1, 0, 0, 0x10, 0xc0, 0xfe, 24, 0, 0, 0];
const TIMER_OVERRIDE: [u8; 10] = [2, 10, 0, 0, 2, 0, 0, 0, 0, 0];
const SCI_OVERRIDE: [u8; 10] = [2, 10, 0, 9, 9, 0, 0, 0, 0xf, 0];
const LAPIC_OVERRIDE: [u8; 12] = [5, 12, 0, 0, 0, 0, 0xe0, 0xfe, 1, 0, 0, 0];

#[test]
fn reads_multiple_io_apics_and_isa_overrides() {
    let mut records = IO_APIC0.to_vec();
    records.extend_from_slice(&TIMER_OVERRIDE);
    // Unused processor and firmware-specific records can appear anywhere.
    records.extend_from_slice(&[0, 8, 0, 0, 1, 0, 0, 0, 0x80, 3, 0]);
    records.extend_from_slice(&IO_APIC1);
    records.extend_from_slice(&SCI_OVERRIDE);
    let mut bytes = table(&records);
    bytes.extend_from_slice(&[0xff; 7]);
    let madt = Madt::parse(&bytes).unwrap();
    assert!(madt.has_legacy_pic());
    assert_eq!(madt.local_apic_address(), 0xfee0_0000);
    assert_eq!(
        madt.io_apics().collect::<Vec<_>>(),
        [
            IoApic {
                id: 0,
                address: 0xfec0_0000,
                gsi_base: 0,
            },
            IoApic {
                id: 1,
                address: 0xfec0_1000,
                gsi_base: 24,
            },
        ]
    );
    assert_eq!(
        madt.isa_override(0),
        Some(IsaRoute {
            gsi: 2,
            polarity: Polarity::ActiveHigh,
            trigger: TriggerMode::Edge,
        })
    );
    assert_eq!(
        madt.isa_override(9),
        Some(IsaRoute {
            gsi: 9,
            polarity: Polarity::ActiveLow,
            trigger: TriggerMode::Level,
        })
    );
    assert_eq!(madt.isa_override(1), None);
    assert_eq!(madt.isa_override(u8::MAX), None);
}

#[test]
fn replaces_the_header_address_with_the_64_bit_override() {
    let mut bytes = table(&LAPIC_OVERRIDE);
    bytes[36..44].fill(0);
    checksum(&mut bytes);
    let madt = Madt::parse(&bytes).unwrap();
    assert!(!madt.has_legacy_pic());
    assert_eq!(madt.local_apic_address(), 0x1_fee0_0000);
}

#[test]
fn checks_signature_declared_length_and_checksum() {
    let good = table(&IO_APIC0);
    for length in 0..good.len() {
        assert_eq!(Madt::parse(&good[..length]).unwrap_err(), Error::Truncated);
    }
    let mut bytes = good.clone();
    bytes[0] = 0;
    assert_eq!(Madt::parse(&bytes).unwrap_err(), Error::BadSignature);

    let mut bytes = good.clone();
    bytes[9] = bytes[9].wrapping_add(1);
    assert_eq!(Madt::parse(&bytes).unwrap_err(), Error::BadChecksum);

    let mut bytes = good.clone();
    bytes[40] = 2;
    checksum(&mut bytes);
    assert_eq!(Madt::parse(&bytes).unwrap_err(), Error::BadFlags);

    let mut bytes = good;
    for length in [
        0,
        (HEADER_LEN - 1) as u32,
        MAX_TABLE_LEN as u32 + 1,
        u32::MAX,
    ] {
        bytes[4..8].copy_from_slice(&length.to_le_bytes());
        assert_eq!(Madt::parse(&bytes).unwrap_err(), Error::BadTableLength);
    }
}

#[test]
fn rejects_records_that_cannot_be_walked() {
    for payload in [&[1][..], &[1, 0], &[2, 1], &[0x80, 3]] {
        assert_eq!(
            Madt::parse(&table(payload)).unwrap_err(),
            Error::BadRecordLength
        );
    }
    for kind in [IO_APIC_TYPE, ISA_OVERRIDE_TYPE, LOCAL_ADDRESS_OVERRIDE_TYPE] {
        assert_eq!(
            Madt::parse(&table(&[kind, 2])).unwrap_err(),
            Error::BadRecord { kind }
        );
    }
}

#[test]
fn rejects_ambiguous_definitions() {
    assert_eq!(
        Madt::parse(&table(&IO_APIC0.repeat(2))).unwrap_err(),
        Error::DuplicateIoApicId
    );
    assert_eq!(
        Madt::parse(&table(&TIMER_OVERRIDE.repeat(2))).unwrap_err(),
        Error::DuplicateIsaSource
    );
    assert_eq!(
        Madt::parse(&table(&LAPIC_OVERRIDE.repeat(2))).unwrap_err(),
        Error::DuplicateLocalAddressOverride
    );
}

#[test]
fn rejects_invalid_address_shapes() {
    let mut io_apic = IO_APIC0;
    io_apic[4..8].fill(0);
    assert_eq!(
        Madt::parse(&table(&io_apic)).unwrap_err(),
        Error::BadAddress
    );
    io_apic[4] = 1;
    assert_eq!(
        Madt::parse(&table(&io_apic)).unwrap_err(),
        Error::BadAddress
    );

    let mut local = LAPIC_OVERRIDE;
    local[4..].copy_from_slice(&0xffff_ffff_ffff_f000u64.to_le_bytes());
    assert_eq!(Madt::parse(&table(&local)).unwrap_err(), Error::BadAddress);
}

#[test]
fn checks_the_isa_bus_source_and_flags() {
    let mut record = TIMER_OVERRIDE;
    record[2] = 1;
    assert_eq!(
        Madt::parse(&table(&record)).unwrap_err(),
        Error::BadRecord {
            kind: ISA_OVERRIDE_TYPE
        }
    );
    record[2] = 0;
    record[3] = 16;
    assert_eq!(
        Madt::parse(&table(&record)).unwrap_err(),
        Error::BadIsaSource
    );

    record[3] = 0;
    for flags in [2u16, 8, 0x10, 0x100] {
        record[8..10].copy_from_slice(&flags.to_le_bytes());
        assert_eq!(Madt::parse(&table(&record)).unwrap_err(), Error::BadFlags);
    }
}
