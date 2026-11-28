//! Read-only discovery of the modern virtio-blk PCI register layout.

use crate::{CapabilityError, ConfigSpace, Header, Layout};

const VIRTIO_VENDOR: u16 = 0x1af4;
const VIRTIO_BLOCK_TRANSITIONAL: u16 = 0x1001;
const VIRTIO_BLOCK_MODERN: u16 = 0x1042;
const VENDOR_CAPABILITY: u8 = 0x09;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VirtioRegion {
    pub bar: u8,
    pub offset: u32,
    pub length: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VirtioBlockLayout {
    pub common: VirtioRegion,
    pub notify: VirtioRegion,
    pub notify_multiplier: u32,
    pub isr: VirtioRegion,
    pub device: VirtioRegion,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VirtioLayoutError {
    List(CapabilityError),
    InvalidCapability(u8),
    DuplicateKind(u8),
    MissingKind(u8),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VirtioBoundsError {
    MissingBar(u8),
    RegionTooShort(u8),
    RegionOutsideBar(u8),
    InvalidNotificationOffset,
}

impl VirtioBlockLayout {
    /// Check every region against lengths supplied by the memory BAR grants
    /// for this function. Use `None` for unavailable or non-memory slots,
    /// including the upper half of a 64-bit BAR.
    pub fn validate_bar_lengths(
        &self,
        lengths: &[Option<u64>; 6],
    ) -> Result<(), VirtioBoundsError> {
        for (region, minimum) in [
            (self.common, COMMON_MIN),
            (self.notify, NOTIFY_MIN),
            (self.isr, ISR_MIN),
            (self.device, DEVICE_MIN),
        ] {
            let bar_length = lengths
                .get(usize::from(region.bar))
                .copied()
                .flatten()
                .ok_or(VirtioBoundsError::MissingBar(region.bar))?;
            if region.length < minimum {
                return Err(VirtioBoundsError::RegionTooShort(region.bar));
            }
            if u64::from(region.offset) + u64::from(region.length) > bar_length {
                return Err(VirtioBoundsError::RegionOutsideBar(region.bar));
            }
        }
        Ok(())
    }

    /// BAR-relative offset of the 16-bit notification register for a queue
    /// when VIRTIO_F_NOTIFICATION_DATA has not been negotiated. Validate
    /// the layout against BAR grants before using the resulting offset.
    pub fn queue_notify_bar_offset(&self, queue_notify_off: u16) -> Result<u64, VirtioBoundsError> {
        let offset = u64::from(queue_notify_off) * u64::from(self.notify_multiplier);
        let bar_offset = u64::from(self.notify.offset) + offset;
        if offset + u64::from(NOTIFY_MIN) > u64::from(self.notify.length)
            || !bar_offset.is_multiple_of(u64::from(NOTIFY_MIN))
        {
            return Err(VirtioBoundsError::InvalidNotificationOffset);
        }
        Ok(bar_offset)
    }
}

const COMMON_MIN: u32 = 56;
const NOTIFY_MIN: u32 = 2;
const ISR_MIN: u32 = 1;
const DEVICE_MIN: u32 = 8;

impl Header {
    pub fn is_virtio_block(&self) -> bool {
        self.vendor_id == VIRTIO_VENDOR
            && matches!(
                self.device_id,
                VIRTIO_BLOCK_TRANSITIONAL | VIRTIO_BLOCK_MODERN
            )
            && matches!(self.layout, Layout::Endpoint(_))
    }
}

impl ConfigSpace<'_> {
    /// Find the four modern capabilities on a virtio-blk endpoint.
    /// The returned ranges are descriptors. Before touching any register,
    /// a caller must validate them against its own BAR resource grant.
    pub fn virtio_block_layout(&self) -> Result<Option<VirtioBlockLayout>, VirtioLayoutError> {
        if !self.header().is_virtio_block() {
            return Ok(None);
        }

        let mut regions = [None; 4];
        let mut notify_multiplier = 0;
        for entry in self.capabilities() {
            let entry = entry.map_err(VirtioLayoutError::List)?;
            if entry.id != VENDOR_CAPABILITY {
                continue;
            }
            let invalid = VirtioLayoutError::InvalidCapability(entry.offset);
            let tail = self.bytes.get(usize::from(entry.offset)..).ok_or(invalid)?;
            let &[_, _, length, kind] = tail.first_chunk::<4>().ok_or(invalid)?;
            let (header_len, minimum_region) = match kind {
                1 => (16, COMMON_MIN),
                2 => (20, NOTIFY_MIN),
                3 => (16, ISR_MIN),
                4 => (16, DEVICE_MIN),
                _ => continue,
            };
            let cap = tail.get(..usize::from(length)).ok_or(invalid)?;
            if cap.len() < header_len || cap[4] >= 6 {
                return Err(invalid);
            }
            let word = |start| {
                cap.get(start..)
                    .and_then(<[u8]>::first_chunk)
                    .copied()
                    .map(u32::from_le_bytes)
                    .ok_or(invalid)
            };
            let region = VirtioRegion {
                bar: cap[4],
                offset: word(8)?,
                length: word(12)?,
            };
            if region.length < minimum_region {
                return Err(invalid);
            }
            if regions[usize::from(kind - 1)].replace(region).is_some() {
                return Err(VirtioLayoutError::DuplicateKind(kind));
            }
            if kind == 2 {
                notify_multiplier = word(16)?;
            }
        }
        let [common, notify, isr, device] = regions;
        Ok(Some(VirtioBlockLayout {
            common: common.ok_or(VirtioLayoutError::MissingKind(1))?,
            notify: notify.ok_or(VirtioLayoutError::MissingKind(2))?,
            notify_multiplier,
            isr: isr.ok_or(VirtioLayoutError::MissingKind(3))?,
            device: device.ok_or(VirtioLayoutError::MissingKind(4))?,
        }))
    }
}
