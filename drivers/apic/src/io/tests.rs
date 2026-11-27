extern crate std;

use super::*;
use core::cell::{Cell, RefCell};
use std::vec::Vec;

struct Registers {
    values: [Cell<u32>; 256],
    selected: Cell<u8>,
    writes: RefCell<Vec<(u8, u32)>>,
    reject_write: Cell<Option<u8>>,
    ignore_write: Cell<Option<u8>>,
}

impl Registers {
    fn new(version: u32) -> Self {
        let registers = Self {
            values: [const { Cell::new(0) }; 256],
            selected: Cell::new(0),
            writes: RefCell::new(Vec::new()),
            reject_write: Cell::new(None),
            ignore_write: Cell::new(None),
        };
        registers.values[usize::from(VERSION)].set(version);
        registers
    }
}

impl RegisterBlock for Registers {
    fn read32(&self, offset: u64) -> Option<u32> {
        (offset == WINDOW).then(|| self.values[usize::from(self.selected.get())].get())
    }

    fn write32(&self, offset: u64, value: u32) -> Option<()> {
        match offset {
            SELECT => self.selected.set(u8::try_from(value).ok()?),
            WINDOW => {
                let index = self.selected.get();
                if self.reject_write.get() == Some(index) {
                    return None;
                }
                if self.ignore_write.get() != Some(index) {
                    self.values[usize::from(index)].set(value);
                }
                self.writes.borrow_mut().push((index, value));
            }
            _ => return None,
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

const ROUTE: Route = Route {
    vector: 0x41,
    destination_id: 0x05,
    active_low: true,
    level_triggered: true,
};

fn device() -> IoApic<Registers> {
    IoApic::probe(Registers::new(0x0017_0020), 32).unwrap()
}

#[test]
fn validates_version_pin_count_and_gsi_range() {
    for (version, base, error) in [
        (0, 0, Error::InvalidVersion),
        (u32::MAX, 0, Error::InvalidVersion),
        (0x0078_0020, 0, Error::InvalidInputCount),
        (0x0001_0020, u32::MAX, Error::GsiOverflow),
    ] {
        assert!(matches!(IoApic::probe(Registers::new(version), base), Err(e) if e == error));
    }
    assert_eq!(device().gsis(), 32..=55);
}

#[test]
fn preserves_active_legacy_extint_during_handoff_preflight() {
    let mut apic = device();
    let index = usize::from(redirection(2));
    apic.registers.values[index].set(0b111 << 8);
    assert_eq!(apic.check_legacy_handoff(), Err(Error::ActiveSpecialRoute));
    assert!(apic.registers.writes.borrow().is_empty());
    apic.registers.values[index].set(MASKED | (0b111 << 8));
    assert_eq!(apic.check_legacy_handoff(), Ok(()));
}

#[test]
fn refuses_handoff_with_old_delivery_in_flight() {
    let mut apic = device();
    let index = usize::from(redirection(0));
    apic.registers.values[index].set(DELIVERY_PENDING);
    assert_eq!(apic.quiesce_for_handoff(), Err(Error::InFlight));
    assert_ne!(apic.registers.values[index].get() & MASKED, 0);
}

#[test]
fn rejects_bad_routes_without_writing_the_table() {
    let mut apic = device();
    for gsi in [31, 56, u32::MAX] {
        assert_eq!(
            apic.configure_masked(gsi, ROUTE),
            Err(Error::GsiOutsideController)
        );
    }
    for vector in [0, 0x1f, 0xf0, 0xff] {
        assert_eq!(
            apic.configure_masked(32, Route { vector, ..ROUTE }),
            Err(Error::InvalidVector)
        );
    }
    assert_eq!(
        apic.configure_masked(
            32,
            Route {
                destination_id: 0xff,
                ..ROUTE
            }
        ),
        Err(Error::BroadcastDestination)
    );
    assert!(apic.registers.writes.borrow().is_empty());
}

#[test]
fn masks_the_old_route_before_changing_destination_or_vector() {
    let mut apic = device();
    apic.registers.values[0x16].set(0x32);
    assert_eq!(apic.unmask(35), Err(Error::Unconfigured));
    apic.configure_masked(35, ROUTE).unwrap();
    assert_eq!(
        *apic.registers.writes.borrow(),
        [(0x16, 0x10032), (0x17, 0x0500_0000), (0x16, 0x1a041)]
    );
    apic.unmask(35).unwrap();
    assert_eq!(apic.registers.values[0x16].get(), 0xa041);
    apic.mask(35).unwrap();
    assert_eq!(apic.registers.values[0x16].get(), 0x1a041);
}

#[test]
fn rejects_reconfiguration_of_an_irq_still_in_flight() {
    for pending in [REMOTE_IRR, DELIVERY_PENDING] {
        let mut apic = device();
        apic.registers.values[0x10].set(pending | LEVEL | 0x31);
        assert_eq!(apic.configure_masked(32, ROUTE), Err(Error::InFlight));
        assert_eq!(
            *apic.registers.writes.borrow(),
            [(0x10, MASKED | pending | LEVEL | 0x31)]
        );
        assert_eq!(apic.unmask(32), Err(Error::Unconfigured));
    }
}

#[test]
fn never_rewrites_destination_if_mask_write_did_not_take_effect() {
    let mut apic = device();
    apic.registers.ignore_write.set(Some(0x10));
    assert_eq!(apic.configure_masked(32, ROUTE), Err(Error::MaskNotSet));
    assert_eq!(*apic.registers.writes.borrow(), [(0x10, MASKED)]);
}

#[test]
fn refuses_to_unmask_if_readback_differs_from_the_route() {
    for index in [0x10, 0x11] {
        let mut apic = device();
        apic.registers.values[0x10].set(MASKED);
        apic.registers.ignore_write.set(Some(index));
        assert_eq!(apic.configure_masked(32, ROUTE), Err(Error::RouteNotSet));
        assert_eq!(apic.unmask(32), Err(Error::Unconfigured));
        assert_ne!(apic.registers.values[0x10].get() & MASKED, 0);
    }
}

#[test]
fn a_partial_route_write_requires_reconfiguration_before_unmasking() {
    let mut apic = device();
    apic.configure_masked(32, ROUTE).unwrap();
    let update = Route {
        vector: 0x42,
        destination_id: 0x03,
        ..ROUTE
    };
    // The mask is already set. Accept the new destination but deny the vector.
    apic.registers.reject_write.set(Some(0x10));
    assert_eq!(
        apic.configure_masked(32, update),
        Err(Error::Access { offset: WINDOW })
    );
    assert_eq!(apic.registers.values[0x10].get(), 0x1a041);
    assert_eq!(apic.registers.values[0x11].get(), 0x0300_0000);
    assert_eq!(apic.unmask(32), Err(Error::Unconfigured));

    apic.registers.reject_write.set(None);
    apic.configure_masked(32, update).unwrap();
    apic.unmask(32).unwrap();
    assert_eq!(apic.registers.values[0x10].get(), 0xa042);
}

#[test]
fn masks_all_pins_without_overflow_at_last_hardware_index() {
    let base = u32::MAX - 119;
    let mut apic = IoApic::probe(Registers::new(0x0077_0020), base).unwrap();
    assert_eq!(apic.gsis(), base..=u32::MAX);
    apic.mask_all().unwrap();
    let writes = apic.registers.writes.borrow();
    assert_eq!(writes.len(), 120);
    assert_eq!(writes.first(), Some(&(0x10, MASKED)));
    assert_eq!(writes.last(), Some(&(0xfe, MASKED)));
    drop(writes);
    apic.configure_masked(u32::MAX, ROUTE).unwrap();
    assert_eq!(apic.registers.values[0xff].get(), 0x0500_0000);
}
