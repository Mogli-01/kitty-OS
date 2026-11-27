#![no_main]

use kitty_acpi::madt::{Error, IsaRoute, Madt, Polarity, TriggerMode};
use libfuzzer_sys::fuzz_target;

const HEADER_LEN: usize = 44;

fuzz_target!(|input: &[u8]| {
    exercise(input);

    // Also exercise records behind the table checksum and size checks.
    if input.len() <= 64 * 1024 - HEADER_LEN {
        exercise(&table_with_records(input));
    }
    if let Some((control, ids)) = input.split_first_chunk::<2>()
        && let Some((first, remaining)) = ids.split_first_chunk::<4>()
        && let Some(second) = remaining.first_chunk::<4>()
    {
        exercise_isa_routes(
            *control,
            [u32::from_le_bytes(*first), u32::from_le_bytes(*second)],
        );
    }
});

fn table_with_records(records: &[u8]) -> Vec<u8> {
    let mut table = vec![0; HEADER_LEN];
    table[..4].copy_from_slice(b"APIC");
    table[4..8].copy_from_slice(&((HEADER_LEN + records.len()) as u32).to_le_bytes());
    table[8] = 6;
    table[36..40].copy_from_slice(&0xfee0_0000u32.to_le_bytes());
    table.extend_from_slice(records);
    table[9] = 0u8.wrapping_sub(table.iter().copied().fold(0u8, u8::wrapping_add));
    table
}

fn exercise_isa_routes([selector, distance]: [u8; 2], gsi: [u32; 2]) {
    let first = selector % 16;
    let second = (first + 1 + distance % 15) % 16;

    let mut records = [0; 20];
    records[..4].copy_from_slice(&[2, 10, 0, first]);
    records[4..8].copy_from_slice(&gsi[0].to_le_bytes());
    records[10..14].copy_from_slice(&[2, 10, 0, second]);
    records[14..18].copy_from_slice(&gsi[1].to_le_bytes());
    records[18] = 0x0f;
    if selector & 0x80 != 0 {
        records.rotate_left(10);
    }

    let table = table_with_records(&records);
    let madt = Madt::parse(&table).expect("generated ISA overrides must be valid");
    let mut expected = [None; 16];
    expected[usize::from(first)] = Some(IsaRoute {
        gsi: gsi[0],
        polarity: Polarity::ActiveHigh,
        trigger: TriggerMode::Edge,
    });
    expected[usize::from(second)] = Some(IsaRoute {
        gsi: gsi[1],
        polarity: Polarity::ActiveLow,
        trigger: TriggerMode::Level,
    });
    assert!((0..16).map(|irq| madt.isa_override(irq)).eq(expected));
}

fn exercise(input: &[u8]) {
    if let Ok(madt) = Madt::parse(input) {
        assert_ne!(madt.local_apic_address(), 0);
        assert!(madt.local_apic_address().is_multiple_of(4096));
        let mut ids = [false; 256];
        for io_apic in madt.io_apics() {
            assert!(!core::mem::replace(&mut ids[usize::from(io_apic.id)], true));
            assert_ne!(io_apic.address, 0);
            assert!(io_apic.address.is_multiple_of(4096));
        }
        assert!(madt.isa_override(16).is_none());
        assert_eq!(madt.local_nmis(0xff), Err(Error::MissingLocalProcessor));
        for id in [0, input.last().copied().unwrap_or(0)] {
            if let Err(Error::ConflictingLocalNmi { lint }) = madt.local_nmis(id) {
                assert!(lint <= 1);
            }
        }
    }
}
