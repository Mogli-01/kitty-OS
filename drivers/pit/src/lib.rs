//! Periodic interrupt source using channel 0 of the legacy 8254 PIT.

#![no_std]
#![forbid(unsafe_code)]

use kitty_hal::PortBlock;

/// Nominal frequency of the PC PIT input clock, in hertz.
pub const INPUT_HZ: u32 = 1_193_182;
const MAX_DIVISOR: u32 = 65_536;
const CHANNEL_0: u16 = 0;
const CONTROL: u16 = 3;
// Select channel 0, write LSB then MSB, mode 2 (rate generator), binary count.
const PERIODIC: u8 = 0x34;

/// Reload count for the mode 2 rate generator.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Period(u32);

impl Period {
    /// Constructs a period directly from a legal binary reload count.
    pub const fn from_divisor(divisor: u32) -> Option<Self> {
        if divisor >= 2 && divisor <= MAX_DIVISOR {
            Some(Self(divisor))
        } else {
            None
        }
    }

    /// Approximates an in-range integer frequency to the nearest clock period.
    ///
    /// The supported requests are 19 through 596,591 Hz, inclusive. At a
    /// half-clock tie the result selects the longer period.
    pub const fn from_hz(hz: u32) -> Option<Self> {
        if hz < INPUT_HZ.div_ceil(MAX_DIVISOR) || hz > INPUT_HZ / 2 {
            return None;
        }
        Self::from_divisor((INPUT_HZ + hz / 2) / hz)
    }

    /// Clock edges per IRQ. The actual rate is `INPUT_HZ / divisor()` Hz.
    pub const fn divisor(self) -> u32 {
        self.0
    }
}

/// The granted port block refused a write.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PortRejected {
    pub offset: u16,
}

/// Owns access to the PIT ports for as long as the timer is in use.
pub struct Pit<P> {
    ports: P,
}

impl<P: PortBlock> Pit<P> {
    /// Receives a port block based at 0x40 and covering offsets 0 and 3.
    pub const fn new(ports: P) -> Self {
        Self { ports }
    }

    /// Programs channel 0. Call with IRQ0 masked, then install the handler
    /// and unmask the line. A pending IRQ can arrive before the first period.
    ///
    /// An error can leave the counter partially configured. Keep IRQ0 masked
    /// until a complete programming sequence succeeds.
    pub fn start_periodic(&mut self, period: Period) -> Result<(), PortRejected> {
        // A reload of 65,536 is encoded as zero in binary mode.
        let [low, high] = (period.divisor() as u16).to_le_bytes();
        self.write(CONTROL, PERIODIC)?;
        self.write(CHANNEL_0, low)?;
        self.write(CHANNEL_0, high)
    }

    fn write(&mut self, offset: u16, value: u8) -> Result<(), PortRejected> {
        self.ports
            .write8(offset, value)
            .ok_or(PortRejected { offset })
    }
}

#[cfg(test)]
mod tests;
