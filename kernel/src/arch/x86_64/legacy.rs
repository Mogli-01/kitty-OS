//! Legacy PC devices whose I/O ports drivers may claim.
//!
//! A driver cannot create a `Port` itself (that is `unsafe`, D-010). Instead
//! it claims one of the devices below and gets a [`LegacyPorts`] object that
//! implements `kitty_hal::PortBlock` for **only** that device's ports. Each
//! device can be claimed once, so two drivers never fight over it.
//!
//! Two allow-lists make handing out these ports safe:
//! - **devices**: none of them can perform DMA;
//! - **values**: some registers of these devices *can* affect the whole
//!   machine, so writes to them are filtered too. The i8042 command port
//!   accepts "pulse output line" (0xF0–0xFF, 0xFE resets the CPU) and
//!   "write output port" (0xD1: bit 0 = system reset, bit 1 = A20 gate —
//!   turning A20 off aliases physical memory, i.e. memory corruption). The
//!   CMOS index port's bit 7 disables NMIs system-wide. A driver only gets
//!   the commands a PS/2 keyboard/mouse driver needs (found by the security
//!   review of this phase).

use core::sync::atomic::{AtomicBool, Ordering};

use kitty_hal::PortBlock;

use super::port::Port;

/// Devices that can be claimed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LegacyDevice {
    /// 8042 PS/2 controller: data port 0x60 (offset 0) and status/command
    /// port 0x64 (offset 4). Port 0x61 (speaker/NMI control) is excluded.
    /// Commands written to offset 4 must be in [`I8042_ALLOWED_COMMANDS`].
    I8042,
    /// 8253/8254 PIT: channels 0–2 at 0x40–0x42, mode/command at 0x43.
    Pit,
    /// CMOS/RTC: index 0x70 (offset 0) and data 0x71 (offset 1). Bit 7 of
    /// the index (NMI disable) is always written as 0.
    Cmos,
    /// Second serial port (COM2, 16550-compatible): 0x2F8-0x2FF, offsets
    /// 0-7; its interrupt is ISA IRQ 3 (`irq::register_isa(3, …)`). COM1
    /// is **not** claimable: it stays the kernel's diagnostics and panic
    /// channel (issue #72, D-061). A UART cannot do DMA and no register of
    /// it affects anything but the UART itself, so no value filter.
    Com2,
}

/// i8042 controller commands a driver may send (offset 4 = port 0x64):
/// read/write the configuration byte, disable/enable/test the two ports,
/// controller self-test, and "next data byte goes to the mouse" (0xD4).
/// Everything else (output-port writes, line pulses/reset, …) is refused.
pub const I8042_ALLOWED_COMMANDS: &[u8] =
    &[0x20, 0x60, 0xA7, 0xA8, 0xA9, 0xAA, 0xAB, 0xAD, 0xAE, 0xD4];

impl LegacyDevice {
    const ALL: [LegacyDevice; 4] = [
        LegacyDevice::I8042,
        LegacyDevice::Pit,
        LegacyDevice::Cmos,
        LegacyDevice::Com2,
    ];

    fn base(self) -> u16 {
        match self {
            LegacyDevice::I8042 => 0x60,
            LegacyDevice::Pit => 0x40,
            LegacyDevice::Cmos => 0x70,
            LegacyDevice::Com2 => 0x2F8,
        }
    }

    fn offsets(self) -> &'static [u16] {
        match self {
            LegacyDevice::I8042 => &[0, 4],
            LegacyDevice::Pit => &[0, 1, 2, 3],
            LegacyDevice::Cmos => &[0, 1],
            LegacyDevice::Com2 => &[0, 1, 2, 3, 4, 5, 6, 7],
        }
    }

    fn index(self) -> usize {
        match self {
            LegacyDevice::I8042 => 0,
            LegacyDevice::Pit => 1,
            LegacyDevice::Cmos => 2,
            LegacyDevice::Com2 => 3,
        }
    }
}

static CLAIMED: [AtomicBool; LegacyDevice::ALL.len()] = [
    AtomicBool::new(false),
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
        // that has no DMA capability (PS/2 controller, PIT, CMOS, COM2). Reads have
        // no system-wide effect. Writes that could affect the whole machine
        // (i8042 output port / reset / line pulses, CMOS NMI-disable bit) are
        // filtered by `write8` *before* reaching this port, so the remaining
        // accesses can at most misconfigure the device itself.
        Some(unsafe { Port::new(port) })
    }
}

impl PortBlock for LegacyPorts {
    fn read8(&self, offset: u16) -> Option<u8> {
        self.port(offset).map(|p| p.read())
    }

    fn write8(&self, offset: u16, value: u8) -> Option<()> {
        let value = match (self.device, offset) {
            (LegacyDevice::I8042, 4) if !I8042_ALLOWED_COMMANDS.contains(&value) => return None,
            (LegacyDevice::Cmos, 0) => value & 0x7F, // keep NMIs enabled
            _ => value,
        };
        self.port(offset).map(|p| p.write(value))
    }
}

impl Drop for LegacyPorts {
    fn drop(&mut self) {
        CLAIMED[self.device.index()].store(false, Ordering::Release);
    }
}

/// Boot self-test of the value filters (no hardware access for refused
/// writes: `write8` returns `None` before touching the port).
pub fn selftest() -> bool {
    let Some(kbd) = claim(LegacyDevice::I8042) else {
        return false;
    };
    let refused = [0xD1, 0xFE, 0xF0, 0xD2, 0xD3]
        .iter()
        .all(|&cmd| kbd.write8(4, cmd).is_none());
    let bad_offsets = kbd.read8(1).is_none() && kbd.write8(1, 0).is_none();
    let twice = claim(LegacyDevice::I8042).is_none();
    drop(kbd);
    let released = claim(LegacyDevice::I8042).is_some();
    // COM2: exactly its 8 registers, nothing past them.
    let com2 =
        claim(LegacyDevice::Com2).is_some_and(|c| c.read8(8).is_none() && c.write8(8, 0).is_none());
    refused && bad_offsets && twice && released && com2
}
