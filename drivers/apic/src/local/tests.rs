extern crate std;

use super::*;
use core::cell::{Cell, RefCell};
use std::vec::Vec;

struct Registers {
    values: [Cell<u32>; 64],
    writes: RefCell<Vec<(u64, u32)>>,
    reject_write: Cell<Option<u64>>,
    ignore_write: Cell<Option<u64>>,
}

impl Registers {
    fn new(version: u32) -> Self {
        let registers = Self {
            values: [const { Cell::new(0) }; 64],
            writes: RefCell::new(Vec::new()),
            reject_write: Cell::new(None),
            ignore_write: Cell::new(None),
        };
        registers.set(VERSION, version);
        for offset in LOCAL_VECTORS {
            registers.set(offset, LVT_MASKED);
        }
        registers
    }

    fn cell(&self, offset: u64) -> Option<&Cell<u32>> {
        if !offset.is_multiple_of(16) {
            return None;
        }
        self.values.get(usize::try_from(offset / 16).ok()?)
    }

    fn set(&self, offset: u64, value: u32) {
        self.cell(offset).unwrap().set(value);
    }
}

impl RegisterBlock for Registers {
    fn read32(&self, offset: u64) -> Option<u32> {
        Some(self.cell(offset)?.get())
    }

    fn write32(&self, offset: u64, value: u32) -> Option<()> {
        let register = self.cell(offset)?;
        self.writes.borrow_mut().push((offset, value));
        if self.reject_write.get() == Some(offset) {
            return None;
        }
        if self.ignore_write.get() != Some(offset) {
            register.set(value);
        }
        Some(())
    }

    fn read64(&self, _: u64) -> Option<u64> {
        None
    }

    fn write64(&self, _: u64, _: u64) -> Option<()> {
        None
    }
}

fn device() -> LocalApic<Registers> {
    LocalApic::probe(Registers::new(0x0006_0014)).unwrap()
}

#[test]
fn validates_revision_lvt_count_and_destination() {
    for (version, error) in [
        (0x0006_000f, Error::UnsupportedVersion(0x0f)),
        (0x0006_0016, Error::UnsupportedVersion(0x16)),
        (0x0002_0014, Error::UnsupportedMaxLvt(2)),
        (0x0007_0014, Error::UnsupportedMaxLvt(7)),
    ] {
        assert!(matches!(LocalApic::probe(Registers::new(version)), Err(e) if e == error));
    }

    let apic = device();
    apic.registers.set(ID, 0x3d00_0000);
    assert_eq!(apic.id(), Ok(0x3d));
    apic.registers.set(ID, 0xff00_0000);
    assert_eq!(apic.id(), Err(Error::BroadcastId));
}

#[test]
fn delivery_preserves_nmis_and_requires_other_sources_to_be_masked() {
    let mut apic = device();
    let lint_nmi = DELIVERY_NMI | (1 << 13);
    apic.registers.set(LINT1, lint_nmi);
    for (offset, entry) in [(LINT0, 0b111 << 8), (TIMER, 0x40), (0x2f0, 0x42)] {
        apic.registers.set(offset, entry);
        assert_eq!(
            apic.enable_delivery(),
            Err(Error::ActiveLocalSource { offset })
        );
        assert!(apic.registers.writes.borrow().is_empty());
        apic.registers.set(offset, LVT_MASKED);
    }

    apic.enable_delivery().unwrap();
    assert_eq!(*apic.registers.writes.borrow(), [(0x0f0, 0x1ff)]);
    assert_eq!(apic.registers.read32(LINT1), Some(lint_nmi));
}

#[test]
fn checks_that_eoi_broadcast_is_enabled() {
    let mut apic = device();
    apic.registers.set(SPURIOUS, 0x11ff);
    apic.registers.ignore_write.set(Some(SPURIOUS));
    assert_eq!(
        apic.enable_delivery(),
        Err(Error::ValueNotSet { offset: SPURIOUS })
    );
}

#[test]
fn acknowledges_device_vectors_only() {
    let mut apic = device();
    apic.end_of_interrupt(SPURIOUS_VECTOR).unwrap();
    for vector in [0, 0x1f, 0xf0, 0xfe] {
        assert_eq!(apic.end_of_interrupt(vector), Err(Error::InvalidVector));
    }
    assert!(apic.registers.writes.borrow().is_empty());
    for vector in [0x20, 0xef] {
        apic.end_of_interrupt(vector).unwrap();
    }
    assert_eq!(*apic.registers.writes.borrow(), [(0x0b0, 0), (0x0b0, 0)]);
}

#[test]
fn validates_timer_vectors_and_masks_before_reconfiguring() {
    let mut apic = device();
    for vector in [0x2f, 0xf0, 0xff] {
        assert_eq!(
            apic.configure_timer(vector, TimerMode::Periodic, TimerDivider::By32),
            Err(Error::InvalidVector)
        );
    }
    assert!(apic.registers.writes.borrow().is_empty());
    apic.registers.set(TIMER, 0x35);
    apic.configure_timer(0x40, TimerMode::Periodic, TimerDivider::By32)
        .unwrap();
    assert_eq!(
        *apic.registers.writes.borrow(),
        [(0x320, 0x10035), (0x380, 0), (0x3e0, 8), (0x320, 0x30040)]
    );
    assert_eq!(apic.unmask_timer(), Err(Error::DeliveryNotEnabled));
    apic.enable_delivery().unwrap();
    apic.unmask_timer().unwrap();
    assert_eq!(apic.registers.read32(TIMER), Some(0x20040));
}

#[test]
fn programs_all_dividers_and_can_reload_after_stopping() {
    let mut apic = device();
    let count = NonZeroU32::new(200).unwrap();
    assert_eq!(apic.start_timer(count), Err(Error::TimerUnconfigured));
    for (divide, encoding) in [
        (TimerDivider::By1, 11),
        (TimerDivider::By2, 0),
        (TimerDivider::By4, 1),
        (TimerDivider::By8, 2),
        (TimerDivider::By16, 3),
        (TimerDivider::By32, 8),
        (TimerDivider::By64, 9),
        (TimerDivider::By128, 10),
    ] {
        apic.configure_timer(0x30, TimerMode::OneShot, divide)
            .unwrap();
        assert_eq!(apic.registers.read32(DIVIDE), Some(encoding));
        assert_eq!(apic.registers.read32(TIMER), Some(0x10030));
    }
    apic.start_timer(count).unwrap();
    apic.registers.set(CURRENT_COUNT, 180);
    assert_eq!(apic.current_count(), Ok(180));
    apic.stop_timer().unwrap();
    apic.start_timer(count).unwrap();
    assert!(
        apic.registers
            .writes
            .borrow()
            .ends_with(&[(0x380, 200), (0x380, 0), (0x380, 200)])
    );
}

#[test]
fn incomplete_timer_configuration_cannot_be_started_or_unmasked() {
    for offset in [DIVIDE, TIMER] {
        let mut apic = device();
        apic.enable_delivery().unwrap();
        apic.registers.ignore_write.set(Some(offset));
        assert_eq!(
            apic.configure_timer(0x40, TimerMode::Periodic, TimerDivider::By32),
            Err(Error::ValueNotSet { offset })
        );
        assert_eq!(apic.unmask_timer(), Err(Error::TimerUnconfigured));
        assert_ne!(apic.registers.read32(TIMER).unwrap() & LVT_MASKED, 0);
        assert_eq!(
            apic.start_timer(NonZeroU32::MIN),
            Err(Error::TimerUnconfigured)
        );
    }
}

#[test]
fn failed_unmask_or_stop_invalidates_timer_configuration() {
    let mut apic = device();
    apic.enable_delivery().unwrap();
    apic.configure_timer(0x45, TimerMode::OneShot, TimerDivider::By1)
        .unwrap();
    apic.start_timer(NonZeroU32::MIN).unwrap();
    apic.registers.ignore_write.set(Some(TIMER));
    assert_eq!(
        apic.unmask_timer(),
        Err(Error::ValueNotSet { offset: TIMER })
    );
    assert_eq!(apic.registers.read32(INITIAL_COUNT), Some(0));
    assert_eq!(
        apic.start_timer(NonZeroU32::MIN),
        Err(Error::TimerUnconfigured)
    );

    apic.registers.ignore_write.set(None);
    apic.configure_timer(0x45, TimerMode::OneShot, TimerDivider::By1)
        .unwrap();
    apic.unmask_timer().unwrap();
    apic.registers.reject_write.set(Some(TIMER));
    assert_eq!(apic.stop_timer(), Err(Error::Access { offset: TIMER }));
    assert!(
        apic.registers
            .writes
            .borrow()
            .ends_with(&[(TIMER, 0x10045), (INITIAL_COUNT, 0)])
    );
    assert_eq!(apic.unmask_timer(), Err(Error::TimerUnconfigured));
}
