extern crate std;

use super::*;
use core::cell::RefCell;
use std::vec::Vec;

#[derive(Default)]
struct Ports {
    writes: RefCell<Vec<(u16, u8)>>,
    reject_at: Option<usize>,
}

impl PortBlock for Ports {
    fn read8(&self, _: u16) -> Option<u8> {
        panic!("programming channel 0 needs no reads");
    }

    fn write8(&self, offset: u16, value: u8) -> Option<()> {
        let mut writes = self.writes.borrow_mut();
        let accepted = self.reject_at != Some(writes.len());
        writes.push((offset, value));
        accepted.then_some(())
    }
}

#[test]
fn validates_reload_counts() {
    for divisor in [0, 1, 65_537, u32::MAX] {
        assert_eq!(Period::from_divisor(divisor), None);
    }
    for divisor in [2, 65_535, 65_536] {
        assert_eq!(Period::from_divisor(divisor).unwrap().divisor(), divisor);
    }
}

#[test]
fn rounds_to_the_nearest_clock_period() {
    for hz in [0, 18, 596_592, u32::MAX] {
        assert_eq!(Period::from_hz(hz), None);
    }
    for (hz, divisor) in [(19, 62_799), (100, 11_932), (1_000, 1_193), (596_591, 2)] {
        assert_eq!(Period::from_hz(hz).unwrap().divisor(), divisor);
    }
}

#[test]
fn writes_channel_and_reload_in_hardware_order() {
    let mut pit = Pit::new(Ports::default());
    pit.start_periodic(Period::from_hz(100).unwrap()).unwrap();
    assert_eq!(
        pit.ports.writes.into_inner(),
        [(3, 0x34), (0, 0x9c), (0, 0x2e)]
    );
}

#[test]
fn encodes_the_maximum_period_as_zero() {
    let mut pit = Pit::new(Ports::default());
    pit.start_periodic(Period::from_divisor(65_536).unwrap())
        .unwrap();
    assert_eq!(pit.ports.writes.into_inner(), [(3, 0x34), (0, 0), (0, 0)]);
}

#[test]
fn stops_after_a_rejected_write() {
    for (index, offset) in [3, 0, 0].into_iter().enumerate() {
        let mut pit = Pit::new(Ports {
            reject_at: Some(index),
            ..Ports::default()
        });
        assert_eq!(
            pit.start_periodic(Period::from_hz(100).unwrap()),
            Err(PortRejected { offset })
        );
        assert_eq!(pit.ports.writes.into_inner().len(), index + 1);
    }
}

#[test]
fn switches_to_mode_zero_without_reloading_the_counter() {
    let mut pit = Pit::new(Ports::default());
    let period = Period::from_hz(1000).unwrap();
    pit.start_periodic(period).unwrap();
    pit.stop().unwrap();
    pit.start_periodic(period).unwrap();
    assert_eq!(
        pit.ports.writes.into_inner(),
        [
            (3, 0x34),
            (0, 0xa9),
            (0, 0x04),
            (3, 0x30),
            (3, 0x34),
            (0, 0xa9),
            (0, 0x04)
        ]
    );
}

#[test]
fn reports_a_rejected_stop_write() {
    let mut pit = Pit::new(Ports {
        reject_at: Some(0),
        ..Ports::default()
    });
    assert_eq!(pit.stop(), Err(PortRejected { offset: 3 }));
    assert_eq!(pit.ports.writes.into_inner(), [(3, 0x30)]);
}

#[test]
fn latches_and_reads_channel_zero_in_hardware_order() {
    struct LatchPorts(RefCell<std::vec::IntoIter<u8>>);

    impl PortBlock for LatchPorts {
        fn read8(&self, offset: u16) -> Option<u8> {
            assert_eq!(offset, CHANNEL_0);
            self.0.borrow_mut().next()
        }

        fn write8(&self, offset: u16, value: u8) -> Option<()> {
            assert_eq!((offset, value), (CONTROL, 0x00));
            assert_eq!(self.0.borrow().len(), 2);
            Some(())
        }
    }

    let mut pit = Pit::new(LatchPorts(RefCell::new(std::vec![0x34, 0x12].into_iter())));
    assert_eq!(pit.latch_count(), Ok(0x1234));
}
