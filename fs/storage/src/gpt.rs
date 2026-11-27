//! Reads and validates a GUID Partition Table before exposing partitions.

use alloc::vec::Vec;

use crate::{ReadAt, ReadError, crc32::Crc32};

const MIN_HEADER_BYTES: usize = 92;
const MAX_SECTOR_BYTES: usize = 4096;
const MIN_ENTRY_BYTES: usize = 128;
const MAX_ENTRY_COUNT: u32 = 4096;
const MAX_TABLE_BYTES: usize = 4 * 1024 * 1024;
const PRIMARY_HEADER_LBA: u64 = 1;
const MBR_ENTRY_OFFSET: usize = 446;

pub type Guid = [u8; 16];

/// EFI System Partition type GUID in GPT's on-disk byte order.
pub const EFI_SYSTEM: Guid = [
    0x28, 0x73, 0x2a, 0xc1, 0x1f, 0xf8, 0xd2, 0x11, 0xba, 0x4b, 0x00, 0xa0, 0xc9, 0x3e, 0xc9, 0x3b,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    InvalidDiskGeometry,
    Read,
    InvalidProtectiveMbr,
    InvalidHeader,
    HeaderChecksum,
    InvalidTableGeometry,
    TableChecksum,
    InvalidPartition,
    OverlappingPartitions,
    DuplicatePartitionGuid,
    OutOfMemory,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HeaderCopy {
    Primary,
    Backup,
}

#[derive(Debug, Eq, PartialEq)]
pub struct Partition {
    number: u32,
    partition_type: Guid,
    id: Guid,
    first_lba: u64,
    last_lba: u64,
    attributes: u64,
    name: [u16; 36],
}

impl Partition {
    /// One-based partition number, preserving gaps in the on-disk table.
    pub fn number(&self) -> u32 {
        self.number
    }

    pub fn partition_type(&self) -> Guid {
        self.partition_type
    }

    pub fn id(&self) -> Guid {
        self.id
    }

    pub fn first_lba(&self) -> u64 {
        self.first_lba
    }

    pub fn last_lba(&self) -> u64 {
        self.last_lba
    }

    pub fn attributes(&self) -> u64 {
        self.attributes
    }

    /// Raw UTF-16 units of the optional display name. The name is metadata;
    /// consumers should identify a partition by number or GUID.
    pub fn name_units(&self) -> &[u16] {
        let length = self
            .name
            .iter()
            .position(|&unit| unit == 0)
            .unwrap_or(self.name.len());
        &self.name[..length]
    }
}

#[derive(Debug)]
pub struct Gpt<R> {
    disk: R,
    sector_bytes: u32,
    disk_id: Guid,
    header_copy: HeaderCopy,
    partitions: Vec<Partition>,
}

impl<R: ReadAt> Gpt<R> {
    /// Tries the primary GPT followed by the backup on a read or validation
    /// error. Both copies failing returns the primary error. Disk contents
    /// must remain stable for the lifetime of the returned table.
    pub fn read(disk: R, sector_bytes: u32) -> Result<Self, Error> {
        let bytes = u64::from(sector_bytes);
        let length = disk.byte_len();
        if !matches!(sector_bytes, 512 | 1024 | 2048 | 4096)
            || !length.is_multiple_of(bytes)
            || length / bytes < 6
        {
            return Err(Error::InvalidDiskGeometry);
        }
        let last_lba = length / bytes - 1;
        validate_protective_mbr(&disk, last_lba)?;
        let load = |copy| read_table(&disk, sector_bytes, last_lba, copy);
        let (table, header_copy) = match load(HeaderCopy::Primary) {
            Ok(table) => (table, HeaderCopy::Primary),
            Err(Error::OutOfMemory) => return Err(Error::OutOfMemory),
            Err(primary_error) => match load(HeaderCopy::Backup) {
                Ok(table) => (table, HeaderCopy::Backup),
                Err(Error::OutOfMemory) => return Err(Error::OutOfMemory),
                Err(_) => return Err(primary_error),
            },
        };
        Ok(Self {
            disk,
            sector_bytes,
            disk_id: table.disk_id,
            header_copy,
            partitions: table.partitions,
        })
    }

    pub fn disk_id(&self) -> Guid {
        self.disk_id
    }

    pub fn header_copy(&self) -> HeaderCopy {
        self.header_copy
    }

    pub fn partitions(&self) -> &[Partition] {
        &self.partitions
    }

    /// Opens a byte view rooted at the start of a validated partition.
    pub fn partition(&self, number: u32) -> Option<PartitionView<'_, R>> {
        let index = self
            .partitions
            .binary_search_by_key(&number, Partition::number)
            .ok()?;
        let partition = &self.partitions[index];
        let sector_bytes = u64::from(self.sector_bytes);
        Some(PartitionView {
            disk: &self.disk,
            offset: partition.first_lba * sector_bytes,
            length: (partition.last_lba - partition.first_lba + 1) * sector_bytes,
        })
    }
}

pub struct PartitionView<'a, R> {
    disk: &'a R,
    offset: u64,
    length: u64,
}

impl<R: ReadAt> ReadAt for PartitionView<'_, R> {
    fn byte_len(&self) -> u64 {
        self.length
    }

    fn read_exact_at(&self, offset: u64, out: &mut [u8]) -> Result<(), ReadError> {
        let remaining = self.length.checked_sub(offset).ok_or(ReadError)?;
        if out.len() as u64 > remaining {
            return Err(ReadError);
        }
        if out.is_empty() {
            return Ok(());
        }
        // The entire partition range was checked against the disk length.
        self.disk.read_exact_at(self.offset + offset, out)
    }
}

struct Header {
    disk_id: Guid,
    first_usable: u64,
    last_usable: u64,
    entries_lba: u64,
    entry_count: u32,
    entry_bytes: usize,
    table_crc: u32,
}

struct Table {
    disk_id: Guid,
    partitions: Vec<Partition>,
}

fn validate_protective_mbr(disk: &impl ReadAt, last_lba: u64) -> Result<(), Error> {
    let mut mbr = [0; 512];
    read(disk, 0, &mut mbr)?;
    if mbr[510..] != [0x55, 0xaa] {
        return Err(Error::InvalidProtectiveMbr);
    }

    let count = last_lba.min(u64::from(u32::MAX)) as u32;
    let mut found = false;
    for entry in mbr[MBR_ENTRY_OFFSET..510].as_chunks::<16>().0 {
        if *entry == [0; 16] {
            continue;
        }
        if found
            || entry[0] != 0
            || entry[4] != 0xee
            || dword(entry, 8) != 1
            || dword(entry, 12) != count
        {
            return Err(Error::InvalidProtectiveMbr);
        }
        found = true;
    }
    if !found {
        return Err(Error::InvalidProtectiveMbr);
    }
    Ok(())
}

fn read_table(
    disk: &impl ReadAt,
    sector_bytes: u32,
    last_lba: u64,
    copy: HeaderCopy,
) -> Result<Table, Error> {
    let (current, alternate) = match copy {
        HeaderCopy::Primary => (PRIMARY_HEADER_LBA, last_lba),
        HeaderCopy::Backup => (last_lba, PRIMARY_HEADER_LBA),
    };
    let sector_bytes = u64::from(sector_bytes);
    let mut block = [0; MAX_SECTOR_BYTES];
    let header_block = &mut block[..sector_bytes as usize];
    read(disk, current * sector_bytes, header_block)?;
    let header = Header::parse(header_block, current, alternate, last_lba)?;
    let table_bytes = header.entry_count as usize * header.entry_bytes;
    let sectors = (table_bytes as u64).div_ceil(sector_bytes);
    let table_end = header
        .entries_lba
        .checked_add(sectors)
        .ok_or(Error::InvalidTableGeometry)?;
    let location_valid = match copy {
        HeaderCopy::Primary => header.entries_lba > current && table_end <= header.first_usable,
        HeaderCopy::Backup => header.entries_lba > header.last_usable && table_end <= current,
    };
    // Both usable bounds must leave room for the headers and their arrays.
    let backup_array_start = last_lba
        .checked_sub(sectors)
        .ok_or(Error::InvalidTableGeometry)?;
    if !location_valid
        || header.first_usable < 2 + sectors
        || header.last_usable >= backup_array_start
    {
        return Err(Error::InvalidTableGeometry);
    }

    let mut partitions = Vec::new();
    let mut crc = Crc32::new();
    let mut position = header.entries_lba * sector_bytes;
    let mut remaining = table_bytes;
    let mut index = 0;
    // Supported entry sizes all divide the scratch size, keeping records
    // intact. The final read omits the sector padding from the checksum.
    while remaining != 0 {
        let count = remaining.min(block.len());
        let bytes = &mut block[..count];
        read(disk, position, bytes)?;
        crc.update(bytes);
        for entry in bytes.chunks_exact(header.entry_bytes) {
            index += 1;
            if let Some(partition) = header.partition(index, entry)? {
                partitions.try_reserve(1).map_err(|_| Error::OutOfMemory)?;
                partitions.push(partition);
            }
        }
        position += count as u64;
        remaining -= count;
    }

    if crc.finish() != header.table_crc {
        return Err(Error::TableChecksum);
    }
    partitions.sort_unstable_by_key(Partition::first_lba);
    if partitions
        .array_windows::<2>()
        .any(|[a, b]| a.last_lba >= b.first_lba)
    {
        return Err(Error::OverlappingPartitions);
    }
    partitions.sort_unstable_by_key(Partition::id);
    if partitions.array_windows::<2>().any(|[a, b]| a.id == b.id) {
        return Err(Error::DuplicatePartitionGuid);
    }
    partitions.sort_unstable_by_key(Partition::number);
    Ok(Table {
        disk_id: header.disk_id,
        partitions,
    })
}

impl Header {
    fn parse(block: &[u8], current: u64, alternate: u64, last_lba: u64) -> Result<Self, Error> {
        let length = dword(block, 12) as usize;
        if &block[..8] != b"EFI PART"
            || dword(block, 8) != 0x0001_0000
            || !(MIN_HEADER_BYTES..=block.len()).contains(&length)
            || dword(block, 20) != 0
            || qword(block, 24) != current
            || qword(block, 32) != alternate
        {
            return Err(Error::InvalidHeader);
        }
        let mut crc = Crc32::new();
        crc.update(&block[..16]);
        crc.update(&[0; 4]);
        crc.update(&block[20..length]);
        if crc.finish() != dword(block, 16) {
            return Err(Error::HeaderChecksum);
        }

        let first_usable = qword(block, 40);
        let last_usable = qword(block, 48);
        let disk_id = guid(block, 56);
        let entries_lba = qword(block, 72);
        let entry_count = dword(block, 80);
        let entry_bytes = dword(block, 84) as usize;
        if first_usable < 2
            || first_usable > last_usable
            || last_usable >= last_lba
            || disk_id == [0; 16]
            || !(1..=MAX_ENTRY_COUNT).contains(&entry_count)
            || !(MIN_ENTRY_BYTES..=MAX_SECTOR_BYTES).contains(&entry_bytes)
            || !entry_bytes.is_power_of_two()
            || entry_count as usize * entry_bytes > MAX_TABLE_BYTES
        {
            return Err(Error::InvalidTableGeometry);
        }
        Ok(Self {
            disk_id,
            first_usable,
            last_usable,
            entries_lba,
            entry_count,
            entry_bytes,
            table_crc: dword(block, 88),
        })
    }

    fn partition(&self, number: u32, entry: &[u8]) -> Result<Option<Partition>, Error> {
        let partition_type = guid(entry, 0);
        if partition_type == [0; 16] {
            return Ok(None);
        }
        let id = guid(entry, 16);
        let first_lba = qword(entry, 32);
        let last_lba = qword(entry, 40);
        if id == [0; 16]
            || first_lba < self.first_usable
            || last_lba > self.last_usable
            || first_lba > last_lba
        {
            return Err(Error::InvalidPartition);
        }
        let mut name = [0; 36];
        for (unit, &bytes) in name.iter_mut().zip(entry[56..128].as_chunks::<2>().0) {
            *unit = u16::from_le_bytes(bytes);
        }
        Ok(Some(Partition {
            number,
            partition_type,
            id,
            first_lba,
            last_lba,
            attributes: qword(entry, 48),
            name,
        }))
    }
}

fn read(disk: &impl ReadAt, offset: u64, out: &mut [u8]) -> Result<(), Error> {
    disk.read_exact_at(offset, out).map_err(|_| Error::Read)
}

fn field<const N: usize>(bytes: &[u8], offset: usize) -> [u8; N] {
    // Callers provide a complete MBR record, header sector or partition record,
    // and use offsets defined by those fixed layouts.
    core::array::from_fn(|index| bytes[offset + index])
}

fn dword(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(field(bytes, offset))
}

fn qword(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(field(bytes, offset))
}

fn guid(bytes: &[u8], offset: usize) -> Guid {
    field(bytes, offset)
}

#[cfg(test)]
mod tests;
