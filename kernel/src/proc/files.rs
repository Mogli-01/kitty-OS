//! Bounded per-process table for read-only initrd files.

use crate::sync::IrqSpinLock;
use kitty_vfs::File;

pub(crate) const DIR: u64 = 1;
const FIRST_FILE: u64 = 2;
const SLOTS: usize = 16;

pub(crate) struct Files {
    slots: [Slot; SLOTS],
}

#[derive(Clone, Copy)]
struct Slot {
    generation: u16,
    file: Option<File<'static>>,
    offset: u64,
}

impl Files {
    pub(super) const fn table() -> IrqSpinLock<Self> {
        IrqSpinLock::new(Self {
            slots: [Slot {
                generation: 0,
                file: None,
                offset: 0,
            }; SLOTS],
        })
    }

    pub(crate) fn open(&mut self, file: File<'static>) -> Option<u64> {
        let (index, slot) = self
            .slots
            .iter_mut()
            .enumerate()
            .find(|(_, slot)| slot.file.is_none() && slot.generation != u16::MAX)?;
        slot.file = Some(file);
        slot.offset = 0;
        Some((u64::from(slot.generation) << 16) | (FIRST_FILE + index as u64))
    }

    pub(crate) fn read(
        &mut self,
        handle: u64,
        address: u64,
        buffer: &mut [u8],
    ) -> Result<usize, i64> {
        use crate::mm::user::{UserSlice, copy_to_user};

        let slot = self.slot(handle).ok_or(-3)?;
        let file = slot.file.ok_or(-3)?;
        UserSlice::new(address, buffer.len()).map_err(|_| -2)?;
        let count = file.read_at(slot.offset, buffer);
        let dest = UserSlice::new(address, count).map_err(|_| -2)?;
        copy_to_user(dest, &buffer[..count]).map_err(|_| -2)?;
        slot.offset += count as u64;
        Ok(count)
    }

    pub(crate) fn close(&mut self, handle: u64) -> bool {
        let Some(slot) = self.slot(handle) else {
            return false;
        };
        slot.file = None;
        // A generation is never reused. Retire the slot before wrapping.
        slot.generation += 1;
        true
    }

    pub(crate) fn contains(&mut self, handle: u64) -> bool {
        self.slot(handle).is_some()
    }

    fn slot(&mut self, handle: u64) -> Option<&mut Slot> {
        let handle = u32::try_from(handle).ok()?;
        let index = usize::try_from(u64::from(handle & 0xffff).checked_sub(FIRST_FILE)?).ok()?;
        let slot = self.slots.get_mut(index)?;
        (slot.file.is_some() && u32::from(slot.generation) == handle >> 16).then_some(slot)
    }
}
