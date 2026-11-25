//! Legacy PC devices whose I/O ports drivers may claim.
//!
//! A driver cannot create a `Port` itself (that is `unsafe`, D-010). Instead
//! it claims one of the devices below and gets a [`LegacyPorts`] object that
//! implements `kitty_hal::PortBlock` for **only** that device's ports. Each
//! device can be claimed once, so two drivers never fight over it.
//!
//! The allow-list contains devices that cannot perform DMA or change the
//! memory map, which is what makes handing out their ports safe.

use core::sync::atomic::{AtomicBool, Ordering};

use kitty_hal::PortBlock;

use super::port::Port;

/// Devices that can be claimed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LegacyDevice {
    /// 8042 PS/2 controller: data port 0x60 (offset 0) and status/command
    /// port 0x64 (offset 4). Port 0x61 (speaker/NMI control) is excluded.
    I8042,
    /// 8253/8254 PIT: channels 0–2 at 0x40–0x42, mode/command at 0x43.
    Pit,
    /// CMOS/RTC: index 0x70 (offset 0) and data 0x71 (offset 1).
    Cmos,
}

impl LegacyDevice {
    const ALL: [LegacyDevice; 3] = [LegacyDevice::I8042, LegacyDevice::Pit, LegacyDevice::Cmos];

    fn base(self) -> u16 {
        match self {
            LegacyDevice::I8042 => 0x60,
            LegacyDevice::Pit => 0x40,
            LegacyDevice::Cmos => 0x70,
        }
    }

    fn offsets(self) -> &'static [u16] {
        match self {
            LegacyDevice::I8042 => &[0, 4],
            LegacyDevice::Pit => &[0, 1, 2, 3],
            LegacyDevice::Cmos => &[0, 1],
        }
    }

    fn index(self) -> usize {
        match self {
            LegacyDevice::I8042 => 0,
            LegacyDevice::Pit => 1,
            LegacyDevice::Cmos => 2,
        }
    }
}

static CLAIMED: [AtomicBool; LegacyDevice::ALL.len()] = [
    AtomicBool::new(false),
    AtomicBool::new(false),
    AtomicBool::new(false),
];

/// Exclusive access to one legacy device's ports.
#[derive(Debug)]
pub struct LegacyPorts {
    device: LegacyDevice,
}

/// Claims `device`. Returns `None` if it was already claimed.
pub fn claim(device: LegacyDevice) -> Option<LegacyPorts> {
    let taken = CLAIMED[device.index()].swap(true, Ordering::AcqRel);
    (!taken).then_some(LegacyPorts { device })
}

impl LegacyPorts {
    /// Which device these ports belong to.
    pub fn device(&self) -> LegacyDevice {
        self.device
    }

    fn port(&self, offset: u16) -> Option<Port<u8>> {
        if !self.device.offsets().contains(&offset) {
            return None;
        }
        let port = self.device.base().checked_add(offset)?;
        // SAFETY: `port` is one of the allow-listed registers of a device
        // that has no DMA capability and cannot alter the memory map (PS/2
        // controller, PIT, CMOS). Accessing it in any order can at most
        // misconfigure that device.
        Some(unsafe { Port::new(port) })
    }
}

impl PortBlock for LegacyPorts {
    fn read8(&self, offset: u16) -> Option<u8> {
        self.port(offset).map(|p| p.read())
    }

    fn write8(&self, offset: u16, value: u8) -> Option<()> {
        self.port(offset).map(|p| p.write(value))
    }
}

impl Drop for LegacyPorts {
    fn drop(&mut self) {
        CLAIMED[self.device.index()].store(false, Ordering::Release);
    }
}
