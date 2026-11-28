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

/// What a normal boot learned about PCI configuration access. Never a
/// failure: a machine without PCI, or without a device at 00:00.0, is not
/// a kernel bug (review of #92).
pub enum Probe {
    /// Mechanism #1 works; vendor ID of the host bridge at 00:00.0.
    HostBridge(u16),
    /// Mechanism #1 works but 00:00.0 did not answer.
    NoHostBridge,
    /// No configuration mechanism #1 on this machine.
    Unavailable,
}

/// Boot detection, tolerant of missing hardware.
pub fn probe_host_bridge() -> Probe {
    let Ok(host) = PciAddress::new(0, 0, 0) else {
        return Probe::NoHostBridge;
    };
    let mut header = [0u8; HEADER_LEN];
    match read_config(host, &mut header) {
        Ok(()) => Probe::HostBridge(u16::from_le_bytes([header[0], header[1]])),
        Err(ConfigError::Unavailable) => Probe::Unavailable,
        Err(_) => Probe::NoHostBridge,
    }
}

/// Boot self-test of the input checks, which hold on any machine: device
/// and function numbers out of range and lengths other than 64/256 are
/// refused before any port is touched.
pub fn validation_selftest() -> Result<(), &'static str> {
    let bad_number = PciAddress::new(0, 32, 0) == Err(ConfigError::OutOfRange)
        && PciAddress::new(0, 0, 8) == Err(ConfigError::OutOfRange);
    if !bad_number {
        return Err("device/function numbers out of range were accepted");
    }
    let any = PciAddress::new(0, 0, 0).map_err(|_| "00:00.0 refused")?;
    if read_config(any, &mut [0u8; 16]) != Err(ConfigError::OutOfRange) {
        return Err("a length other than 64/256 was accepted");
    }
    Ok(())
}

/// Topology self-test for the QEMU test matrix only (feature
/// `selftest-pci-topology`): on QEMU's q35/i440fx the host bridge answers
/// and 255:31.7 is empty. Real machines guarantee neither.
#[cfg(feature = "selftest-pci-topology")]
pub fn topology_selftest() -> Result<u16, &'static str> {
    let Probe::HostBridge(vendor) = probe_host_bridge() else {
        return Err("host bridge 00:00.0 not readable");
    };
    let empty = PciAddress::new(255, 31, 7).map(|a| read_config(a, &mut [0u8; HEADER_LEN]))
        == Ok(Err(ConfigError::Absent));
    if !empty {
        return Err("an empty slot did not read as absent");
    }
    Ok(vendor)
}
