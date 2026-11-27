//! PCI configuration space, **read-only**, for enumeration (issue #70,
//! D-061).
//!
//! The kernel owns the legacy configuration mechanism (ports 0xCF8
//! `CONFIG_ADDRESS` and 0xCFC `CONFIG_DATA`). A transaction is two steps —
//! select an address, then read the data port — so two users interleaving
//! would read the wrong register; one lock makes each read atomic. Drivers
//! never get these ports:
//! - **no writes**: writing configuration space can move a device's BARs
//!   over RAM or enable bus mastering (DMA into kernel memory). Resource
//!   assignment comes in a later contract, with its own checks;
//! - reads have no side effects on conventional configuration registers.
//!
//! Scope: segment 0, buses 0-255, the first 256 bytes of each function
//! (conventional space). Extended space (ECAM/MCFG) comes later.

use super::port::Port;
use crate::sync::IrqSpinLock;

/// Address of one PCI function.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PciAddress {
    bus: u8,
    device: u8,
    function: u8,
}

impl PciAddress {
    /// `device` must be < 32 and `function` < 8.
    pub fn new(bus: u8, device: u8, function: u8) -> Result<Self, ConfigError> {
        if device >= 32 || function >= 8 {
            return Err(ConfigError::OutOfRange);
        }
        Ok(Self {
            bus,
            device,
            function,
        })
    }

    /// Bus number.
    pub fn bus(self) -> u8 {
        self.bus
    }

    /// Device number (0-31).
    pub fn device(self) -> u8 {
        self.device
    }

    /// Function number (0-7).
    pub fn function(self) -> u8 {
        self.function
    }

    fn config_address(self, offset: u8) -> u32 {
        (1 << 31)
            | (u32::from(self.bus) << 16)
            | (u32::from(self.device) << 11)
            | (u32::from(self.function) << 8)
            | u32::from(offset & 0xFC)
    }
}

/// Why a configuration read failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigError {
    /// Device or function number out of range, or a length other than
    /// 64 or 256 bytes.
    OutOfRange,
    /// No function at that address (vendor ID reads as 0xFFFF).
    Absent,
    /// The configuration mechanism is not available on this machine.
    Unavailable,
}

/// Bytes a caller may ask for: the standard header, or the whole
/// conventional space (header + capability lists).
pub const HEADER_LEN: usize = 64;
/// Whole conventional configuration space.
pub const CONVENTIONAL_LEN: usize = 256;

/// The two ports; `None` until probed, or if the mechanism is missing.
struct Mechanism {
    address: Port<u32>,
    data: Port<u32>,
}

static MECHANISM: IrqSpinLock<Option<Option<Mechanism>>> = IrqSpinLock::new(None);

fn probe() -> Option<Mechanism> {
    // SAFETY: 0xCF8 is CONFIG_ADDRESS of the PCI host bridge on every PC
    // with PCI. It is only ever accessed as a dword; chipsets with this
    // mechanism decode the neighbouring reset register (0xCF9) only for
    // byte accesses, so a dword write here only selects which configuration
    // register 0xCFC exposes. It cannot start DMA or change the memory map.
    let address = unsafe { Port::<u32>::new(0xCF8) };
    // SAFETY: 0xCFC is CONFIG_DATA. This module only ever *reads* it, and
    // reading conventional configuration registers has no side effects.
    let data = unsafe { Port::<u32>::new(0xCFC) };
    // The usual probe: the enable bit reads back on a machine with
    // mechanism #1; port 0xCF8 floats (all ones) or differs otherwise.
    // Restore the old value only if the mechanism exists: without it the
    // port floats (all ones), and writing 0xFF into the 0xCF9 byte lane
    // could reset a machine that decodes it at any width (security review).
    let saved = address.read();
    address.write(0x8000_0000);
    let present = address.read() == 0x8000_0000;
    if present {
        address.write(saved);
    }
    present.then_some(Mechanism { address, data })
}

/// Copies the first `dst.len()` bytes (64 or 256) of `addr`'s
/// configuration space into `dst`. The copy is taken under one lock, so it
/// is consistent with respect to other kernel readers.
pub fn read_config(addr: PciAddress, dst: &mut [u8]) -> Result<(), ConfigError> {
    if dst.len() != HEADER_LEN && dst.len() != CONVENTIONAL_LEN {
        return Err(ConfigError::OutOfRange);
    }
    let mut guard = MECHANISM.lock();
    let mechanism = guard
        .get_or_insert_with(probe)
        .as_ref()
        .ok_or(ConfigError::Unavailable)?;
    let read = |offset: u8| {
        mechanism.address.write(addr.config_address(offset));
        mechanism.data.read()
    };
    if read(0) & 0xFFFF == 0xFFFF {
        return Err(ConfigError::Absent);
    }
    for (i, chunk) in dst.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let offset = u8::try_from(i * 4).map_err(|_| ConfigError::OutOfRange)?;
        chunk.copy_from_slice(&read(offset).to_le_bytes());
    }
    Ok(())
}

/// Boot self-test: the host bridge at 00:00.0 exists, a function number
/// out of range is refused, a wrong length is refused, and an empty slot
/// reads as absent. `Ok(None)`: this machine has no mechanism #1.
/// `Err`: a check failed (a kernel bug, the caller panics).
pub fn selftest() -> Result<Option<u16>, &'static str> {
    let host = PciAddress::new(0, 0, 0).map_err(|_| "00:00.0 refused")?;
    let mut header = [0u8; HEADER_LEN];
    match read_config(host, &mut header) {
        Err(ConfigError::Unavailable) => return Ok(None),
        Err(_) => return Err("host bridge 00:00.0 not readable"),
        Ok(()) => {}
    }
    let vendor = u16::from_le_bytes([header[0], header[1]]);
    let bad_number = PciAddress::new(0, 32, 0) == Err(ConfigError::OutOfRange)
        && PciAddress::new(0, 0, 8) == Err(ConfigError::OutOfRange);
    let bad_len = read_config(host, &mut [0u8; 16]) == Err(ConfigError::OutOfRange);
    let empty = PciAddress::new(255, 31, 7).map(|a| read_config(a, &mut [0u8; HEADER_LEN]))
        == Ok(Err(ConfigError::Absent));
    if !bad_number {
        return Err("device/function numbers out of range were accepted");
    }
    if !bad_len {
        return Err("a length other than 64/256 was accepted");
    }
    if !empty {
        return Err("an empty slot did not read as absent");
    }
    Ok(Some(vendor))
}
