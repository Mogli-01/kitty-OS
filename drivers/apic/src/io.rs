//! I/O APIC register access and fixed, physical-destination IRQ routing.

use core::ops::RangeInclusive;
use kitty_hal::RegisterBlock;

const SELECT: u64 = 0x00;
const WINDOW: u64 = 0x10;
const VERSION: u8 = 0x01;
const REDIRECTION_START: u8 = 0x10;
const MAX_INPUTS: u8 = (u8::MAX - REDIRECTION_START) / 2 + 1;

const DELIVERY_PENDING: u32 = 1 << 12;
const ACTIVE_LOW: u32 = 1 << 13;
const REMOTE_IRR: u32 = 1 << 14;
const LEVEL: u32 = 1 << 15;
const MASKED: u32 = 1 << 16;
const ROUTE_BITS: u32 = MASKED | LEVEL | ACTIVE_LOW | 0x0fff;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Access { offset: u64 },
    InvalidVersion,
    InvalidInputCount,
    GsiOverflow,
    GsiOutsideController,
    InvalidVector,
    BroadcastDestination,
    MaskNotSet,
    InFlight,
    RouteNotSet,
    Unconfigured,
}

/// A fixed-delivery route to a single xAPIC destination.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Route {
    pub vector: u8,
    pub destination_id: u8,
    pub active_low: bool,
    pub level_triggered: bool,
}

/// Exclusively owns the selector/window register pair of one I/O APIC.
///
/// Hold exclusive access across each operation. The platform must grant one
/// block per MMIO device and serialize access shared with interrupt handlers.
pub struct IoApic<P> {
    registers: P,
    gsi_base: u32,
    max_pin: u8,
    configured: u128,
}

impl<P: RegisterBlock> IoApic<P> {
    /// Reads the version register and validates the table size and GSI range.
    /// The block must address the I/O APIC's UC register page.
    pub fn probe(registers: P, gsi_base: u32) -> Result<Self, Error> {
        let mut apic = Self {
            registers,
            gsi_base,
            max_pin: 0,
            configured: 0,
        };
        let version = apic.read(VERSION)?;
        if matches!(version as u8, 0 | 0xff) {
            return Err(Error::InvalidVersion);
        }
        let max_pin = (version >> 16) as u8;
        if max_pin >= MAX_INPUTS {
            return Err(Error::InvalidInputCount);
        }
        gsi_base
            .checked_add(u32::from(max_pin))
            .ok_or(Error::GsiOverflow)?;
        apic.max_pin = max_pin;
        Ok(apic)
    }

    /// GSIs handled by this controller. The platform must reject overlapping
    /// ranges before assigning routes across multiple I/O APICs.
    pub fn gsis(&self) -> RangeInclusive<u32> {
        self.gsi_base..=self.gsi_base + u32::from(self.max_pin)
    }

    /// Masks every input, including routes left configured by the firmware.
    /// On error, keep the controller out of service until a full pass succeeds.
    pub fn mask_all(&mut self) -> Result<(), Error> {
        for pin in 0..=self.max_pin {
            self.mask_pin(pin)?;
        }
        Ok(())
    }

    /// Masks the input. Any in-flight interrupt still needs its normal EOI.
    pub fn mask(&mut self, gsi: u32) -> Result<(), Error> {
        self.mask_pin(self.pin(gsi)?).map(|_| ())
    }

    /// Installs a new route, leaving the input masked. The caller must keep
    /// the old handler installed until any interrupt already in the CPU's
    /// queue has been handled.
    ///
    /// A pending delivery or remote IRR leaves the input masked and requires
    /// the caller to drain the previous route before retrying.
    pub fn configure_masked(&mut self, gsi: u32, route: Route) -> Result<(), Error> {
        let pin = self.pin(gsi)?;
        if !(0x20..=0xef).contains(&route.vector) {
            return Err(Error::InvalidVector);
        }
        if route.destination_id == 0xff {
            return Err(Error::BroadcastDestination);
        }

        let bit = 1 << pin;
        self.configured &= !bit;
        let before = self.mask_pin(pin)?;
        if before & (REMOTE_IRR | DELIVERY_PENDING) != 0 {
            return Err(Error::InFlight);
        }

        let mut low = u32::from(route.vector) | MASKED;
        if route.active_low {
            low |= ACTIVE_LOW;
        }
        if route.level_triggered {
            low |= LEVEL;
        }
        let high = u32::from(route.destination_id) << 24;
        let index = redirection(pin);
        self.write(index + 1, high)?;
        self.write(index, low)?;
        if self.read(index + 1)? >> 24 != u32::from(route.destination_id)
            || self.read(index)? & ROUTE_BITS != low
        {
            return Err(Error::RouteNotSet);
        }
        self.configured |= bit;
        Ok(())
    }

    /// Unmasks a route that this driver configured successfully. Install the
    /// corresponding IRQ handler before calling this method.
    pub fn unmask(&mut self, gsi: u32) -> Result<(), Error> {
        let pin = self.pin(gsi)?;
        if self.configured & (1 << pin) == 0 {
            return Err(Error::Unconfigured);
        }
        let index = redirection(pin);
        let low = self.read(index)?;
        self.write(index, low & !MASKED)
    }

    fn pin(&self, gsi: u32) -> Result<u8, Error> {
        let index = gsi
            .checked_sub(self.gsi_base)
            .ok_or(Error::GsiOutsideController)?;
        if index > u32::from(self.max_pin) {
            return Err(Error::GsiOutsideController);
        }
        Ok(index as u8)
    }

    fn mask_pin(&mut self, pin: u8) -> Result<u32, Error> {
        let index = redirection(pin);
        let before = self.read(index)?;
        if before & MASKED != 0 {
            return Ok(before);
        }
        self.write(index, before | MASKED)?;
        let masked = self.read(index)?;
        if masked & MASKED == 0 {
            return Err(Error::MaskNotSet);
        }
        Ok(masked)
    }

    fn read(&mut self, index: u8) -> Result<u32, Error> {
        self.select(index)?;
        self.registers
            .read32(WINDOW)
            .ok_or(Error::Access { offset: WINDOW })
    }

    fn write(&mut self, index: u8, value: u32) -> Result<(), Error> {
        self.select(index)?;
        self.registers
            .write32(WINDOW, value)
            .ok_or(Error::Access { offset: WINDOW })
    }

    fn select(&mut self, index: u8) -> Result<(), Error> {
        self.registers
            .write32(SELECT, u32::from(index))
            .ok_or(Error::Access { offset: SELECT })
    }
}

const fn redirection(pin: u8) -> u8 {
    REDIRECTION_START + pin * 2
}

#[cfg(test)]
mod tests;
