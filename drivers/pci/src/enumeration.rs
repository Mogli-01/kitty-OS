//! Walk the conventional PCI hierarchy described by read-only headers.

use crate::{ConfigSpace, HEADER_BYTES, Header, Layout, MULTIFUNCTION};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Address {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
}

/// Discover the functions reachable from bus 0 of segment 0. The reader
/// fills the entire header on `Ok(true)`, returns `Ok(false)` for an absent
/// function, or returns an error to stop the walk;
/// the visitor receives each successfully decoded header exactly once.
/// The queue is bounded by the 256 possible bus numbers.
pub fn enumerate<E>(
    mut read: impl FnMut(Address, &mut [u8; HEADER_BYTES]) -> Result<bool, E>,
    mut visit: impl FnMut(Address, Header),
) -> Result<(), E> {
    let mut visited = [false; 256];
    let mut queue = [0; 256];
    visited[0] = true;
    let mut next = 0;
    let mut queued = 1;
    let mut bytes = [0; HEADER_BYTES];

    while next < queued {
        let bus = queue[next];
        next += 1;
        for device in 0..32 {
            for function in 0..8 {
                let address = Address {
                    bus,
                    device,
                    function,
                };
                // A vendor ID of 1 is PCIe Configuration Request Retry
                // Status when software visibility is enabled.
                let present = read(address, &mut bytes)?
                    && !matches!(u16::from_le_bytes([bytes[0], bytes[1]]), 1 | u16::MAX);
                if present && let Ok(config) = ConfigSpace::parse(&bytes) {
                    let header = *config.header();
                    visit(address, header);
                    if let Layout::Bridge(bridge) = header.layout
                        && header.class == 6
                        && header.subclass == 4
                        && bridge.primary_bus == bus
                        && bridge.secondary_bus <= bridge.subordinate_bus
                        && !visited[usize::from(bridge.secondary_bus)]
                    {
                        visited[usize::from(bridge.secondary_bus)] = true;
                        // Each bus is enqueued once, including bus 0.
                        queue[queued] = bridge.secondary_bus;
                        queued += 1;
                    }
                }
                if function == 0 && (!present || bytes[0x0e] & MULTIFUNCTION == 0) {
                    break;
                }
            }
        }
    }
    Ok(())
}
