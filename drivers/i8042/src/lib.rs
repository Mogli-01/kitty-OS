//! i8042 controller transport for the PS/2 keyboard.
//!
//! The kernel grants the controller ports through `PortBlock`. Initialization
//! uses bounded polling while IRQ1 is masked. The IRQ path reads at most one
//! output byte per call and distinguishes the keyboard and auxiliary ports.

#![no_std]
#![forbid(unsafe_code)]

use kitty_hal::PortBlock;

const DATA: u16 = 0;
const STATUS_COMMAND: u16 = 4;

const OUTPUT_FULL: u8 = 1 << 0;
const INPUT_FULL: u8 = 1 << 1;
const AUXILIARY_OUTPUT: u8 = 1 << 5;
const OUTPUT_ERROR: u8 = (1 << 6) | (1 << 7); // Timeout or parity error.

const READ_CONFIG: u8 = 0x20;
const WRITE_CONFIG: u8 = 0x60;
const DISABLE_KEYBOARD_PORT: u8 = 0xad;
const DISABLE_AUXILIARY_PORT: u8 = 0xa7;
const ENABLE_KEYBOARD_PORT: u8 = 0xae;

const KEYBOARD_IRQ: u8 = 1 << 0;
const SYSTEM_FLAG: u8 = 1 << 2;
const KEYBOARD_CLOCK_DISABLED: u8 = 1 << 4;
const AUXILIARY_CLOCK_DISABLED: u8 = 1 << 5;

const DISABLE_SCANNING: u8 = 0xf5;
const ENABLE_SCANNING: u8 = 0xf4;
const SCAN_CODE_SET: u8 = 0xf0;
const QUERY_CURRENT_SET: u8 = 0;
const SET_2: u8 = 2;
const ACK: u8 = 0xfa;
const RESEND: u8 = 0xfe;

const POLL_LIMIT: usize = 100_000;
const DRAIN_LIMIT: usize = 256;
const COMMAND_ATTEMPTS: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Keyboard,
    Auxiliary,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutputByte {
    pub source: Source,
    pub value: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    PortUnavailable,
    PollTimeout,
    OutputDidNotDrain,
    CorruptOutput(Source),
    AuxiliaryReply,
    UnexpectedReply(u8),
    ResendLimit,
}

pub struct I8042<P> {
    ports: P,
}

impl<P: PortBlock> I8042<P> {
    pub const fn new(ports: P) -> Self {
        Self { ports }
    }

    /// Selects untranslated Set 2 and enables the controller's keyboard IRQ.
    /// Call during device initialization with IRQ1 masked in the kernel's
    /// interrupt controller. The auxiliary port remains disabled.
    pub fn initialize_keyboard(&mut self) -> Result<(), Error> {
        let result = self.configure_keyboard();
        if result.is_err() {
            // Keep a partially initialized keyboard from producing input.
            let _ = self.write(STATUS_COMMAND, DISABLE_KEYBOARD_PORT);
        }
        result
    }

    /// Reads one available byte without waiting. A corrupt byte is consumed
    /// so the caller can reset the correct decoder and continue receiving.
    pub fn try_read(&mut self) -> Result<Option<OutputByte>, Error> {
        let status = self.read(STATUS_COMMAND)?;
        if status & OUTPUT_FULL == 0 {
            return Ok(None);
        }
        let source = if status & AUXILIARY_OUTPUT == 0 {
            Source::Keyboard
        } else {
            Source::Auxiliary
        };
        let value = self.read(DATA)?;
        if status & OUTPUT_ERROR != 0 {
            return Err(Error::CorruptOutput(source));
        }
        Ok(Some(OutputByte { source, value }))
    }

    fn configure_keyboard(&mut self) -> Result<(), Error> {
        self.write(STATUS_COMMAND, DISABLE_KEYBOARD_PORT)?;
        self.write(STATUS_COMMAND, DISABLE_AUXILIARY_PORT)?;
        self.drain_output()?;

        self.write(STATUS_COMMAND, READ_CONFIG)?;
        let system_flag = self.keyboard_reply()? & SYSTEM_FLAG;
        // Preserve the firmware's system flag. Clear translation, IRQs and
        // reserved bits while both ports remain disabled.
        self.write_config(system_flag | KEYBOARD_CLOCK_DISABLED | AUXILIARY_CLOCK_DISABLED)?;
        self.write(STATUS_COMMAND, ENABLE_KEYBOARD_PORT)?;

        self.keyboard_command(DISABLE_SCANNING)?;
        self.keyboard_command(SCAN_CODE_SET)?;
        self.keyboard_command(SET_2)?;
        self.keyboard_command(SCAN_CODE_SET)?;
        self.keyboard_command(QUERY_CURRENT_SET)?;
        let current = self.keyboard_reply()?;
        if current != SET_2 {
            return Err(Error::UnexpectedReply(current));
        }
        self.keyboard_command(ENABLE_SCANNING)?;
        self.write_config(system_flag | AUXILIARY_CLOCK_DISABLED | KEYBOARD_IRQ)
    }

    fn drain_output(&self) -> Result<(), Error> {
        for _ in 0..DRAIN_LIMIT {
            if self.read(STATUS_COMMAND)? & OUTPUT_FULL == 0 {
                return Ok(());
            }
            self.read(DATA)?;
        }
        if self.read(STATUS_COMMAND)? & OUTPUT_FULL == 0 {
            Ok(())
        } else {
            Err(Error::OutputDidNotDrain)
        }
    }

    fn keyboard_command(&mut self, value: u8) -> Result<(), Error> {
        for _ in 0..COMMAND_ATTEMPTS {
            self.write(DATA, value)?;
            match self.keyboard_reply()? {
                ACK => return Ok(()),
                RESEND => continue,
                other => return Err(Error::UnexpectedReply(other)),
            }
        }
        Err(Error::ResendLimit)
    }

    fn keyboard_reply(&mut self) -> Result<u8, Error> {
        for _ in 0..POLL_LIMIT {
            if let Some(reply) = self.try_read()? {
                return match reply.source {
                    Source::Keyboard => Ok(reply.value),
                    Source::Auxiliary => Err(Error::AuxiliaryReply),
                };
            }
            core::hint::spin_loop();
        }
        Err(Error::PollTimeout)
    }

    fn write_config(&self, value: u8) -> Result<(), Error> {
        self.write(STATUS_COMMAND, WRITE_CONFIG)?;
        self.write(DATA, value)
    }

    fn write(&self, offset: u16, value: u8) -> Result<(), Error> {
        for _ in 0..POLL_LIMIT {
            if self.read(STATUS_COMMAND)? & INPUT_FULL == 0 {
                return self
                    .ports
                    .write8(offset, value)
                    .ok_or(Error::PortUnavailable);
            }
            core::hint::spin_loop();
        }
        Err(Error::PollTimeout)
    }

    fn read(&self, offset: u16) -> Result<u8, Error> {
        self.ports.read8(offset).ok_or(Error::PortUnavailable)
    }
}
