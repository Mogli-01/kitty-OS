use std::cell::{Cell, RefCell};
use std::collections::VecDeque;

use kitty_hal::PortBlock;
use kitty_i8042::{Error, I8042, OutputByte, Source};

const DATA: u16 = 0;
const STATUS: u16 = 4;

#[derive(Debug, PartialEq, Eq)]
enum Operation {
    Read(u16, u8),
    Write(u16, u8),
}

struct Script(RefCell<VecDeque<Operation>>);

impl Script {
    fn new(operations: impl IntoIterator<Item = Operation>) -> Self {
        Self(RefCell::new(operations.into_iter().collect()))
    }

    fn finished(&self) {
        assert!(self.0.borrow().is_empty());
    }
}

impl PortBlock for &Script {
    fn read8(&self, offset: u16) -> Option<u8> {
        let next = self.0.borrow_mut().pop_front();
        match next {
            Some(Operation::Read(expected, value)) if expected == offset => Some(value),
            other => panic!("unexpected read of {offset:#x}, script has {other:?}"),
        }
    }

    fn write8(&self, offset: u16, value: u8) -> Option<()> {
        assert_eq!(
            self.0.borrow_mut().pop_front(),
            Some(Operation::Write(offset, value))
        );
        Some(())
    }
}

fn write(ops: &mut Vec<Operation>, offset: u16, value: u8) {
    ops.extend([Operation::Read(STATUS, 0), Operation::Write(offset, value)]);
}

fn reply(ops: &mut Vec<Operation>, value: u8) {
    ops.extend([Operation::Read(STATUS, 1), Operation::Read(DATA, value)]);
}

fn setup() -> Vec<Operation> {
    let mut ops = Vec::new();
    write(&mut ops, STATUS, 0xad);
    write(&mut ops, STATUS, 0xa7);
    // Drain a stale byte before accepting the configuration response.
    reply(&mut ops, 0x1c);
    ops.push(Operation::Read(STATUS, 0));
    write(&mut ops, STATUS, 0x20);
    reply(&mut ops, 0x47); // Translation and IRQs initially enabled.
    write(&mut ops, STATUS, 0x60);
    write(&mut ops, DATA, 0x34);
    write(&mut ops, STATUS, 0xae);
    ops
}

fn select_set2(ops: &mut Vec<Operation>) {
    for value in [0xf5, 0xf0, 2, 0xf0, 0] {
        write(ops, DATA, value);
        reply(ops, 0xfa);
    }
}

#[test]
fn configures_raw_set2_with_translation_off_before_enabling_irq1() {
    let mut ops = setup();
    // The first device command is retried after a RESEND.
    write(&mut ops, DATA, 0xf5);
    reply(&mut ops, 0xfe);
    select_set2(&mut ops);
    reply(&mut ops, 2);
    write(&mut ops, DATA, 0xf4);
    reply(&mut ops, 0xfa);
    write(&mut ops, STATUS, 0x60);
    write(&mut ops, DATA, 0x25);
    let port = Script::new(ops);
    let mut controller = I8042::new(&port);
    assert_eq!(controller.initialize_keyboard(), Ok(()));
    port.finished();
}

#[test]
fn leaves_the_port_disabled_if_the_keyboard_reports_a_different_set() {
    let mut ops = setup();
    select_set2(&mut ops);
    reply(&mut ops, 1);
    write(&mut ops, STATUS, 0xad);
    let port = Script::new(ops);
    assert_eq!(
        I8042::new(&port).initialize_keyboard(),
        Err(Error::UnexpectedReply(1))
    );
    port.finished();
}

#[test]
fn stops_after_three_resends() {
    let mut ops = setup();
    for _ in 0..3 {
        write(&mut ops, DATA, 0xf5);
        reply(&mut ops, 0xfe);
    }
    write(&mut ops, STATUS, 0xad);
    let port = Script::new(ops);
    assert_eq!(
        I8042::new(&port).initialize_keyboard(),
        Err(Error::ResendLimit)
    );
    port.finished();
}

#[test]
fn reads_at_most_one_byte_and_keeps_source_and_error_status() {
    use Operation::Read;
    let port = Script::new([
        Read(STATUS, 0x20),
        Read(STATUS, 0x21),
        Read(DATA, 0x31),
        Read(STATUS, 0x81),
        Read(DATA, 0x1c),
        Read(STATUS, 0x01),
        Read(DATA, 0x1c),
    ]);
    let mut controller = I8042::new(&port);
    assert_eq!(controller.try_read(), Ok(None));
    assert_eq!(
        controller.try_read(),
        Ok(Some(OutputByte {
            source: Source::Auxiliary,
            value: 0x31
        }))
    );
    assert_eq!(
        controller.try_read(),
        Err(Error::CorruptOutput(Source::Keyboard))
    );
    assert_eq!(
        controller.try_read(),
        Ok(Some(OutputByte {
            source: Source::Keyboard,
            value: 0x1c
        }))
    );
    port.finished();
}

#[test]
fn controller_that_never_accepts_writes_times_out() {
    struct Busy(Cell<usize>);

    impl PortBlock for &Busy {
        fn read8(&self, offset: u16) -> Option<u8> {
            assert_eq!(offset, STATUS);
            let reads = self.0.get() + 1;
            assert!(reads <= 200_000, "unbounded polling");
            self.0.set(reads);
            Some(2)
        }

        fn write8(&self, _offset: u16, _value: u8) -> Option<()> {
            panic!("wrote to a busy controller")
        }
    }

    let port = Busy(Cell::new(0));
    assert_eq!(
        I8042::new(&port).initialize_keyboard(),
        Err(Error::PollTimeout)
    );
}
