//! Minimal polled 16550 UART driver for COM1, used as the *early* debug
//! console (boot log and panics).
//!
//! This is intentionally tiny and lives in the arch layer because it must
//! work before anything else exists. A full UART driver (interrupt driven,
//! input, several ports) belongs in `drivers/` (Codex).

use super::port::Port;

/// I/O base of COM1 on PC-compatible machines.
const COM1_BASE: u16 = 0x3F8;

/// Line Status Register bit: transmit holding register empty.
const LSR_THR_EMPTY: u8 = 1 << 5;

/// How many status polls we do before giving up on a byte. Prevents a hang
/// if the UART is absent or broken (e.g. on real hardware without COM1).
const TX_SPIN_LIMIT: u32 = 100_000;

/// A polled 16550-compatible UART.
pub struct SerialPort {
    data: Port<u8>,      // +0: RBR/THR (DLAB=0), divisor low (DLAB=1)
    int_en: Port<u8>,    // +1: IER (DLAB=0), divisor high (DLAB=1)
    fifo_ctl: Port<u8>,  // +2: FCR
    line_ctl: Port<u8>,  // +3: LCR
    modem_ctl: Port<u8>, // +4: MCR
    line_sts: Port<u8>,  // +5: LSR
}

impl SerialPort {
    /// Initialises COM1 at 115200 baud, 8N1, FIFOs on, interrupts off.
    ///
    /// Returns `None` if the loopback self-test fails (no UART present).
    pub fn init_com1() -> Option<Self> {
        let base = COM1_BASE;
        // SAFETY: ports 0x3F8..=0x3FD are the registers of the COM1 16550
        // UART. The UART has no DMA and cannot change the memory map, so no
        // sequence of accesses can break memory safety. Worst case: garbled
        // serial output.
        let port = |off: u16| unsafe { Port::<u8>::new(base + off) };
        let s = Self {
            data: port(0),
            int_en: port(1),
            fifo_ctl: port(2),
            line_ctl: port(3),
            modem_ctl: port(4),
            line_sts: port(5),
        };

        s.int_en.write(0x00); // no UART interrupts: we poll
        s.line_ctl.write(0x80); // DLAB=1 to program the baud divisor
        s.data.write(0x01); // divisor = 1 -> 115200 baud (115200 / 1)
        s.int_en.write(0x00); // divisor high byte
        s.line_ctl.write(0x03); // DLAB=0, 8 data bits, no parity, 1 stop bit
        s.fifo_ctl.write(0xC7); // enable + clear FIFOs, 14-byte threshold
        s.modem_ctl.write(0x1E); // loopback mode for the self-test

        s.data.write(0xAE);
        if s.data.read() != 0xAE {
            return None;
        }

        s.modem_ctl.write(0x0F); // normal mode: DTR, RTS, OUT1, OUT2
        Some(s)
    }

    /// Sends one raw byte, waiting (bounded) for the transmitter.
    pub fn write_byte(&mut self, byte: u8) {
        for _ in 0..TX_SPIN_LIMIT {
            if self.line_sts.read() & LSR_THR_EMPTY != 0 {
                self.data.write(byte);
                return;
            }
            core::hint::spin_loop();
        }
        // Transmitter stuck: drop the byte rather than hang the kernel.
    }
}

impl core::fmt::Write for SerialPort {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for b in s.bytes() {
            if b == b'\n' {
                self.write_byte(b'\r');
            }
            self.write_byte(b);
        }
        Ok(())
    }
}
