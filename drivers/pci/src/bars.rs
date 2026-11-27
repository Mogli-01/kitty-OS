//! Read-only interpretation of the BAR words in a conventional PCI header.

use core::iter::FusedIterator;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Bar {
    pub slot: usize,
    pub resource: BarResource,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BarResource {
    Io { base: u32 },
    Memory32 { base: u32, prefetchable: bool },
    Memory64 { base: u64, prefetchable: bool },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BarError {
    ReservedIoBit { slot: usize },
    UnsupportedMemoryType { slot: usize, memory_type: u8 },
    MissingUpperWord { slot: usize },
}

/// Decodes assigned-value fields only. A zero low word does not distinguish
/// an unimplemented BAR from an unassigned 32-bit memory BAR.
pub struct Bars<'a> {
    words: &'a [u32],
    next: usize,
}

impl<'a> Bars<'a> {
    pub(super) const fn new(words: &'a [u32]) -> Self {
        Self { words, next: 0 }
    }
}

impl Iterator for Bars<'_> {
    type Item = Result<Bar, BarError>;

    fn next(&mut self) -> Option<Self::Item> {
        let (slot, low) = loop {
            let slot = self.next;
            let low = *self.words.get(slot)?;
            self.next += 1;
            if low != 0 {
                break (slot, low);
            }
        };

        let resource = if low & 1 != 0 {
            if low & 2 != 0 {
                return Some(Err(BarError::ReservedIoBit { slot }));
            }
            BarResource::Io { base: low & !3 }
        } else {
            let prefetchable = low & 8 != 0;
            match ((low >> 1) & 3) as u8 {
                0 => BarResource::Memory32 {
                    base: low & !15,
                    prefetchable,
                },
                2 => {
                    let Some(&high) = self.words.get(self.next) else {
                        return Some(Err(BarError::MissingUpperWord { slot }));
                    };
                    self.next += 1;
                    let base = (u64::from(high) << 32) | u64::from(low & !15);
                    BarResource::Memory64 { base, prefetchable }
                }
                memory_type => {
                    return Some(Err(BarError::UnsupportedMemoryType { slot, memory_type }));
                }
            }
        };
        Some(Ok(Bar { slot, resource }))
    }
}

impl FusedIterator for Bars<'_> {}
