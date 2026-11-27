#![no_main]

use kitty_storage::{
    ReadAt, ReadError,
    gpt::{EFI_SYSTEM, Gpt},
};
use libfuzzer_sys::fuzz_target;

const SECTOR: usize = 512;
const SECTORS: usize = 16;

fn put32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn put64(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn replace(bytes: &mut [u8], input: &[u8]) {
    let count = bytes.len().min(input.len());
    bytes[..count].copy_from_slice(&input[..count]);
}

fn crc(bytes: &[u8]) -> u32 {
    !bytes.iter().fold(u32::MAX, |state, &byte| {
        (0..8).fold(state ^ u32::from(byte), |value, _| {
            (value >> 1) ^ (0xedb8_8320 * (value & 1))
        })
    })
}

fuzz_target!(|input: &[u8]| {
    let mut disk = [0; SECTOR * SECTORS];
    let (mbr, body) = disk.split_first_chunk_mut::<SECTOR>().expect("fixture MBR");
    let (header, body) = body
        .split_first_chunk_mut::<SECTOR>()
        .expect("fixture header");
    let (table, data) = body
        .split_first_chunk_mut::<SECTOR>()
        .expect("fixture table");
    mbr[450] = 0xee;
    put32(mbr, 454, 1);
    put32(mbr, 458, SECTORS as u32 - 1);
    mbr[510..].copy_from_slice(&[0x55, 0xaa]);
    header[..8].copy_from_slice(b"EFI PART");
    put32(header, 8, 0x0001_0000);
    put32(header, 12, 92);
    put64(header, 24, 1);
    put64(header, 32, SECTORS as u64 - 1);
    put64(header, 40, 3);
    put64(header, 48, SECTORS as u64 - 3);
    header[56] = 1;
    put64(header, 72, 2);
    put32(header, 80, 4);
    put32(header, 84, 128);
    table[..16].copy_from_slice(&EFI_SYSTEM);
    table[16] = 1;
    put64(table, 32, 4);
    put64(table, 40, 5);

    let (selector, payload) = input
        .split_first()
        .map_or((0, &[][..]), |(&byte, rest)| (byte, rest));
    match selector % 4 {
        0 => replace(&mut header[20..88], payload),
        1 => replace(table, payload),
        2 => replace(&mut table[16..], payload),
        _ => replace(data, payload),
    }
    // Checksums remain valid so mutations reach metadata validation.
    put32(header, 88, crc(table));
    let checksum = crc(&header[..92]);
    put32(header, 16, checksum);

    if let Ok(gpt) = Gpt::read(disk.as_slice(), SECTOR as u32) {
        for partition in gpt.partitions() {
            let view = gpt
                .partition(partition.number())
                .expect("listed partition has a view");
            let mut bytes = [0; 2];
            let length = view.byte_len();
            assert!(length >= SECTOR as u64);
            assert_eq!(view.read_exact_at(length - 1, &mut bytes), Err(ReadError));
            assert_eq!(view.read_exact_at(length, &mut []), Ok(()));
            assert_eq!(view.read_exact_at(length - 2, &mut bytes), Ok(()));
        }
    }
});
