//! Integrated xAPIC register operations and countdown timer.

use core::num::NonZeroU32;
use kitty_hal::RegisterBlock;

const ID: u64 = 0x020;
const VERSION: u64 = 0x030;
const TASK_PRIORITY: u64 = 0x080;
const EOI: u64 = 0x0b0;
const SPURIOUS: u64 = 0x0f0;
const IN_SERVICE: u64 = 0x100;
const REQUESTS: u64 = 0x200;
const TIMER: u64 = 0x320;
const LINT0: u64 = 0x350;
const LINT1: u64 = 0x360;
const INITIAL_COUNT: u64 = 0x380;
const CURRENT_COUNT: u64 = 0x390;
const DIVIDE: u64 = 0x3e0;

// LVT entries in the order counted by the Version register's Max LVT field.
const LOCAL_VECTORS: [u64; 7] = [TIMER, LINT0, LINT1, 0x370, 0x340, 0x330, 0x2f0];
pub const SPURIOUS_VECTOR: u8 = 0xff;
const SOFTWARE_ENABLED: u32 = 1 << 8;
const FOCUS_PROCESSOR: u32 = 1 << 9;
const SUPPRESS_EOI_BROADCAST: u32 = 1 << 12;
const SPURIOUS_CONFIG: u32 = SOFTWARE_ENABLED | SPURIOUS_VECTOR as u32;
const SPURIOUS_CONFIG_MASK: u32 = 0xff | SOFTWARE_ENABLED | SUPPRESS_EOI_BROADCAST;
const SPURIOUS_FIELDS: u32 = SPURIOUS_CONFIG_MASK | FOCUS_PROCESSOR;
const LVT_MASKED: u32 = 1 << 16;
const DELIVERY_MODE: u32 = 0b111 << 8;
const DELIVERY_NMI: u32 = 0b100 << 8;
const DELIVERY_EXTINT: u32 = 0b111 << 8;
const ACTIVE_LOW: u32 = 1 << 13;
const LEVEL_TRIGGERED: u32 = 1 << 15;
const LINT_FIELDS: u32 = 0xff | DELIVERY_MODE | ACTIVE_LOW | LEVEL_TRIGGERED | LVT_MASKED;
const TIMER_MODE: u32 = 0b11 << 17;
const TIMER_FIELDS: u32 = 0xff | LVT_MASKED | TIMER_MODE;
const DIVIDE_FIELDS: u32 = 0b1011;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Access { offset: u64 },
    ValueNotSet { offset: u64 },
    UnsupportedVersion(u8),
    UnsupportedMaxLvt(u8),
    ActiveLocalSource { offset: u64 },
    PendingInterrupts,
    UnrestorableRegister { offset: u64 },
    RollbackFailed,
    BroadcastId,
    InvalidVector,
    DeliveryNotEnabled,
    TimerUnconfigured,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Nmi {
    pub active_low: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TimerMode {
    OneShot,
    Periodic,
}

/// Divider encodings use bits 0, 1 and 3 of the xAPIC Divide register.
#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TimerDivider {
    By1 = 0b1011,
    By2 = 0b0000,
    By4 = 0b0001,
    By8 = 0b0010,
    By16 = 0b0011,
    By32 = 0b1000,
    By64 = 0b1001,
    By128 = 0b1010,
}

/// Registers of the local APIC for the executing CPU. The platform must
/// grant the validated UC xAPIC page, keep accesses on that CPU and
/// synchronize them with its interrupt handlers.
pub struct LocalApic<P> {
    registers: P,
    lvt_count: usize,
    timer_entry: Option<u32>,
}

impl<P: RegisterBlock> LocalApic<P> {
    /// Probes an integrated xAPIC. The platform establishes xAPIC mode and
    /// verifies the physical register mapping before giving out the block.
    pub fn probe(registers: P) -> Result<Self, Error> {
        let version = registers
            .read32(VERSION)
            .ok_or(Error::Access { offset: VERSION })?;
        let revision = version as u8;
        if !(0x10..=0x15).contains(&revision) {
            return Err(Error::UnsupportedVersion(revision));
        }
        let max_lvt = (version >> 16) as u8;
        if !(3..=6).contains(&max_lvt) {
            return Err(Error::UnsupportedMaxLvt(max_lvt));
        }
        Ok(Self {
            registers,
            lvt_count: usize::from(max_lvt) + 1,
            timer_entry: None,
        })
    }

    /// Reads the current CPU's unicast destination ID.
    pub fn id(&self) -> Result<u8, Error> {
        let id = (self.read(ID)? >> 24) as u8;
        if id == 0xff {
            return Err(Error::BroadcastId);
        }
        Ok(id)
    }

    /// Programs the software-enable bit and the reserved spurious vector.
    /// Call during controller setup with interrupts disabled. The platform
    /// quiesces the old external routes and installs any required handlers.
    /// Existing LINT NMIs are permitted and their entries are never written.
    pub fn enable_delivery(&mut self) -> Result<(), Error> {
        for &offset in &LOCAL_VECTORS[..self.lvt_count] {
            let lvt = self.read(offset)?;
            let lint_nmi = matches!(offset, LINT0 | LINT1) && lvt & DELIVERY_MODE == DELIVERY_NMI;
            if lvt & LVT_MASKED == 0 && !lint_nmi {
                return Err(Error::ActiveLocalSource { offset });
            }
        }
        // Leave EOI broadcasts enabled so level-triggered routes can complete.
        self.write(SPURIOUS, SPURIOUS_CONFIG)?;
        self.check(SPURIOUS, SPURIOUS_CONFIG_MASK, SPURIOUS_CONFIG)
    }

    /// Prepares the boot CPU to receive I/O APIC interrupts. The caller
    /// holds interrupts off and has already masked all external inputs.
    /// On success the caller replaces the legacy controller immediately.
    ///
    /// Enable the local APIC before unmasking LINT1. LINT0 is masked last.
    /// Restore the original routing if a write is rejected or ignored. If
    /// the spurious register was changed, restore its control bits with the
    /// kernel's reserved vector, since the capability can reject other vectors.
    pub fn enable_isa_delivery(&mut self, lint1_nmi: Option<Nmi>) -> Result<(), Error> {
        for &offset in &LOCAL_VECTORS[..self.lvt_count] {
            if !matches!(offset, LINT0 | LINT1) && self.read(offset)? & LVT_MASKED == 0 {
                return Err(Error::ActiveLocalSource { offset });
            }
        }
        let lint0 = self.read(LINT0)? & LINT_FIELDS;
        let lint1 = self.read(LINT1)? & LINT_FIELDS;
        if lint0 & LVT_MASKED == 0 && lint0 & DELIVERY_MODE != DELIVERY_EXTINT {
            return Err(Error::ActiveLocalSource { offset: LINT0 });
        }
        if lint1 & LVT_MASKED == 0 && lint1 & DELIVERY_MODE != DELIVERY_NMI {
            return Err(Error::ActiveLocalSource { offset: LINT1 });
        }
        let new_lint1 = lint1_nmi.map_or(lint1, |nmi| {
            DELIVERY_NMI | if nmi.active_low { ACTIVE_LOW } else { 0 }
        });
        if new_lint1 != lint1
            && !matches!(lint1 & DELIVERY_MODE, 0 | DELIVERY_NMI | DELIVERY_EXTINT)
        {
            return Err(Error::UnrestorableRegister { offset: LINT1 });
        }
        // Masking the I/O APIC does not retire vectors already accepted by
        // the local APIC. Do not let the core immediately reuse one of them.
        for index in 0..8 {
            let offset = index * 0x10;
            if self.read(IN_SERVICE + offset)? != 0 || self.read(REQUESTS + offset)? != 0 {
                return Err(Error::PendingInterrupts);
            }
        }
        // Firmware may leave the focus-processor bit set. Preserve it.
        let spurious = self.read(SPURIOUS)?;
        let changes = [
            (TASK_PRIORITY, self.read(TASK_PRIORITY)? & 0xff, 0, 0xff),
            (
                SPURIOUS,
                spurious & SPURIOUS_FIELDS,
                SPURIOUS_CONFIG | (spurious & FOCUS_PROCESSOR),
                SPURIOUS_FIELDS,
            ),
            (LINT1, lint1, new_lint1, LINT_FIELDS),
            (LINT0, lint0, lint0 | LVT_MASKED, LINT_FIELDS),
        ];
        for (index, &(offset, before, after, mask)) in changes.iter().enumerate() {
            if before == after {
                continue;
            }
            if let Err(error) = self
                .write(offset, after)
                .and_then(|()| self.check(offset, mask, after))
            {
                let mut restored = true;
                for &(offset, before, after, mask) in changes[..=index].iter().rev() {
                    if before != after && self.check(offset, mask, before).is_err() {
                        let before = if offset == SPURIOUS {
                            (before & !0xff) | u32::from(SPURIOUS_VECTOR)
                        } else {
                            before
                        };
                        restored &= self
                            .write(offset, before)
                            .and_then(|()| self.check(offset, mask, before))
                            .is_ok();
                    }
                }
                return Err(if restored {
                    error
                } else {
                    Error::RollbackFailed
                });
            }
        }
        Ok(())
    }

    /// Acknowledges a device interrupt after its handler returns. The
    /// spurious vector has no in-service entry to acknowledge.
    pub fn end_of_interrupt(&mut self, vector: u8) -> Result<(), Error> {
        match vector {
            SPURIOUS_VECTOR => Ok(()),
            0x20..=0xef => self.write(EOI, 0),
            _ => Err(Error::InvalidVector),
        }
    }

    /// Selects a countdown mode and dynamic device vector. The timer is
    /// stopped and the interrupt is masked until the caller enables it.
    /// Keep the previous handler for any interrupt already queued by the CPU.
    pub fn configure_timer(
        &mut self,
        vector: u8,
        mode: TimerMode,
        divide: TimerDivider,
    ) -> Result<(), Error> {
        if !(0x30..=0xef).contains(&vector) {
            return Err(Error::InvalidVector);
        }
        self.timer_entry = None;
        self.stop_timer()?;
        self.write(DIVIDE, divide as u32)?;
        self.check(DIVIDE, DIVIDE_FIELDS, divide as u32)?;
        let entry = u32::from(vector)
            | match mode {
                TimerMode::OneShot => 0,
                TimerMode::Periodic => 1 << 17,
            };
        self.write(TIMER, entry | LVT_MASKED)?;
        self.check(TIMER, TIMER_FIELDS, entry | LVT_MASKED)?;
        self.timer_entry = Some(entry);
        Ok(())
    }

    /// Enables countdown interrupts. Install the handler before calling.
    pub fn unmask_timer(&mut self) -> Result<(), Error> {
        let entry = self.timer_entry.ok_or(Error::TimerUnconfigured)?;
        if self.read(SPURIOUS)? & SPURIOUS_CONFIG_MASK != SPURIOUS_CONFIG {
            return Err(Error::DeliveryNotEnabled);
        }
        let result = self
            .check(TIMER, TIMER_FIELDS & !LVT_MASKED, entry)
            .and_then(|()| self.write(TIMER, entry))
            .and_then(|()| self.check(TIMER, TIMER_FIELDS, entry));
        if result.is_err() {
            self.timer_entry = None;
            let _ = self.stop_timer();
        }
        result
    }

    /// Starts or reloads the configured timer. A masked timer can be used
    /// for calibration by observing its current count.
    pub fn start_timer(&mut self, count: NonZeroU32) -> Result<(), Error> {
        self.timer_entry.ok_or(Error::TimerUnconfigured)?;
        self.write(INITIAL_COUNT, count.get())
    }

    /// Reads the remaining countdown ticks after division.
    pub fn current_count(&self) -> Result<u32, Error> {
        self.read(CURRENT_COUNT)
    }

    /// Masks timer interrupts and stops a one-shot or periodic countdown.
    /// Pending interrupts already accepted by the CPU still need their
    /// usual handler and EOI.
    pub fn stop_timer(&mut self) -> Result<(), Error> {
        let masked = self.mask_timer();
        let stopped = self.write(INITIAL_COUNT, 0);
        let result = masked.and(stopped);
        if result.is_err() {
            self.timer_entry = None;
        }
        result
    }

    fn mask_timer(&self) -> Result<(), Error> {
        let entry = self.read(TIMER)?;
        if entry & LVT_MASKED == 0 {
            self.write(TIMER, (entry & TIMER_FIELDS) | LVT_MASKED)?;
            self.check(TIMER, LVT_MASKED, LVT_MASKED)?;
        }
        Ok(())
    }

    fn check(&self, offset: u64, mask: u32, value: u32) -> Result<(), Error> {
        if self.read(offset)? & mask != value {
            return Err(Error::ValueNotSet { offset });
        }
        Ok(())
    }

    fn read(&self, offset: u64) -> Result<u32, Error> {
        self.registers
            .read32(offset)
            .ok_or(Error::Access { offset })
    }

    fn write(&self, offset: u64, value: u32) -> Result<(), Error> {
        self.registers
            .write32(offset, value)
            .ok_or(Error::Access { offset })
    }
}

#[cfg(test)]
mod tests;
