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
const ENABLE_AUXILIARY_PORT: u8 = 0xa8;
const ENABLE_KEYBOARD_PORT: u8 = 0xae;
const WRITE_AUXILIARY_DEVICE: u8 = 0xd4;

const KEYBOARD_IRQ: u8 = 1 << 0;
const AUXILIARY_IRQ: u8 = 1 << 1;
const SYSTEM_FLAG: u8 = 1 << 2;
const KEYBOARD_CLOCK_DISABLED: u8 = 1 << 4;
const AUXILIARY_CLOCK_DISABLED: u8 = 1 << 5;

const DISABLE_SCANNING: u8 = 0xf5;
const ENABLE_SCANNING: u8 = 0xf4;
const SCAN_CODE_SET: u8 = 0xf0;
const QUERY_CURRENT_SET: u8 = 0;
const SET_2: u8 = 2;
const GET_DEVICE_ID: u8 = 0xf2;
const SET_DEFAULTS: u8 = 0xf6;
const STANDARD_MOUSE_ID: u8 = 0;
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
    KeyboardReply,
    UnexpectedReply(u8),
    ResendLimit,
    KeyboardNotConfigured,
    KeyboardRestoreFailed,
    UnsupportedMouseId(u8),
}

pub struct I8042<P> {
    ports: P,
    keyboard_only_config: Option<u8>,
}

impl<P: PortBlock> I8042<P> {
    pub const fn new(ports: P) -> Self {
        Self {
            ports,
            keyboard_only_config: None,
        }
    }

    /// Selects untranslated Set 2 and enables the controller's keyboard IRQ.
    /// Call during device initialization with IRQ1 masked in the kernel's
    /// interrupt controller. The auxiliary port remains disabled.
    pub fn initialize_keyboard(&mut self) -> Result<(), Error> {
        self.keyboard_only_config = None;
        match self.configure_keyboard() {
            Ok(config) => {
                self.keyboard_only_config = Some(config);
                Ok(())
            }
            Err(error) => {
                // Keep a partially initialized keyboard from producing input.
                let _ = self.write(STATUS_COMMAND, DISABLE_KEYBOARD_PORT);
                Err(error)
            }
        }
    }

    /// Enables a standard three-byte PS/2 mouse after keyboard setup.
    /// Call at boot with both ISA IRQs masked and no active consumers. The
    /// keyboard clock is stopped and pending input is discarded during setup.
    /// `KeyboardRestoreFailed` means the caller must leave both IRQs masked.
    pub fn initialize_mouse(&mut self) -> Result<(), Error> {
        let config = self
            .keyboard_only_config
            .ok_or(Error::KeyboardNotConfigured)?;
        if let Err(error) = self.configure_mouse(config) {
            self.restore_keyboard(config)?;
            return Err(error);
        }
        Ok(())
    }

    /// Restores keyboard-only mode if IRQ12 could not be registered.
    /// Use during boot while consumers are still stopped.
    pub fn stop_mouse(&mut self) -> Result<(), Error> {
        let config = self
            .keyboard_only_config
            .ok_or(Error::KeyboardNotConfigured)?;
        self.restore_keyboard(config)
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

    fn configure_keyboard(&mut self) -> Result<u8, Error> {
        self.write(STATUS_COMMAND, DISABLE_KEYBOARD_PORT)?;
        self.write(STATUS_COMMAND, DISABLE_AUXILIARY_PORT)?;
        self.drain_output()?;

        self.write(STATUS_COMMAND, READ_CONFIG)?;
        let source = Source::Keyboard;
        let system_flag = self.reply(source)? & SYSTEM_FLAG;
        // Preserve the firmware's system flag. Clear translation, IRQs and
        // reserved bits while both ports remain disabled.
        self.write_config(system_flag | KEYBOARD_CLOCK_DISABLED | AUXILIARY_CLOCK_DISABLED)?;
        self.write(STATUS_COMMAND, ENABLE_KEYBOARD_PORT)?;

        self.command(source, DISABLE_SCANNING)?;
        self.command(source, SCAN_CODE_SET)?;
        self.command(source, SET_2)?;
        self.command(source, SCAN_CODE_SET)?;
        self.command(source, QUERY_CURRENT_SET)?;
        let current = self.reply(source)?;
        if current != SET_2 {
            return Err(Error::UnexpectedReply(current));
        }
        self.command(source, ENABLE_SCANNING)?;
        let config = system_flag | AUXILIARY_CLOCK_DISABLED | KEYBOARD_IRQ;
        self.write_config(config)?;
        Ok(config)
    }

    fn configure_mouse(&mut self, config: u8) -> Result<(), Error> {
        self.write(STATUS_COMMAND, DISABLE_KEYBOARD_PORT)?;
        self.write(STATUS_COMMAND, DISABLE_AUXILIARY_PORT)?;
        self.drain_output()?;
        self.write_config(
            (config & SYSTEM_FLAG) | KEYBOARD_CLOCK_DISABLED | AUXILIARY_CLOCK_DISABLED,
        )?;
        self.write(STATUS_COMMAND, ENABLE_AUXILIARY_PORT)?;
        let source = Source::Auxiliary;
        self.command(source, DISABLE_SCANNING)?;
        self.command(source, SET_DEFAULTS)?;
        self.command(source, GET_DEVICE_ID)?;
        let id = self.reply(source)?;
        if id != STANDARD_MOUSE_ID {
            return Err(Error::UnsupportedMouseId(id));
        }
        self.command(source, ENABLE_SCANNING)?;
        self.write(STATUS_COMMAND, ENABLE_KEYBOARD_PORT)?;
        self.write_config((config & !AUXILIARY_CLOCK_DISABLED) | AUXILIARY_IRQ)
    }

    fn restore_keyboard(&mut self, config: u8) -> Result<(), Error> {
        let drained = self
            .write(STATUS_COMMAND, DISABLE_AUXILIARY_PORT)
            .and_then(|()| self.drain_output());
        let restored = self.write_config(config);
        if drained.is_err() || restored.is_err() {
            self.keyboard_only_config = None;
            return Err(Error::KeyboardRestoreFailed);
        }
        Ok(())
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

    fn command(&mut self, source: Source, value: u8) -> Result<(), Error> {
        for _ in 0..COMMAND_ATTEMPTS {
            if source == Source::Auxiliary {
                self.write(STATUS_COMMAND, WRITE_AUXILIARY_DEVICE)?;
            }
            self.write(DATA, value)?;
            match self.reply(source)? {
                ACK => return Ok(()),
                RESEND => continue,
                other => return Err(Error::UnexpectedReply(other)),
            }
        }
        Err(Error::ResendLimit)
    }

    fn reply(&mut self, source: Source) -> Result<u8, Error> {
        for _ in 0..POLL_LIMIT {
            if let Some(reply) = self.try_read()? {
                if reply.source == source {
                    return Ok(reply.value);
                }
                return match reply.source {
                    Source::Keyboard => Err(Error::KeyboardReply),
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
