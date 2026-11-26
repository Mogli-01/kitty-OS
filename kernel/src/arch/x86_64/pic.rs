//! Legacy 8259 Programmable Interrupt Controller (master + slave).
//!
//! This is the kernel's *boot-time* interrupt controller: every PC has one
//! and it needs only port I/O, so IRQs work before an APIC driver exists.
//! The I/O APIC driver (Codex, Phase 4) replaces it via
//! `irq::set_controller`, which masks the PIC completely.
//!
//! Why remap: after reset the PIC delivers IRQ 0–7 on vectors 0x08–0x0F,
//! which collide with CPU exceptions (0x08 is double fault!). We move the
//! lines to 0x20–0x2F, and keep every line masked until a driver registers.

use core::sync::atomic::{AtomicU16, Ordering};

use super::irq::{FIRST_INTERRUPT_VECTOR, InterruptController, IrqError, Vector};
use super::port::Port;

const MASTER_CMD: u16 = 0x20;
const MASTER_DATA: u16 = 0x21;
const SLAVE_CMD: u16 = 0xA0;
const SLAVE_DATA: u16 = 0xA1;
/// An unused port; writing it gives the old PIC time to settle (~1 µs).
const DELAY_PORT: u16 = 0x80;

const ICW1_INIT_ICW4: u8 = 0x11;
const ICW4_8086: u8 = 0x01;
const OCW3_READ_ISR: u8 = 0x0B;
const EOI: u8 = 0x20;
/// The slave PIC is wired to master line 2.
const CASCADE_LINE: u8 = 2;

const MASTER_BASE: u8 = FIRST_INTERRUPT_VECTOR; // 0x20
const SLAVE_BASE: u8 = FIRST_INTERRUPT_VECTOR + 8; // 0x28

/// The two cascaded 8259s.
pub struct Pic8259 {
    /// Current mask, bit n = line n masked (1 = masked).
    mask: AtomicU16,
}

fn port(p: u16) -> Port<u8> {
    // SAFETY: 0x20/0x21/0xA0/0xA1 are the 8259 PIC registers and 0x80 is the
    // POST diagnostic port used only as a delay. None of them can perform
    // DMA or change the memory map; the worst a bad write can do is mask or
    // misroute interrupts.
    unsafe { Port::new(p) }
}

fn delay() {
    port(DELAY_PORT).write(0);
}

impl Pic8259 {
    /// A PIC with every line masked (not yet programmed).
    pub const fn new() -> Self {
        Self {
            mask: AtomicU16::new(0xFFFF),
        }
    }

    /// Remaps master → 0x20, slave → 0x28 and masks all lines.
    pub fn init(&self) {
        let (mc, md, sc, sd) = (
            port(MASTER_CMD),
            port(MASTER_DATA),
            port(SLAVE_CMD),
            port(SLAVE_DATA),
        );
        mc.write(ICW1_INIT_ICW4);
        delay();
        sc.write(ICW1_INIT_ICW4);
        delay();
        md.write(MASTER_BASE); // ICW2: vector offset
        delay();
        sd.write(SLAVE_BASE);
        delay();
        md.write(1 << CASCADE_LINE); // ICW3: slave on line 2
        delay();
        sd.write(CASCADE_LINE); // ICW3: slave identity
        delay();
        md.write(ICW4_8086);
        delay();
        sd.write(ICW4_8086);
        delay();
        self.mask.store(0xFFFF, Ordering::Release);
        self.write_mask(0xFFFF);
    }

    fn write_mask(&self, mask: u16) {
        port(MASTER_DATA).write(mask as u8);
        port(SLAVE_DATA).write((mask >> 8) as u8);
    }

    fn update_mask(&self, f: impl Fn(u16) -> u16) {
        let new = f(self.mask.load(Ordering::Acquire));
        self.mask.store(new, Ordering::Release);
        self.write_mask(new);
    }

    /// In-service register of the master (`slave = false`) or slave PIC.
    fn isr(&self, slave: bool) -> u8 {
        // After OCW3 "read ISR", reading the *command* port returns the ISR.
        let cmd = port(if slave { SLAVE_CMD } else { MASTER_CMD });
        cmd.write(OCW3_READ_ISR);
        cmd.read()
    }
}

impl InterruptController for Pic8259 {
    fn name(&self) -> &'static str {
        "8259 PIC"
    }

    fn connect_isa(&self, line: u8, _preferred: Vector) -> Result<Vector, IrqError> {
        // Fixed wiring: line n is always vector 0x20 + n.
        if line >= 16 || line == CASCADE_LINE {
            return Err(IrqError::InvalidLine);
        }
        Ok(Vector(MASTER_BASE + line))
    }

    fn unmask_isa(&self, line: u8) {
        self.update_mask(|m| {
            let mut m = m & !(1 << line);
            if line >= 8 {
                m &= !(1 << CASCADE_LINE); // the slave needs the cascade line
            }
            m
        });
    }

    fn mask_isa(&self, line: u8) {
        self.update_mask(|m| m | (1 << line));
    }

    fn eoi(&self, vector: Vector) {
        if vector.0 >= SLAVE_BASE {
            port(SLAVE_CMD).write(EOI);
        }
        port(MASTER_CMD).write(EOI);
    }

    fn is_spurious(&self, vector: Vector) -> bool {
        // A spurious IRQ7/IRQ15 has its in-service bit clear. For IRQ15 the
        // master *did* see a real cascade interrupt and still needs an EOI.
        match vector.0 {
            v if v == MASTER_BASE + 7 => self.isr(false) & 0x80 == 0,
            v if v == SLAVE_BASE + 7 => {
                let spurious = self.isr(true) & 0x80 == 0;
                if spurious {
                    port(MASTER_CMD).write(EOI);
                }
                spurious
            }
            _ => false,
        }
    }

    fn disable(&self) {
        self.mask.store(0xFFFF, Ordering::Release);
        self.write_mask(0xFFFF);
    }
}
