#![no_main]

use kitty_acpi::madt::{Error, Madt};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|input: &[u8]| {
    exercise(input);

    // Also exercise records behind the table checksum and size checks.
    const HEADER_LEN: usize = 44;
    if input.len() <= 64 * 1024 - HEADER_LEN {
        let mut table = vec![0; HEADER_LEN];
        table[..4].copy_from_slice(b"APIC");
        table[4..8].copy_from_slice(&((HEADER_LEN + input.len()) as u32).to_le_bytes());
        table[8] = 6;
        table[36..40].copy_from_slice(&0xfee0_0000u32.to_le_bytes());
        table.extend_from_slice(input);
        table[9] = 0u8.wrapping_sub(table.iter().copied().fold(0u8, u8::wrapping_add));
        exercise(&table);
    }
});

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
