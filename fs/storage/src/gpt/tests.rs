extern crate std;

use alloc::vec;
use std::string::String;

use super::*;

const SECTORS: u64 = 128;
const TABLE_ENTRIES: u32 = 33;
const TABLE_BYTES: usize = TABLE_ENTRIES as usize * MIN_ENTRY_BYTES;
const DISK_ID: Guid = [0x21; 16];
const BASIC_DATA: Guid = [0x42; 16];

struct Image {
    bytes: Vec<u8>,
    sector_bytes: usize,
}

impl Image {
    fn new(sector_bytes: usize) -> Self {
        let mut image = Self {
            bytes: vec![0; SECTORS as usize * sector_bytes],
            sector_bytes,
        };
        image.bytes[510..512].copy_from_slice(&[0x55, 0xaa]);
        image.bytes[MBR_ENTRY_OFFSET + 4] = 0xee;
        dword_at(&mut image.bytes, MBR_ENTRY_OFFSET + 8, 1);
        dword_at(
            &mut image.bytes,
            MBR_ENTRY_OFFSET + 12,
            (SECTORS - 1) as u32,
        );
        let first = image.first_usable();
        for copy in [HeaderCopy::Primary, HeaderCopy::Backup] {
            let lba = image.table_lba(copy);
            image.partition_at(
                lba,
                1,
                EFI_SYSTEM,
                [0x31; 16],
                (first + 2, first + 5),
                "Arranque 🐈",
            );
            image.partition_at(lba, 17, BASIC_DATA, [0x32; 16], (first, first + 1), "Dados");
            image.write_header(copy);
        }
        image
    }

    fn table_sectors(&self) -> u64 {
        TABLE_BYTES.div_ceil(self.sector_bytes) as u64
    }

    fn first_usable(&self) -> u64 {
        2 + self.table_sectors()
    }

    fn table_lba(&self, copy: HeaderCopy) -> u64 {
        match copy {
            HeaderCopy::Primary => 2,
            HeaderCopy::Backup => SECTORS - 1 - self.table_sectors(),
        }
    }

    fn sector_offset(&self, lba: u64) -> usize {
        lba as usize * self.sector_bytes
    }

    fn header_offset(&self, copy: HeaderCopy) -> usize {
        self.sector_offset(if copy == HeaderCopy::Primary {
            1
        } else {
            SECTORS - 1
        })
    }

    fn table_offset(&self, copy: HeaderCopy) -> usize {
        self.sector_offset(self.table_lba(copy))
    }

    fn partition_at(
        &mut self,
        table_lba: u64,
        number: usize,
        kind: Guid,
        id: Guid,
        range: (u64, u64),
        name: &str,
    ) {
        let start = self.sector_offset(table_lba) + (number - 1) * MIN_ENTRY_BYTES;
        let entry = &mut self.bytes[start..start + MIN_ENTRY_BYTES];
        entry[..16].copy_from_slice(&kind);
        entry[16..32].copy_from_slice(&id);
        qword_at(entry, 32, range.0);
        qword_at(entry, 40, range.1);
        qword_at(entry, 48, 0x8000_0000_0000_0001);
        for (offset, unit) in (56..128).step_by(2).zip(name.encode_utf16()) {
            entry[offset..offset + 2].copy_from_slice(&unit.to_le_bytes());
        }
    }

    fn write_header(&mut self, copy: HeaderCopy) {
        let entries = self.table_offset(copy);
        let table_crc = crc(&self.bytes[entries..entries + TABLE_BYTES]);
        let first_usable = self.first_usable();
        let last_usable = SECTORS - 2 - self.table_sectors();
        let entries_lba = self.table_lba(copy);
        let offset = self.header_offset(copy);
        let header = &mut self.bytes[offset..offset + MIN_HEADER_BYTES];
        header[..8].copy_from_slice(b"EFI PART");
        dword_at(header, 8, 0x0001_0000);
        dword_at(header, 12, MIN_HEADER_BYTES as u32);
        qword_at(
            header,
            24,
            if copy == HeaderCopy::Primary {
                1
            } else {
                SECTORS - 1
            },
        );
        qword_at(
            header,
            32,
            if copy == HeaderCopy::Primary {
                SECTORS - 1
            } else {
                1
            },
        );
        qword_at(header, 40, first_usable);
        qword_at(header, 48, last_usable);
        header[56..72].copy_from_slice(&DISK_ID);
        qword_at(header, 72, entries_lba);
        dword_at(header, 80, TABLE_ENTRIES);
        dword_at(header, 84, MIN_ENTRY_BYTES as u32);
        dword_at(header, 88, table_crc);
        update_header_crc(header);
    }

    fn read(&self) -> Result<Gpt<&[u8]>, Error> {
        Gpt::read(self.bytes.as_slice(), self.sector_bytes as u32)
    }
}

impl crate::ReadSectors for &Image {
    fn sector_bytes(&self) -> u32 {
        self.sector_bytes as u32
    }

    fn sector_count(&self) -> u64 {
        SECTORS
    }

    fn read_sectors(&self, first: u64, out: &mut [u8]) -> Result<(), ReadError> {
        assert!(out.len().is_multiple_of(self.sector_bytes));
        self.bytes
            .as_slice()
            .read_exact_at(first * self.sector_bytes as u64, out)
    }
}

fn crc(bytes: &[u8]) -> u32 {
    let mut checksum = Crc32::new();
    checksum.update(bytes);
    checksum.finish()
}

fn update_header_crc(header: &mut [u8]) {
    let length = dword(header, 12) as usize;
    dword_at(header, 16, 0);
    let checksum = crc(&header[..length]);
    dword_at(header, 16, checksum);
}

fn dword_at(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn qword_at(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

#[test]
fn matches_the_standard_crc_vector_across_multiple_updates() {
    let mut checksum = Crc32::new();
    checksum.update(b"1234");
    checksum.update(b"56789");
    assert_eq!(checksum.finish(), 0xcbf4_3926);
}

#[test]
fn reads_metadata_and_bounds_the_partition_view_to_its_inclusive_range() {
    for sector_bytes in [512, 4096] {
        let mut image = Image::new(sector_bytes);
        let start = image.sector_offset(image.first_usable() + 2);
        image.bytes[start..start + 3].copy_from_slice(b"cat");
        image.bytes[start - 1] = b'x';
        image.bytes[start + sector_bytes * 4] = b'y';
        let sectors = crate::SectorReader::new(&image).unwrap();
        let disk = Gpt::read(sectors, sector_bytes as u32).unwrap();
        assert_eq!(disk.disk_id(), DISK_ID);
        assert_eq!(disk.header_copy(), HeaderCopy::Primary);
        let [efi, data] = disk.partitions() else {
            panic!("wrong partition count")
        };
        assert_eq!((efi.number(), data.number()), (1, 17));
        assert_eq!(efi.partition_type(), EFI_SYSTEM);
        assert_eq!(efi.id(), [0x31; 16]);
        assert_eq!(efi.first_lba(), image.first_usable() + 2);
        assert_eq!(efi.last_lba(), image.first_usable() + 5);
        assert_eq!(efi.attributes(), 0x8000_0000_0000_0001);
        assert_eq!(String::from_utf16(efi.name_units()).unwrap(), "Arranque 🐈");

        let partition = disk.partition(1).unwrap();
        let length = (sector_bytes * 4) as u64;
        assert_eq!(partition.byte_len(), length);
        let mut bytes = [0; 3];
        partition.read_exact_at(0, &mut bytes).unwrap();
        assert_eq!(&bytes, b"cat");
        assert_eq!(
            partition.read_exact_at(length - 2, &mut bytes),
            Err(ReadError)
        );
        assert_eq!(partition.read_exact_at(length, &mut []), Ok(()));
        assert_eq!(partition.read_exact_at(length, &mut bytes), Err(ReadError));
        assert_eq!(
            partition.read_exact_at(u64::MAX, &mut bytes),
            Err(ReadError)
        );
        assert!(disk.partition(2).is_none());
    }
}

#[test]
fn recovers_from_either_bad_primary_checksum() {
    for header in [true, false] {
        let mut image = Image::new(512);
        let offset = if header {
            image.header_offset(HeaderCopy::Primary) + 16
        } else {
            image.table_offset(HeaderCopy::Primary) + 56
        };
        image.bytes[offset] ^= 1;
        let table = image.read().unwrap();
        assert_eq!(table.header_copy(), HeaderCopy::Backup);
        assert_eq!(table.partitions().len(), 2);
    }
}

#[test]
fn reports_the_primary_error_when_both_copies_have_bad_checksums() {
    let mut image = Image::new(512);
    for copy in [HeaderCopy::Primary, HeaderCopy::Backup] {
        let offset = image.table_offset(copy);
        image.bytes[offset + 56] ^= 1;
    }
    assert_eq!(image.read().err(), Some(Error::TableChecksum));
    let offset = image.header_offset(HeaderCopy::Primary);
    image.bytes[offset + 16] ^= 1;
    assert_eq!(image.read().err(), Some(Error::HeaderChecksum));
}

#[test]
fn ignores_sector_padding_in_the_table_checksum_and_prefers_primary() {
    let mut image = Image::new(512);
    let offset = image.table_offset(HeaderCopy::Primary);
    image.bytes[offset + TABLE_BYTES] = 0xa5;
    let offset = image.header_offset(HeaderCopy::Backup);
    image.bytes[offset] = 0x55;
    assert_eq!(image.read().unwrap().header_copy(), HeaderCopy::Primary);
}

#[test]
fn rejects_overlaps_duplicate_guids_and_out_of_range_entries() {
    for expected in [
        Error::OverlappingPartitions,
        Error::DuplicatePartitionGuid,
        Error::InvalidPartition,
    ] {
        let mut image = Image::new(512);
        let first = image.first_usable();
        for copy in [HeaderCopy::Primary, HeaderCopy::Backup] {
            let lba = image.table_lba(copy);
            let (id, bounds) = match expected {
                Error::OverlappingPartitions => ([0x35; 16], (first + 1, first + 2)),
                Error::DuplicatePartitionGuid => ([0x31; 16], (first, first + 1)),
                _ => ([0x36; 16], (first - 1, first)),
            };
            image.partition_at(lba, 17, BASIC_DATA, id, bounds, "Dados");
            image.write_header(copy);
        }
        assert_eq!(image.read().err(), Some(expected));
    }
}

#[test]
fn rejects_arithmetic_and_metadata_range_violations_with_valid_checksums() {
    for (field, value, expected) in [
        (24, 2, Error::InvalidHeader),
        (32, 2, Error::InvalidHeader),
        (40, 2, Error::InvalidTableGeometry),
        (48, SECTORS - 2, Error::InvalidTableGeometry),
        (72, u64::MAX, Error::InvalidTableGeometry),
        (72, 1, Error::InvalidTableGeometry),
    ] {
        let mut image = Image::new(512);
        let backup = image.header_offset(HeaderCopy::Backup);
        image.bytes[backup] = 0;
        let primary = image.header_offset(HeaderCopy::Primary);
        let header = &mut image.bytes[primary..primary + 512];
        qword_at(header, field, value);
        update_header_crc(header);
        assert_eq!(image.read().err(), Some(expected), "field {field}");
    }
    for (field, value) in [
        (80, 0),
        (80, MAX_ENTRY_COUNT + 1),
        (84, 64),
        (84, 384),
        (84, 8192),
    ] {
        let mut image = Image::new(512);
        let backup = image.header_offset(HeaderCopy::Backup);
        image.bytes[backup] = 0;
        let primary = image.header_offset(HeaderCopy::Primary);
        let header = &mut image.bytes[primary..primary + 512];
        dword_at(header, field, value);
        update_header_crc(header);
        assert_eq!(image.read().err(), Some(Error::InvalidTableGeometry));
    }
}

#[test]
fn rejects_missing_hybrid_and_inconsistent_protective_mbrs() {
    for (offset, value) in [
        (510, 0),
        (MBR_ENTRY_OFFSET + 4, 0),
        (MBR_ENTRY_OFFSET + 20, 0x83),
        (MBR_ENTRY_OFFSET + 12, 13),
    ] {
        let mut image = Image::new(512);
        image.bytes[offset] = value;
        assert_eq!(image.read().err(), Some(Error::InvalidProtectiveMbr));
    }
    let image = Image::new(512);
    for sector in [0, 128, 513] {
        assert_eq!(
            Gpt::read(image.bytes.as_slice(), sector).err(),
            Some(Error::InvalidDiskGeometry)
        );
    }
    assert_eq!(
        Gpt::read(&image.bytes[..image.bytes.len() - 1], 512).err(),
        Some(Error::InvalidDiskGeometry)
    );
}

#[test]
fn uses_the_backup_if_the_primary_header_cannot_be_read() {
    struct DiskWithReadError<'a>(&'a [u8]);

    impl ReadAt for DiskWithReadError<'_> {
        fn byte_len(&self) -> u64 {
            self.0.byte_len()
        }

        fn read_exact_at(&self, offset: u64, out: &mut [u8]) -> Result<(), ReadError> {
            if offset == 512 {
                return Err(ReadError);
            }
            self.0.read_exact_at(offset, out)
        }
    }

    let image = Image::new(512);
    let disk = Gpt::read(DiskWithReadError(image.bytes.as_slice()), 512).unwrap();
    assert_eq!(disk.header_copy(), HeaderCopy::Backup);
    assert_eq!(disk.partitions().len(), 2);
}
