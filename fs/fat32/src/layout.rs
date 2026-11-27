//! FAT32 boot sector, address calculations and allocation table.

use super::Error;

const MIN_CLUSTERS: u32 = 65_525;
pub(super) const FIRST_CLUSTER: u32 = 2;
pub(super) const CLUSTER_MASK: u32 = 0x0fff_ffff;
const MAX_CLUSTER: u32 = 0x0fff_ffef;
const END_OF_CHAIN: u32 = 0x0fff_fff8;

#[derive(Clone, Copy, Debug)]
pub(super) struct Layout {
    pub root: u32,
    pub cluster_bytes: u32,
    pub cluster_count: u32,
    fat_offset: u64,
    data_offset: u64,
}

impl Layout {
    pub fn parse(boot: &[u8; 512], source_bytes: u64) -> Result<Self, Error> {
        let sector_bytes = u32::from(word(boot, 11));
        let sectors_per_cluster = u32::from(boot[13]);
        let reserved = u32::from(word(boot, 14));
        let fat_count = u32::from(boot[16]);
        let total_sectors = dword(boot, 32);
        let fat_sectors = dword(boot, 36);
        let flags = word(boot, 40);
        let root = dword(boot, 44);

        if boot[510..] != [0x55, 0xaa]
            || !matches!(sector_bytes, 512 | 1024 | 2048 | 4096)
            || !sectors_per_cluster.is_power_of_two()
            || sector_bytes * sectors_per_cluster > 32 * 1024
            || reserved == 0
            || !matches!(fat_count, 1 | 2)
            || word(boot, 17) != 0
            || word(boot, 19) != 0
            || word(boot, 22) != 0
            || fat_sectors == 0
            || flags & !0x008f != 0
            || word(boot, 42) != 0
        {
            return Err(Error::InvalidBootSector);
        }

        let fat_index = if flags & 0x0080 == 0 {
            0
        } else {
            u32::from(flags & 0x000f)
        };
        if fat_index >= fat_count {
            return Err(Error::InvalidGeometry);
        }
        let data_sector = reserved
            .checked_add(
                fat_sectors
                    .checked_mul(fat_count)
                    .ok_or(Error::InvalidGeometry)?,
            )
            .ok_or(Error::InvalidGeometry)?;
        let data_sectors = total_sectors
            .checked_sub(data_sector)
            .ok_or(Error::InvalidGeometry)?;
        let cluster_count = data_sectors / sectors_per_cluster;
        let sector_bytes_u64 = u64::from(sector_bytes);

        if !(MIN_CLUSTERS..MAX_CLUSTER).contains(&cluster_count)
            || !(FIRST_CLUSTER..=cluster_count + 1).contains(&root)
            || u64::from(cluster_count + 2) * 4 > u64::from(fat_sectors) * sector_bytes_u64
        {
            return Err(Error::InvalidGeometry);
        }
        if u64::from(total_sectors) * sector_bytes_u64 > source_bytes {
            return Err(Error::TruncatedVolume);
        }
        // The selected FAT starts before data_sector, already checked above.
        Ok(Self {
            root,
            cluster_bytes: sector_bytes * sectors_per_cluster,
            cluster_count,
            fat_offset: u64::from(reserved + fat_index * fat_sectors) * sector_bytes_u64,
            data_offset: u64::from(data_sector) * sector_bytes_u64,
        })
    }

    pub fn contains(self, cluster: u32) -> bool {
        (FIRST_CLUSTER..=self.cluster_count + 1).contains(&cluster)
    }

    pub fn cluster_offset(self, cluster: u32) -> u64 {
        debug_assert!(self.contains(cluster));
        self.data_offset + u64::from(cluster - FIRST_CLUSTER) * u64::from(self.cluster_bytes)
    }

    pub fn fat_entry_offset(self, cluster: u32) -> u64 {
        debug_assert!(self.contains(cluster));
        self.fat_offset + u64::from(cluster) * 4
    }

    pub fn next_cluster(self, raw: u32) -> Result<Option<u32>, Error> {
        let entry = raw & CLUSTER_MASK;
        match entry {
            END_OF_CHAIN..=CLUSTER_MASK => Ok(None),
            cluster if self.contains(cluster) => Ok(Some(cluster)),
            // Includes unallocated, reserved, bad and out-of-volume clusters.
            _ => Err(Error::InvalidChain),
        }
    }
}

pub(super) fn word(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

pub(super) fn dword(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}
