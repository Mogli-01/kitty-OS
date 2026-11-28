extern crate std;

use std::{collections::BTreeMap, string::String, vec::Vec};

use super::*;
use crate::name::checksum;

const SECTOR: usize = 512;
const CLUSTERS: u32 = 65_525;
const FAT_SECTORS: u32 = 512;
const RESERVED: u32 = 32;
const DATA_SECTOR: u32 = RESERVED + 2 * FAT_SECTORS;
const EOC: u32 = 0x0fff_ffff;

struct Disk {
    sectors: BTreeMap<u64, [u8; SECTOR]>,
    bytes: u64,
    fail_at: Option<u64>,
}

impl Disk {
    fn new() -> Self {
        let total_sectors = DATA_SECTOR + CLUSTERS;
        let mut boot = [0; SECTOR];
        boot[11..13].copy_from_slice(&512u16.to_le_bytes());
        boot[13] = 1;
        boot[14..16].copy_from_slice(&(RESERVED as u16).to_le_bytes());
        boot[16] = 2;
        boot[32..36].copy_from_slice(&total_sectors.to_le_bytes());
        boot[36..40].copy_from_slice(&FAT_SECTORS.to_le_bytes());
        boot[44..48].copy_from_slice(&2u32.to_le_bytes());
        boot[510..].copy_from_slice(&[0x55, 0xaa]);
        let mut disk = Self {
            sectors: BTreeMap::from([(0, boot)]),
            bytes: u64::from(total_sectors) * SECTOR as u64,
            fail_at: None,
        };
        disk.link(2, EOC);
        disk
    }

    fn sector(&mut self, sector: u64) -> &mut [u8; SECTOR] {
        self.sectors.entry(sector).or_insert([0; SECTOR])
    }

    fn boot(&mut self) -> &mut [u8; SECTOR] {
        self.sector(0)
    }

    fn cluster(&mut self, cluster: u32) -> &mut [u8; SECTOR] {
        self.sector(u64::from(DATA_SECTOR + cluster - 2))
    }

    fn link(&mut self, cluster: u32, value: u32) {
        self.link_in(0, cluster, value);
    }

    fn link_in(&mut self, fat: u32, cluster: u32, value: u32) {
        let byte = cluster as usize * 4;
        let sector = u64::from(RESERVED + fat * FAT_SECTORS) + (byte / SECTOR) as u64;
        let offset = byte % SECTOR;
        self.sector(sector)[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn entry(&mut self, cluster: u32, index: usize, entry: [u8; 32]) {
        let offset = index * 32;
        self.cluster(cluster)[offset..offset + 32].copy_from_slice(&entry);
    }
}

impl ReadAt for Disk {
    fn byte_len(&self) -> u64 {
        self.bytes
    }

    fn read_exact_at(&self, offset: u64, out: &mut [u8]) -> Result<(), ReadError> {
        let end = offset.checked_add(out.len() as u64).ok_or(ReadError)?;
        if end > self.bytes
            || self
                .fail_at
                .is_some_and(|byte| (offset..end).contains(&byte))
        {
            return Err(ReadError);
        }
        let mut remaining = out;
        let mut position = offset;
        while !remaining.is_empty() {
            let sector = position / SECTOR as u64;
            let start = (position % SECTOR as u64) as usize;
            let count = remaining.len().min(SECTOR - start);
            let (chunk, tail) = remaining.split_at_mut(count);
            if let Some(bytes) = self.sectors.get(&sector) {
                chunk.copy_from_slice(&bytes[start..start + count]);
            } else {
                chunk.fill(0);
            }
            position += count as u64;
            remaining = tail;
        }
        Ok(())
    }
}

impl kitty_storage::ReadSectors for Disk {
    fn sector_bytes(&self) -> u32 {
        SECTOR as u32
    }

    fn sector_count(&self) -> u64 {
        self.bytes / SECTOR as u64
    }

    fn read_sectors(&self, first: u64, out: &mut [u8]) -> Result<(), ReadError> {
        assert!(out.len().is_multiple_of(SECTOR));
        self.read_exact_at(first * SECTOR as u64, out)
    }
}

fn entry(alias: &[u8; 11], attrs: u8, cluster: u32, size: u32) -> [u8; 32] {
    let mut bytes = [0; 32];
    bytes[..11].copy_from_slice(alias);
    bytes[11] = attrs;
    bytes[20..22].copy_from_slice(&((cluster >> 16) as u16).to_le_bytes());
    bytes[26..28].copy_from_slice(&(cluster as u16).to_le_bytes());
    bytes[28..32].copy_from_slice(&size.to_le_bytes());
    bytes
}

fn long_entries(name: &str, alias: &[u8; 11]) -> Vec<[u8; 32]> {
    let mut units: Vec<u16> = name.encode_utf16().collect();
    if !units.len().is_multiple_of(13) {
        units.push(0);
        units.resize(units.len().next_multiple_of(13), 0xffff);
    }
    let count = units.len() / 13;
    units
        .as_chunks::<13>()
        .0
        .iter()
        .enumerate()
        .rev()
        .map(|(index, units)| {
            let mut entry = [0; 32];
            entry[0] = (index + 1) as u8;
            if index + 1 == count {
                entry[0] |= 0x40;
            }
            entry[11] = 0x0f;
            entry[13] = checksum(alias);
            for (&unit, offset) in units
                .iter()
                .zip([1, 3, 5, 7, 9, 14, 16, 18, 20, 22, 24, 28, 30])
            {
                entry[offset..offset + 2].copy_from_slice(&unit.to_le_bytes());
            }
            entry
        })
        .collect()
}

#[test]
fn reads_nested_fragmented_files_by_offset_and_stops_at_eof() {
    let mut disk = Disk::new();
    disk.entry(2, 0, entry(b"CONFIG     ", 0x10, 3, 0));
    disk.link(3, EOC);
    disk.entry(3, 0, entry(b".          ", 0x10, 3, 0));
    disk.entry(3, 1, entry(b"..         ", 0x10, 0, 0));
    disk.entry(3, 2, entry(b"BOOT    TXT", 0x20, 7, 518));
    disk.entry(3, 3, entry(b"EMPTY   TXT", 0x20, 0, 0));
    // The top four allocation bits are not part of the cluster value.
    disk.link(7, 0xa000_0004);
    disk.link(4, EOC);
    disk.cluster(7).fill(b'A');
    disk.cluster(4)[..6].copy_from_slice(b"kitty!");

    let fs = Fat32::mount(disk).unwrap();
    let listed: Vec<_> = fs
        .read_dir("/config")
        .unwrap()
        .map(|item| {
            let item = item.unwrap();
            (String::from(item.name()), item.kind, item.size)
        })
        .collect();
    assert_eq!(
        listed,
        [
            (String::from("BOOT.TXT"), Kind::File, 518),
            (String::from("EMPTY.TXT"), Kind::File, 0)
        ]
    );
    let mut file = fs.open("/config/boot.txt").unwrap();
    let mut output = [b'?'; 10];
    assert_eq!(file.read_at(510, &mut output), Ok(8));
    assert_eq!(&output, b"AAkitty!??");
    assert_eq!(file.read_at(514, &mut output[..4]), Ok(4));
    assert_eq!(&output[..4], b"tty!");
    assert_eq!(file.read_at(u64::MAX, &mut output), Ok(0));
    assert_eq!(file.read_at(0, &mut output[..2]), Ok(2));
    assert_eq!(&output[..2], b"AA");
    assert_eq!(
        fs.open("/config/EMPTY.txt")
            .unwrap()
            .read_at(0, &mut output),
        Ok(0)
    );
}

#[test]
fn decodes_a_unicode_long_name_across_clusters_and_accepts_its_alias() {
    let mut disk = Disk::new();
    disk.cluster(2).fill(0xe5);
    disk.link(2, 8);
    disk.link(8, EOC);
    let name = "Configuração 🐈.txt";
    let alias = b"CONFIG~1TXT";
    let slots = long_entries(name, alias);
    assert_eq!(slots.len(), 2);
    disk.entry(2, 15, slots[0]);
    disk.entry(8, 0, slots[1]);
    disk.entry(8, 1, entry(alias, 0x20, 0, 0));
    disk.entry(8, 2, entry(b"VOL_LABEL  ", 0x08, 0, 0));

    let fs = Fat32::mount(disk).unwrap();
    let mut entries = fs.read_dir("/").unwrap();
    assert_eq!(entries.next().unwrap().unwrap().name(), name);
    assert!(entries.next().is_none());
    assert!(fs.open("/Configuração 🐈.txt").unwrap().is_empty());
    assert!(fs.open("/config~1.txt").unwrap().is_empty());
}

#[test]
fn uses_the_short_name_when_long_name_metadata_is_malformed() {
    let alias = b"KITTY~1 TXT";
    let make_fs = |alter: fn(&mut [u8; 32])| {
        let mut disk = Disk::new();
        let mut slot = long_entries("Kitty OS.txt", alias)[0];
        alter(&mut slot);
        disk.entry(2, 0, slot);
        disk.entry(2, 1, entry(alias, 0x20, 0, 0));
        Fat32::mount(disk).unwrap()
    };
    for alter in [
        (|slot: &mut [u8; 32]| slot[13] ^= 1) as fn(&mut [u8; 32]),
        |slot| slot[0] = 0x42,
        |slot| slot[1..3].copy_from_slice(&0xd800u16.to_le_bytes()),
        |slot| slot[1] = b'/',
    ] {
        let fs = make_fs(alter);
        assert_eq!(
            fs.read_dir("/").unwrap().next().unwrap().unwrap().name(),
            "KITTY~1.TXT"
        );
        assert_eq!(fs.open("/Kitty OS.txt").err(), Some(Error::NotFound));
    }
}

#[test]
fn accepts_maximum_long_name_and_exact_slots_without_a_terminator() {
    let mut disk = Disk::new();
    disk.link(2, 3);
    disk.link(3, EOC);
    let alias = b"LONG~1  TXT";
    let name = "é".repeat(255);
    let slots = long_entries(&name, alias);
    assert_eq!(slots.len(), 20);
    for (index, &slot) in slots.iter().enumerate() {
        disk.entry(2 + (index / 16) as u32, index % 16, slot);
    }
    disk.entry(3, 4, entry(alias, 0, 0, 0));
    let slots = long_entries("123456789.txt", b"OTHER~1 TXT");
    assert_eq!(slots.len(), 1);
    disk.entry(3, 5, slots[0]);
    disk.entry(3, 6, entry(b"OTHER~1 TXT", 0, 0, 0));
    let fs = Fat32::mount(disk).unwrap();
    let mut entries = fs.read_dir("/").unwrap();
    assert_eq!(entries.next().unwrap().unwrap().name(), name);
    assert_eq!(entries.next().unwrap().unwrap().name(), "123456789.txt");
}

#[test]
fn decodes_short_oem_names_and_case_flags() {
    let mut disk = Disk::new();
    let mut short = entry(b"CAF\x90    TXT", 0, 0, 0);
    short[12] = 0x18;
    disk.entry(2, 0, short);
    let fs = Fat32::mount(disk).unwrap();
    assert_eq!(
        fs.read_dir("/").unwrap().next().unwrap().unwrap().name(),
        "cafÉ.txt"
    );
    assert!(fs.open("/CAFÉ.TXT").is_ok());
}

#[test]
fn rejects_size_chain_mismatches_reserved_values_and_cycles() {
    for (first_link, second_link, size) in [
        (EOC, EOC, 513),
        (4, EOC, 512),
        (0x0fff_fff7, EOC, 512),
        (0x0fff_fff0, EOC, 512),
        (1, EOC, 512),
        (0, EOC, 512),
        (CLUSTERS + 2, EOC, 512),
        (3, EOC, 1024),
        (4, 3, 4096),
    ] {
        let mut disk = Disk::new();
        disk.entry(2, 0, entry(b"BLOCKS  BIN", 0, 3, size));
        disk.link(3, first_link);
        disk.link(4, second_link);
        let fs = Fat32::mount(disk).unwrap();
        assert_eq!(fs.open("/BLOCKS.BIN").err(), Some(Error::InvalidChain));
    }
}

#[test]
fn aborts_a_cyclic_directory_and_fuses_the_iterator() {
    let mut disk = Disk::new();
    disk.link(2, 6);
    disk.link(6, 2);
    disk.cluster(2).fill(0xe5);
    disk.cluster(6).fill(0xe5);
    let fs = Fat32::mount(disk).unwrap();
    let mut entries = fs.read_dir("/").unwrap();
    assert_eq!(
        entries.next().and_then(Result::err),
        Some(Error::InvalidChain)
    );
    assert!(entries.next().is_none());
    assert_eq!(fs.open("/MISSING").err(), Some(Error::InvalidChain));
}

#[test]
fn bounds_paths_and_refuses_ambiguous_names() {
    let mut disk = Disk::new();
    disk.entry(2, 0, entry(b"DUP     TXT", 0, 0, 0));
    let alias = b"OTHER   TXT";
    let long = long_entries("dup.txt", alias)[0];
    disk.entry(2, 1, long);
    disk.entry(2, 2, entry(alias, 0, 0, 0));
    let fs = Fat32::mount(disk).unwrap();
    assert_eq!(fs.open("/dup.txt").err(), Some(Error::AmbiguousName));
    assert_eq!(fs.open("/").err(), Some(Error::NotFile));
    assert_eq!(fs.read_dir("/other.txt").err(), Some(Error::NotDirectory));
    assert_eq!(fs.open("/other.txt/sub").err(), Some(Error::NotDirectory));
    for path in [
        "",
        "name",
        "//",
        "/./x",
        "/x/../y",
        "/trailing/",
        "/back\\slash",
        "/nul\0",
    ] {
        assert_eq!(fs.open(path).err(), Some(Error::InvalidPath), "{path:?}");
    }
    assert_eq!(
        fs.open(&std::format!("/{}", "x".repeat(256))).err(),
        Some(Error::InvalidPath)
    );
}

#[test]
fn checks_geometry_before_reading_allocation_or_data() {
    let mut disk = Disk::new();
    disk.bytes -= 1;
    assert_eq!(Fat32::mount(disk).err(), Some(Error::TruncatedVolume));
    for (field, byte, error) in [
        (510, 0, Error::InvalidBootSector),
        (12, 3, Error::InvalidBootSector),
        (13, 3, Error::InvalidBootSector),
        (17, 1, Error::InvalidBootSector),
        (42, 1, Error::InvalidBootSector),
        (40, 0x83, Error::InvalidGeometry),
        (44, 1, Error::InvalidGeometry),
    ] {
        let mut disk = Disk::new();
        disk.boot()[field] = byte;
        assert_eq!(Fat32::mount(disk).err(), Some(error), "boot byte {field}");
    }
    let mut disk = Disk::new();
    disk.boot()[36..40].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(Fat32::mount(disk).err(), Some(Error::InvalidGeometry));
    let mut disk = Disk::new();
    disk.boot()[36..40].copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(Fat32::mount(disk).err(), Some(Error::InvalidGeometry));
}

#[test]
fn consults_the_active_fat_when_mirroring_is_disabled() {
    let mut disk = Disk::new();
    disk.boot()[40] = 0x81;
    disk.entry(2, 0, entry(b"ACTIVE  TXT", 0, 3, 1));
    disk.link_in(1, 3, EOC);
    disk.cluster(3)[0] = b'K';
    let fs = Fat32::mount(disk).unwrap();
    let mut byte = [0];
    assert_eq!(fs.open("/ACTIVE.TXT").unwrap().read_at(0, &mut byte), Ok(1));
    assert_eq!(byte, [b'K']);
}

#[test]
fn returns_source_errors_without_reporting_bytes_as_read() {
    let mut disk = Disk::new();
    disk.entry(2, 0, entry(b"FAILED  BIN", 0, 3, 512));
    disk.link(3, EOC);
    disk.fail_at = Some(u64::from(DATA_SECTOR + 1) * SECTOR as u64);
    let fs = Fat32::mount(disk).unwrap();
    let mut file = fs.open("/FAILED.BIN").unwrap();
    assert_eq!(file.read_at(0, &mut [0; 32]), Err(Error::Read));
    assert_eq!(file.read_at(512, &mut [0; 32]), Ok(0));
}

#[test]
fn reads_directory_and_file_data_across_sectors_within_a_cluster() {
    let mut disk = Disk::new();
    let total_sectors = DATA_SECTOR + CLUSTERS * 2;
    disk.boot()[13] = 2;
    disk.boot()[32..36].copy_from_slice(&total_sectors.to_le_bytes());
    disk.bytes = u64::from(total_sectors) * SECTOR as u64;
    let data = u64::from(DATA_SECTOR);
    disk.sector(data).fill(0xe5);
    disk.sector(data + 1)[..32].copy_from_slice(&entry(b"BIGFILE BIN", 0, 3, 1026));
    disk.sector(data + 2)[510..].copy_from_slice(b"AB");
    disk.sector(data + 3)[..2].copy_from_slice(b"CD");
    disk.sector(data + 3)[510..].copy_from_slice(b"EF");
    disk.sector(data + 10)[..2].copy_from_slice(b"GH");
    disk.link(3, 7);
    disk.link(7, EOC);

    let fs = Fat32::mount(kitty_storage::SectorReader::new(disk).unwrap()).unwrap();
    let mut file = fs.open("/bigfile.bin").unwrap();
    let mut bytes = [0; 4];
    assert_eq!(file.read_at(510, &mut bytes), Ok(4));
    assert_eq!(&bytes, b"ABCD");
    assert_eq!(file.read_at(1022, &mut bytes), Ok(4));
    assert_eq!(&bytes, b"EFGH");
}
