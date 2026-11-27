//! Non-blocking byte I/O for a 16550A UART through a granted port block.

#![no_std]
#![forbid(unsafe_code)]

use core::num::NonZeroU16;
use kitty_hal::PortBlock;

// DLAB selects the divisor latches at the first two offsets. With DLAB
// clear they refer to RBR/THR and IER respectively.
const DATA: u16 = 0;
const INTERRUPT_ENABLE: u16 = 1;
const INTERRUPT_IDENTIFICATION: u16 = 2;
const FIFO_CONTROL: u16 = 2;
const LINE_CONTROL: u16 = 3;
const MODEM_CONTROL: u16 = 4;
const LINE_STATUS: u16 = 5;

const LCR_8N1: u8 = 0x03;
const LCR_DLAB: u8 = 1 << 7;
const FCR_ENABLE_AND_CLEAR: u8 = 0x07;
const MCR_DTR_AND_RTS: u8 = 0x03;
const MCR_OUT2: u8 = 1 << 3;
const IER_RECEIVE: u8 = (1 << 0) | (1 << 2);
const IIR_NO_INTERRUPT: u8 = 1 << 0;
const IIR_CAUSE: u8 = 0x0e;

const LSR_DATA_READY: u8 = 1 << 0;
const LSR_OVERRUN: u8 = 1 << 1;
const LSR_PARITY: u8 = 1 << 2;
const LSR_FRAMING: u8 = 1 << 3;
const LSR_BREAK: u8 = 1 << 4;
const LSR_THR_EMPTY: u8 = 1 << 5;
const LSR_BYTE_ERRORS: u8 = LSR_PARITY | LSR_FRAMING | LSR_BREAK;
const LSR_RECEIVE_ERRORS: u8 = LSR_OVERRUN | LSR_BYTE_ERRORS;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    NotConfigured,
    PortRejected { offset: u16 },
    Receive(ReceiveErrors),
}

/// Errors observed since the last receive result. Several can occur together.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReceiveErrors {
    pub overrun: bool,
    pub parity: bool,
    pub framing: bool,
    pub break_signal: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InterruptCause {
    ModemStatus,
    TransmitterEmpty,
    ReceiveData,
    ReceiveLineStatus,
    ReceiveTimeout,
    Reserved(u8),
}

pub struct Uart16550<P> {
    ports: P,
    configured: bool,
    receive_errors: u8,
    invalid_head: bool,
}

impl<P: PortBlock> Uart16550<P> {
    /// Takes exclusive access to a known 16550A UART at offsets 0 through 5.
    /// No port is accessed until `configure` is called.
    pub const fn new(ports: P) -> Self {
        Self {
            ports,
            configured: false,
            receive_errors: 0,
            invalid_head: false,
        }
    }

    /// Configures 8 data bits, no parity and one stop bit. The baud rate is
    /// the input clock divided by 16 and by `divisor`. For a PC UART with
    /// a 1.8432 MHz clock, divisor 1 selects 115,200 baud.
    ///
    /// The caller keeps the interrupt line masked. The driver disables
    /// the UART's interrupt sources, clears both FIFOs, and drives DTR/RTS
    /// with loopback and OUT2 disabled. A rejected port operation may
    /// leave the hardware partly configured. I/O stays unavailable until
    /// a complete configuration succeeds.
    pub fn configure(&mut self, divisor: NonZeroU16) -> Result<(), Error> {
        self.configured = false;
        self.receive_errors = 0;
        self.invalid_head = false;

        self.write(LINE_CONTROL, LCR_8N1)?;
        self.write(INTERRUPT_ENABLE, 0)?;
        self.write(LINE_CONTROL, LCR_8N1 | LCR_DLAB)?;
        let [low, high] = divisor.get().to_le_bytes();
        let divisor_written = self
            .write(DATA, low)
            .and_then(|()| self.write(INTERRUPT_ENABLE, high));
        // Restore the data register bank even if one of the writes failed.
        let bank_restored = self.write(LINE_CONTROL, LCR_8N1);
        divisor_written?;
        bank_restored?;

        self.write(FIFO_CONTROL, FCR_ENABLE_AND_CLEAR)?;
        self.write(MODEM_CONTROL, MCR_DTR_AND_RTS)?;
        self.configured = true;
        Ok(())
    }

    /// Writes at most one byte. Returns `false` if the transmit holding
    /// register is busy, without waiting or modifying it.
    pub fn try_write(&mut self, byte: u8) -> Result<bool, Error> {
        if self.poll_status()? & LSR_THR_EMPTY == 0 {
            return Ok(false);
        }
        self.write(DATA, byte)?;
        Ok(true)
    }

    /// Reads at most one byte. Parity, framing and break errors discard the
    /// affected byte. An overrun alone reports earlier data loss and leaves
    /// the queued byte for the next call.
    pub fn try_read(&mut self) -> Result<Option<u8>, Error> {
        let status = self.poll_status()?;
        let data_ready = status & LSR_DATA_READY != 0;
        if self.receive_errors != 0 {
            if self.invalid_head && data_ready {
                self.read(DATA)?;
            }
            self.invalid_head = false;
            let errors = core::mem::take(&mut self.receive_errors);
            return Err(Error::Receive(ReceiveErrors {
                overrun: errors & LSR_OVERRUN != 0,
                parity: errors & LSR_PARITY != 0,
                framing: errors & LSR_FRAMING != 0,
                break_signal: errors & LSR_BREAK != 0,
            }));
        }
        if data_ready {
            self.read(DATA).map(Some)
        } else {
            Ok(None)
        }
    }

    /// Enables the UART's receive-data and line-status sources and the PC
    /// OUT2 gate. The caller serializes this with its interrupt handler
    /// and keeps the controller line masked until that handler is ready.
    /// On failure, the caller must keep the controller line masked.
    pub fn set_receive_interrupts(&mut self, enabled: bool) -> Result<(), Error> {
        self.ensure_configured()?;
        if !enabled {
            return self.disable_interrupts();
        }
        let result = self
            .write(INTERRUPT_ENABLE, IER_RECEIVE)
            .and_then(|()| self.write(MODEM_CONTROL, MCR_DTR_AND_RTS | MCR_OUT2));
        if result.is_err() {
            let _ = self.disable_interrupts();
        }
        result
    }

    /// Reads the highest-priority pending source once. A receive handler
    /// services data and line errors with `try_read` and leaves controller
    /// acknowledgement to its caller. Reading IIR can acknowledge a
    /// transmitter-empty source, which this driver does not enable.
    pub fn interrupt_cause(&self) -> Result<Option<InterruptCause>, Error> {
        self.ensure_configured()?;
        let identification = self.read(INTERRUPT_IDENTIFICATION)?;
        if identification & IIR_NO_INTERRUPT != 0 {
            return Ok(None);
        }
        let cause = match identification & IIR_CAUSE {
            0x00 => InterruptCause::ModemStatus,
            0x02 => InterruptCause::TransmitterEmpty,
            0x04 => InterruptCause::ReceiveData,
            0x06 => InterruptCause::ReceiveLineStatus,
            0x0c => InterruptCause::ReceiveTimeout,
            reserved => InterruptCause::Reserved(reserved),
        };
        Ok(Some(cause))
    }

    fn disable_interrupts(&self) -> Result<(), Error> {
        let gate = self.write(MODEM_CONTROL, MCR_DTR_AND_RTS);
        let sources = self.write(INTERRUPT_ENABLE, 0);
        gate.and(sources)
    }

    fn ensure_configured(&self) -> Result<(), Error> {
        if !self.configured {
            return Err(Error::NotConfigured);
        }
        Ok(())
    }

    fn poll_status(&mut self) -> Result<u8, Error> {
        self.ensure_configured()?;
        let status = self.read(LINE_STATUS)?;
        // Reading LSR acknowledges the receive error flags even when a
        // transmitter checked it. Keep them until the next receive result.
        self.receive_errors |= status & LSR_RECEIVE_ERRORS;
        self.invalid_head |= status & LSR_DATA_READY != 0 && status & LSR_BYTE_ERRORS != 0;
        Ok(status)
    }

    fn read(&self, offset: u16) -> Result<u8, Error> {
        self.ports
            .read8(offset)
            .ok_or(Error::PortRejected { offset })
    }

    fn write(&self, offset: u16, value: u8) -> Result<(), Error> {
        self.ports
            .write8(offset, value)
            .ok_or(Error::PortRejected { offset })
    }
}

#[cfg(test)]
mod tests;
