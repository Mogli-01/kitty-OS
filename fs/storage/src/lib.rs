//! Bounded read-only storage views and partition discovery.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod gpt;

mod crc32;

/// The source could not fill the entire requested range.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReadError;

/// Byte access to a stable range, such as a disk image or partition.
///
/// The contents and length must remain unchanged while a parser uses the
/// source. Implementations for sector devices support unaligned byte requests.
pub trait ReadAt {
    fn byte_len(&self) -> u64;
    fn read_exact_at(&self, offset: u64, out: &mut [u8]) -> Result<(), ReadError>;
}

impl ReadAt for [u8] {
    fn byte_len(&self) -> u64 {
        self.len() as u64
    }

    fn read_exact_at(&self, offset: u64, out: &mut [u8]) -> Result<(), ReadError> {
        let offset = usize::try_from(offset).map_err(|_| ReadError)?;
        let bytes = self
            .get(offset..)
            .and_then(|tail| tail.get(..out.len()))
            .ok_or(ReadError)?;
        out.copy_from_slice(bytes);
        Ok(())
    }
}

impl<R: ReadAt + ?Sized> ReadAt for &R {
    fn byte_len(&self) -> u64 {
        (**self).byte_len()
    }

    fn read_exact_at(&self, offset: u64, out: &mut [u8]) -> Result<(), ReadError> {
        (**self).read_exact_at(offset, out)
    }
}
