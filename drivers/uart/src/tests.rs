extern crate std;

use core::cell::Cell;
use std::vec::Vec;

use super::*;

#[derive(Clone, Copy, Debug)]
enum Access {
    Read(u16, Option<u8>),
    Write(u16, u8, Option<()>),
}

const fn read(offset: u16, value: u8) -> Access {
    Access::Read(offset, Some(value))
}

const fn write(offset: u16, value: u8) -> Access {
    Access::Write(offset, value, Some(()))
}

// Fixed register offsets and bits document the trace independently of the
// driver's constants. Firmware may leave DLAB set initially.
const CONFIGURE: [Access; 8] = [
    write(3, 0x03),
    write(1, 0),
    write(3, 0x83),
    write(0, 0x34),
    write(1, 0x12),
    write(3, 0x03),
    write(2, 0x07),
    write(4, 0x03),
];
const DIVISOR: NonZeroU16 = NonZeroU16::new(0x1234).unwrap();
const NO_RX_ERRORS: ReceiveErrors = ReceiveErrors {
    overrun: false,
    parity: false,
    framing: false,
    break_signal: false,
};

struct Ports<'a> {
    script: &'a [Access],
    consumed: Cell<usize>,
}

impl<'a> Ports<'a> {
    fn new(script: &'a [Access]) -> Self {
        Self {
            script,
            consumed: Cell::new(0),
        }
    }

    fn next(&self) -> Access {
        let index = self.consumed.get();
        self.consumed.set(index + 1);
        self.script[index]
    }

    fn assert_consumed(&self) {
        assert_eq!(self.consumed.get(), self.script.len());
    }
}

impl PortBlock for Ports<'_> {
    fn read8(&self, offset: u16) -> Option<u8> {
        let Access::Read(expected, result) = self.next() else {
            panic!("unexpected read at offset {offset}");
        };
        assert_eq!(offset, expected);
        result
    }

    fn write8(&self, offset: u16, value: u8) -> Option<()> {
        let Access::Write(expected_offset, expected_value, result) = self.next() else {
            panic!("unexpected write at offset {offset}");
        };
        assert_eq!((offset, value), (expected_offset, expected_value));
        result
    }
}

fn configured_script(tail: &[Access]) -> Vec<Access> {
    let mut script = CONFIGURE.to_vec();
    script.extend_from_slice(tail);
    script
}

#[test]
fn configures_the_divisor_and_polls_single_bytes() {
    let script = configured_script(&[
        read(5, 0x00),
        read(5, 0x20),
        write(0, b'x'),
        read(5, 0x00),
        read(5, 0x81), // The FIFO summary may refer to a later byte.
        read(0, 0),
    ]);
    let mut uart = Uart16550::new(Ports::new(&script));
    assert_eq!(uart.try_read(), Err(Error::NotConfigured));
    assert_eq!(uart.try_write(b'x'), Err(Error::NotConfigured));
    uart.configure(DIVISOR).unwrap();
    assert_eq!(uart.try_write(b'x'), Ok(false));
    assert_eq!(uart.try_write(b'x'), Ok(true));
    assert_eq!(uart.try_read(), Ok(None));
    assert_eq!(uart.try_read(), Ok(Some(0)));
    uart.ports.assert_consumed();
}

#[test]
fn any_configuration_rejection_disables_io_until_reconfigured() {
    for (index, access) in CONFIGURE.into_iter().enumerate() {
        let Access::Write(offset, value, _) = access else {
            panic!("configuration must use writes only");
        };
        let mut script = CONFIGURE.to_vec();
        script.extend_from_slice(&CONFIGURE[..index]);
        script.push(Access::Write(offset, value, None));
        if matches!(index, 3 | 4) {
            script.push(write(3, 0x03));
        }
        script.extend_from_slice(&CONFIGURE);
        script.push(read(5, 0));

        let mut uart = Uart16550::new(Ports::new(&script));
        uart.configure(DIVISOR).unwrap();
        assert_eq!(uart.configure(DIVISOR), Err(Error::PortRejected { offset }));
        assert_eq!(uart.try_read(), Err(Error::NotConfigured));
        assert_eq!(uart.try_write(b'x'), Err(Error::NotConfigured));
        uart.configure(DIVISOR).unwrap();
        assert_eq!(uart.try_read(), Ok(None));
        uart.ports.assert_consumed();
    }
}

#[test]
fn tx_status_reads_preserve_rx_errors_and_bad_bytes_are_discarded() {
    let script = configured_script(&[
        read(5, 0x1d), // RX is ready with parity, framing and break errors.
        read(5, 0x21), // Only TX readiness and RX readiness remain latched.
        write(0, b'x'),
        read(5, 0x01),
        read(0, 0),
        read(5, 0x01),
        read(0, b'y'),
    ]);
    let mut uart = Uart16550::new(Ports::new(&script));
    uart.configure(DIVISOR).unwrap();
    assert_eq!(uart.try_write(b'x'), Ok(false));
    assert_eq!(uart.try_write(b'x'), Ok(true));
    assert_eq!(
        uart.try_read(),
        Err(Error::Receive(ReceiveErrors {
            parity: true,
            framing: true,
            break_signal: true,
            ..NO_RX_ERRORS
        }))
    );
    assert_eq!(uart.try_read(), Ok(Some(b'y')));
    uart.ports.assert_consumed();
}

#[test]
fn overrun_reports_lost_input_without_removing_the_queued_byte() {
    let script = configured_script(&[read(5, 0x03), read(5, 0x01), read(0, b'y')]);
    let mut uart = Uart16550::new(Ports::new(&script));
    uart.configure(DIVISOR).unwrap();
    assert_eq!(
        uart.try_read(),
        Err(Error::Receive(ReceiveErrors {
            overrun: true,
            ..NO_RX_ERRORS
        }))
    );
    assert_eq!(uart.try_read(), Ok(Some(b'y')));
    uart.ports.assert_consumed();
}

#[test]
fn a_line_error_without_data_never_accesses_the_receiver() {
    let script = configured_script(&[read(5, 0x10), read(5, 0x00)]);
    let mut uart = Uart16550::new(Ports::new(&script));
    uart.configure(DIVISOR).unwrap();
    assert_eq!(
        uart.try_read(),
        Err(Error::Receive(ReceiveErrors {
            break_signal: true,
            ..NO_RX_ERRORS
        }))
    );
    assert_eq!(uart.try_read(), Ok(None));
    uart.ports.assert_consumed();
}

#[test]
fn rejected_accesses_are_reported_without_losing_a_saved_rx_error() {
    let script = configured_script(&[
        Access::Read(5, None),
        read(5, 0x20),
        Access::Write(0, b'x', None),
        read(5, 0x05),
        Access::Read(0, None),
        read(5, 0x01),
        read(0, 0),
    ]);
    let mut uart = Uart16550::new(Ports::new(&script));
    uart.configure(DIVISOR).unwrap();
    assert_eq!(uart.try_write(b'x'), Err(Error::PortRejected { offset: 5 }));
    assert_eq!(uart.try_write(b'x'), Err(Error::PortRejected { offset: 0 }));
    assert_eq!(uart.try_read(), Err(Error::PortRejected { offset: 0 }));
    assert_eq!(
        uart.try_read(),
        Err(Error::Receive(ReceiveErrors {
            parity: true,
            ..NO_RX_ERRORS
        }))
    );
    uart.ports.assert_consumed();
}

#[test]
fn enables_only_receive_sources_before_opening_the_irq_gate() {
    let script = configured_script(&[write(1, 0x05), write(4, 0x0b), write(4, 0x03), write(1, 0)]);
    let mut uart = Uart16550::new(Ports::new(&script));
    assert_eq!(uart.set_receive_interrupts(true), Err(Error::NotConfigured));
    assert_eq!(uart.interrupt_cause(), Err(Error::NotConfigured));
    uart.configure(DIVISOR).unwrap();
    assert_eq!(uart.set_receive_interrupts(true), Ok(()));
    assert_eq!(uart.set_receive_interrupts(false), Ok(()));
    uart.ports.assert_consumed();
}

#[test]
fn rolls_back_a_rejected_enable_and_closes_both_paths_when_disabling() {
    let script = configured_script(&[
        write(1, 0x05),
        Access::Write(4, 0x0b, None),
        write(4, 0x03),
        write(1, 0),
        Access::Write(4, 0x03, None),
        write(1, 0),
    ]);
    let mut uart = Uart16550::new(Ports::new(&script));
    uart.configure(DIVISOR).unwrap();
    assert_eq!(
        uart.set_receive_interrupts(true),
        Err(Error::PortRejected { offset: 4 })
    );
    assert_eq!(
        uart.set_receive_interrupts(false),
        Err(Error::PortRejected { offset: 4 })
    );
    uart.ports.assert_consumed();
}

#[test]
fn distinguishes_interrupt_causes_with_fifo_bits_set() {
    let cases = [
        (0xcf, None),
        (0xc0, Some(InterruptCause::ModemStatus)),
        (0xc2, Some(InterruptCause::TransmitterEmpty)),
        (0xc4, Some(InterruptCause::ReceiveData)),
        (0xc6, Some(InterruptCause::ReceiveLineStatus)),
        (0xc8, Some(InterruptCause::Reserved(8))),
        (0xcc, Some(InterruptCause::ReceiveTimeout)),
    ];
    let reads: Vec<_> = cases.iter().map(|&(value, _)| read(2, value)).collect();
    let script = configured_script(&reads);
    let mut uart = Uart16550::new(Ports::new(&script));
    uart.configure(DIVISOR).unwrap();
    for (_, expected) in cases {
        assert_eq!(uart.interrupt_cause(), Ok(expected));
    }
    uart.ports.assert_consumed();
}
