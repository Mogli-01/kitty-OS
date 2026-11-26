//! Decode a read-only snapshot of conventional PCI configuration space.

#![no_std]
#![forbid(unsafe_code)]

use core::{array, iter::FusedIterator};

const HEADER_BYTES: usize = 64;
const CONVENTIONAL_BYTES: usize = 256;
const FIRST_CAPABILITY: u8 = 0x40;
const CAPABILITY_POINTER: usize = 0x34;
const STATUS_CAPABILITIES: u16 = 1 << 4;
const HEADER_TYPE_MASK: u8 = 0x7f;
const MULTIFUNCTION: u8 = 0x80;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HeaderError {
    Truncated,
    Absent,
    UnsupportedType(u8),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Header {
    pub vendor_id: u16,
    pub device_id: u16,
    pub command: u16,
    pub status: u16,
    pub revision: u8,
    pub programming_interface: u8,
    pub subclass: u8,
    pub class: u8,
    pub multifunction: bool,
    pub interrupt_line: u8,
    pub interrupt_pin: u8,
    pub layout: Layout,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Layout {
    Endpoint(Endpoint),
    Bridge(Bridge),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Endpoint {
    /// Raw BAR register values. These do not grant access to the ranges.
    pub bars: [u32; 6],
    pub subsystem_vendor_id: u16,
    pub subsystem_id: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Bridge {
    /// Raw BAR register values. These do not grant access to the ranges.
    pub bars: [u32; 2],
    pub primary_bus: u8,
    pub secondary_bus: u8,
    pub subordinate_bus: u8,
}

pub struct ConfigSpace<'a> {
    header: Header,
    bytes: &'a [u8],
}

impl<'a> ConfigSpace<'a> {
    /// Decodes a type 0 or type 1 header. Capability bodies and PCIe
    /// extended configuration space are outside this decoder's scope.
    pub fn parse(bytes: &'a [u8]) -> Result<Self, HeaderError> {
        let prefix = bytes
            .first_chunk::<HEADER_BYTES>()
            .ok_or(HeaderError::Truncated)?;
        let word = |offset| u16::from_le_bytes([prefix[offset], prefix[offset + 1]]);
        let vendor_id = word(0);
        if vendor_id == u16::MAX {
            return Err(HeaderError::Absent);
        }

        let words = prefix.as_chunks::<4>().0;
        let bar = |index| u32::from_le_bytes(words[4 + index]);
        let header_type = prefix[0x0e];
        let layout = match header_type & HEADER_TYPE_MASK {
            0 => Layout::Endpoint(Endpoint {
                bars: array::from_fn(bar),
                subsystem_vendor_id: word(0x2c),
                subsystem_id: word(0x2e),
            }),
            1 => Layout::Bridge(Bridge {
                bars: array::from_fn(bar),
                primary_bus: prefix[0x18],
                secondary_bus: prefix[0x19],
                subordinate_bus: prefix[0x1a],
            }),
            kind => return Err(HeaderError::UnsupportedType(kind)),
        };
        let header = Header {
            vendor_id,
            device_id: word(2),
            command: word(4),
            status: word(6),
            revision: prefix[8],
            programming_interface: prefix[9],
            subclass: prefix[0x0a],
            class: prefix[0x0b],
            multifunction: header_type & MULTIFUNCTION != 0,
            interrupt_line: prefix[0x3c],
            interrupt_pin: prefix[0x3d],
            layout,
        };
        Ok(Self {
            header,
            bytes: &bytes[..bytes.len().min(CONVENTIONAL_BYTES)],
        })
    }

    pub const fn header(&self) -> &Header {
        &self.header
    }

    /// Walks the conventional capability links only when enabled in status.
    /// A short snapshot can supply a complete header and a partial list.
    pub fn capabilities(&self) -> Capabilities<'a> {
        let next = if self.header.status & STATUS_CAPABILITIES != 0 {
            self.bytes[CAPABILITY_POINTER]
        } else {
            0
        };
        Capabilities {
            bytes: self.bytes,
            next,
            visited: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Capability {
    pub offset: u8,
    pub id: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CapabilityError {
    InvalidPointer(u8),
    Truncated(u8),
    Cycle(u8),
}

/// Reads only the ID and link bytes. Stops permanently on an invalid link.
pub struct Capabilities<'a> {
    bytes: &'a [u8],
    next: u8,
    visited: u64,
}

impl Iterator for Capabilities<'_> {
    type Item = Result<Capability, CapabilityError>;

    fn next(&mut self) -> Option<Self::Item> {
        let offset = core::mem::take(&mut self.next);
        if offset == 0 {
            return None;
        }
        if offset < FIRST_CAPABILITY || !offset.is_multiple_of(4) {
            return Some(Err(CapabilityError::InvalidPointer(offset)));
        }
        // At most 48 aligned offsets (0x40 through 0xfc) can be visited.
        let bit = 1_u64 << ((offset - FIRST_CAPABILITY) / 4);
        if self.visited & bit != 0 {
            return Some(Err(CapabilityError::Cycle(offset)));
        }

        let index = usize::from(offset);
        let Some(&[id, next]) = self.bytes.get(index..).and_then(<[u8]>::first_chunk) else {
            return Some(Err(CapabilityError::Truncated(offset)));
        };
        self.visited |= bit;
        self.next = next;
        Some(Ok(Capability { offset, id }))
    }
}

impl FusedIterator for Capabilities<'_> {}

#[cfg(test)]
mod tests;
