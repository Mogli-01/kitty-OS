//! Adapt stable, read-only logical sectors to byte-addressed reads.

use crate::{ReadAt, ReadError};

const MAX_SECTOR_BYTES: usize = 4096;

/// Read complete logical sectors. The geometry and contents must remain
/// stable while they are exposed through a `SectorReader`.
pub trait ReadSectors {
    fn sector_bytes(&self) -> u32;
    fn sector_count(&self) -> u64;

    /// Fill a whole number of sectors starting at `first`. The destination
    /// is regular borrowed memory, with no extra alignment guarantees.
    fn read_sectors(&self, first: u64, out: &mut [u8]) -> Result<(), ReadError>;
}

/// A bounded byte view of a logical-sector reader.
pub struct SectorReader<D> {
    device: D,
    sector_bytes: u32,
    length: u64,
}

impl<D: ReadSectors> SectorReader<D> {
    pub fn new(device: D) -> Result<Self, ReadError> {
        let sector_bytes = device.sector_bytes();
        if !matches!(sector_bytes, 512 | 1024 | 2048 | 4096) {
            return Err(ReadError);
        }
        let length = device
            .sector_count()
            .checked_mul(u64::from(sector_bytes))
            .filter(|&bytes| bytes > 0)
            .ok_or(ReadError)?;
        Ok(Self {
            device,
            sector_bytes,
            length,
        })
    }

    pub const fn sector_bytes(&self) -> u32 {
        self.sector_bytes
    }
}

impl<D: ReadSectors> ReadAt for SectorReader<D> {
    fn byte_len(&self) -> u64 {
        self.length
    }

    fn read_exact_at(&self, offset: u64, mut out: &mut [u8]) -> Result<(), ReadError> {
        if offset
            .checked_add(out.len() as u64)
            .is_none_or(|end| end > self.length)
        {
            return Err(ReadError);
        }
        if out.is_empty() {
            return Ok(());
        }

        let sector_bytes = self.sector_bytes as usize;
        let block_size = u64::from(self.sector_bytes);
        let mut sector = offset / block_size;
        let prefix = (offset % block_size) as usize;
        let mut scratch = [0; MAX_SECTOR_BYTES];

        if prefix != 0 {
            self.device
                .read_sectors(sector, &mut scratch[..sector_bytes])?;
            let copied = out.len().min(sector_bytes - prefix);
            let (head, rest) = out.split_at_mut(copied);
            head.copy_from_slice(&scratch[prefix..prefix + copied]);
            out = rest;
            sector += 1;
        }
        let whole = out.len() / sector_bytes * sector_bytes;
        if whole != 0 {
            let (head, rest) = out.split_at_mut(whole);
            self.device.read_sectors(sector, head)?;
            out = rest;
            sector += (whole / sector_bytes) as u64;
        }
        if !out.is_empty() {
            self.device
                .read_sectors(sector, &mut scratch[..sector_bytes])?;
            out.copy_from_slice(&scratch[..out.len()]);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use core::cell::Cell;

    use super::*;

    struct Disk {
        sectors: u64,
        bytes: u32,
        reads: Cell<usize>,
    }

    impl ReadSectors for Disk {
        fn sector_bytes(&self) -> u32 {
            self.bytes
        }

        fn sector_count(&self) -> u64 {
            self.sectors
        }

        fn read_sectors(&self, first: u64, out: &mut [u8]) -> Result<(), ReadError> {
            self.reads.set(self.reads.get() + 1);
            assert!(out.len().is_multiple_of(self.bytes as usize));
            for (i, sector) in out.chunks_exact_mut(self.bytes as usize).enumerate() {
                assert!(first + (i as u64) < self.sectors);
                sector.fill((first + i as u64) as u8);
            }
            Ok(())
        }
    }

    #[test]
    fn reads_unaligned_prefix_and_tail_with_batched_sectors_between() {
        let source = SectorReader::new(Disk {
            sectors: 5,
            bytes: 512,
            reads: Cell::new(0),
        })
        .unwrap();
        let mut out = [0xff; 1028];
        source.read_exact_at(510, &mut out).unwrap();
        assert_eq!(&out[..2], &[0; 2]);
        assert_eq!(&out[2..514], &[1; 512]);
        assert_eq!(&out[514..1026], &[2; 512]);
        assert_eq!(&out[1026..], &[3; 2]);
        assert_eq!(source.device.reads.get(), 3);
    }

    #[test]
    fn rejects_bad_geometry_and_bounds_requests_before_io() {
        for (sectors, bytes) in [(0, 512), (1, 42), (u64::MAX, 4096)] {
            assert!(
                SectorReader::new(Disk {
                    sectors,
                    bytes,
                    reads: Cell::new(0)
                })
                .is_err()
            );
        }
        let source = SectorReader::new(Disk {
            sectors: 1,
            bytes: 4096,
            reads: Cell::new(0),
        })
        .unwrap();
        assert_eq!(source.read_exact_at(4096, &mut []), Ok(()));
        assert_eq!(source.read_exact_at(4097, &mut []), Err(ReadError));
        assert_eq!(source.read_exact_at(4095, &mut [0; 2]), Err(ReadError));
        assert_eq!(source.read_exact_at(u64::MAX, &mut [0; 2]), Err(ReadError));
        assert_eq!(source.device.reads.get(), 0);
    }
}
