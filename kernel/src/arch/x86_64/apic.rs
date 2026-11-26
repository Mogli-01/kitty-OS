//! Register capabilities for the local APIC and the I/O APICs (issue #18,
//! docs/DECISIONS.md D-042).
//!
//! The APIC drivers (Codex, `drivers/apic`) are `forbid(unsafe_code)`: they
//! receive these objects, which implement `kitty_hal::RegisterBlock`, and
//! can only touch what the kernel allows:
//!
//! - **Which device:** the local APIC base comes from the CPU's
//!   IA32_APIC_BASE MSR (never from the driver); an I/O APIC is only granted
//!   at an address the kernel's own validated MADT copy declares. Each
//!   device can be claimed once. Mappings are uncached and never executable.
//! - **Which accesses:** 32-bit, 4-byte aligned, inside the register window.
//!   64-bit accesses are refused (xAPIC and the I/O APIC do not support them).
//! - **Which values:** like the i8042 (D-038), some register writes affect
//!   the whole machine even without DMA:
//!   * local APIC **ICR** (0x300/0x310) sends INIT/SIPI to other CPUs, which
//!     then run code at a low physical address — a full takeover. Not
//!     writable (IPIs are kernel business, Phase 9).
//!   * LVT entries and I/O APIC redirection entries with delivery mode
//!     **SMI** (010) or **INIT** (101) would enter firmware SMM or reset a
//!     CPU. Refused; Fixed, Lowest-priority (I/O APIC only), NMI and ExtINT
//!     are allowed.

use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use alloc::boxed::Box;
use alloc::vec::Vec;
use kitty_hal::RegisterBlock;

use super::cpu;
use crate::mm::CacheType;
use crate::mm::PhysAddr;
use crate::mm::vmm::{self, MmioRegion};
use crate::sync::IrqSpinLock;

/// Local APIC registers a driver may write.
const LAPIC_WRITABLE: &[u64] = &[
    0x080, // TPR
    0x0B0, // EOI
    0x0F0, // spurious-interrupt vector / software enable
    0x320, // LVT timer
    0x350, // LVT LINT0
    0x360, // LVT LINT1
    0x370, // LVT error
    0x380, // timer initial count
    0x3E0, // timer divide configuration
];
/// LVT registers whose delivery mode is checked.
const LAPIC_LVT: &[u64] = &[0x320, 0x350, 0x360, 0x370];
/// Size of the xAPIC register page actually used.
const LAPIC_WINDOW: u64 = 0x400;

/// I/O APIC: IOREGSEL at 0x00, IOWIN at 0x10.
const IOAPIC_SELECT: u64 = 0x00;
const IOAPIC_WINDOW_REG: u64 = 0x10;
const IOAPIC_WINDOW: u64 = 0x20;
const IOAPIC_REDIRECTION_START: u32 = 0x10;

const DELIVERY_SMI: u32 = 0b010;
const DELIVERY_INIT: u32 = 0b101;

/// Delivery mode field (bits 8–10) of an LVT / redirection low dword.
fn delivery_mode(value: u32) -> u32 {
    (value >> 8) & 0b111
}

fn dangerous_delivery(value: u32) -> bool {
    matches!(delivery_mode(value), DELIVERY_SMI | DELIVERY_INIT)
}

/// Why a capability could not be granted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApicError {
    /// The local APIC is disabled in IA32_APIC_BASE.
    Disabled,
    /// The CPU runs the local APIC in x2APIC mode (MSR interface, no MMIO):
    /// not supported yet; keep the 8259 PIC.
    X2Apic,
    /// Address not declared by the validated MADT (or no MADT).
    NotDeclared,
    /// Already claimed by someone else.
    InUse,
    /// The register window could not be mapped.
    MapFailed,
}

fn aligned32(offset: u64, window: u64) -> bool {
    offset.is_multiple_of(4) && offset.checked_add(4).is_some_and(|e| e <= window)
}

// ---------------------------------------------------------------- local ---

static LAPIC_CLAIMED: AtomicBool = AtomicBool::new(false);
static LAPIC_MAPPING: IrqSpinLock<Option<&'static MmioRegion>> = IrqSpinLock::new(None);

/// Exclusive, filtered access to the boot CPU's local APIC registers.
pub struct LocalApicRegisters {
    regs: &'static MmioRegion,
}

/// Claims the local APIC of the boot CPU (xAPIC mode only).
pub fn local_apic() -> Result<LocalApicRegisters, ApicError> {
    let state = cpu::local_apic_state();
    if !state.enabled {
        return Err(ApicError::Disabled);
    }
    if state.x2apic {
        return Err(ApicError::X2Apic);
    }
    if LAPIC_CLAIMED.swap(true, Ordering::AcqRel) {
        return Err(ApicError::InUse);
    }
    let regs = cached_mapping(&LAPIC_MAPPING, state.base, LAPIC_WINDOW).inspect_err(|_| {
        LAPIC_CLAIMED.store(false, Ordering::Release);
    })?;
    Ok(LocalApicRegisters { regs })
}

impl RegisterBlock for LocalApicRegisters {
    fn read32(&self, offset: u64) -> Option<u32> {
        aligned32(offset, LAPIC_WINDOW).then(|| self.regs.read(offset))?
    }

    fn write32(&self, offset: u64, value: u32) -> Option<()> {
        if !aligned32(offset, LAPIC_WINDOW) || !LAPIC_WRITABLE.contains(&offset) {
            return None;
        }
        if LAPIC_LVT.contains(&offset) && dangerous_delivery(value) {
            return None;
        }
        self.regs.write(offset, value)
    }

    fn read64(&self, _offset: u64) -> Option<u64> {
        None
    }

    fn write64(&self, _offset: u64, _value: u64) -> Option<()> {
        None
    }
}

impl Drop for LocalApicRegisters {
    fn drop(&mut self) {
        LAPIC_CLAIMED.store(false, Ordering::Release);
    }
}

// ---------------------------------------------------------------- I/O ---

struct IoApicSlot {
    phys: u64,
    claimed: bool,
    mapping: Option<&'static MmioRegion>,
}

static IO_APICS: IrqSpinLock<Vec<IoApicSlot>> = IrqSpinLock::new(Vec::new());

/// Records the I/O APIC addresses declared by the validated MADT. Called
/// once by the kernel after ACPI discovery; later calls are ignored.
pub fn set_declared_io_apics(addresses: &[u64]) {
    let mut slots = IO_APICS.lock();
    if slots.is_empty() {
        slots.extend(addresses.iter().map(|&phys| IoApicSlot {
            phys,
            claimed: false,
            mapping: None,
        }));
    }
}

/// Exclusive, filtered access to one I/O APIC's selector/window pair.
pub struct IoApicRegisters {
    regs: &'static MmioRegion,
    phys: u64,
    /// Last value written to IOREGSEL, to know what a window write targets.
    selected: AtomicU32,
}

/// Claims the I/O APIC whose register base is `phys`, which must be one of
/// the addresses declared by the validated MADT.
pub fn io_apic(phys: u64) -> Result<IoApicRegisters, ApicError> {
    let mut slots = IO_APICS.lock();
    let slot = slots
        .iter_mut()
        .find(|s| s.phys == phys)
        .ok_or(ApicError::NotDeclared)?;
    if slot.claimed {
        return Err(ApicError::InUse);
    }
    let regs = match slot.mapping {
        Some(r) => r,
        None => {
            let r = map_uncached(phys, IOAPIC_WINDOW)?;
            slot.mapping = Some(r);
            r
        }
    };
    slot.claimed = true;
    Ok(IoApicRegisters {
        regs,
        phys,
        selected: AtomicU32::new(0),
    })
}

impl RegisterBlock for IoApicRegisters {
    fn read32(&self, offset: u64) -> Option<u32> {
        matches!(offset, IOAPIC_SELECT | IOAPIC_WINDOW_REG).then(|| self.regs.read(offset))?
    }

    fn write32(&self, offset: u64, value: u32) -> Option<()> {
        match offset {
            IOAPIC_SELECT => {
                self.selected.store(value & 0xFF, Ordering::Relaxed);
                self.regs.write(offset, value)
            }
            IOAPIC_WINDOW_REG => {
                let index = self.selected.load(Ordering::Relaxed);
                let redirection_low = index >= IOAPIC_REDIRECTION_START
                    && (index - IOAPIC_REDIRECTION_START) % 2 == 0;
                if redirection_low && dangerous_delivery(value) {
                    return None;
                }
                self.regs.write(offset, value)
            }
            _ => None,
        }
    }

    fn read64(&self, _offset: u64) -> Option<u64> {
        None
    }

    fn write64(&self, _offset: u64, _value: u64) -> Option<()> {
        None
    }
}

impl Drop for IoApicRegisters {
    fn drop(&mut self) {
        if let Some(slot) = IO_APICS.lock().iter_mut().find(|s| s.phys == self.phys) {
            slot.claimed = false;
        }
    }
}

// ------------------------------------------------------------- mapping ---

/// Maps an uncached register window once and keeps it for later claims
/// (the MMIO window is never reused, so remapping on every claim would leak
/// address space).
fn cached_mapping(
    cache: &IrqSpinLock<Option<&'static MmioRegion>>,
    phys: u64,
    len: u64,
) -> Result<&'static MmioRegion, ApicError> {
    let mut slot = cache.lock();
    if let Some(r) = *slot {
        return Ok(r);
    }
    let r = map_uncached(phys, len)?;
    *slot = Some(r);
    Ok(r)
}

fn map_uncached(phys: u64, len: u64) -> Result<&'static MmioRegion, ApicError> {
    let phys = PhysAddr::new(phys).ok_or(ApicError::MapFailed)?;
    if !phys.as_u64().is_multiple_of(16) {
        return Err(ApicError::MapFailed);
    }
    // SAFETY: the local APIC (base from the CPU MSR) and the I/O APICs
    // (addresses from the kernel-validated MADT) cannot perform DMA, and
    // every write that could affect other CPUs or the whole machine (ICR,
    // SMI/INIT delivery) is filtered by the capability types above before it
    // reaches the device. `map_mmio` additionally refuses RAM.
    let region = unsafe { vmm::map_mmio(phys, len, CacheType::Uncached) }
        .map_err(|_| ApicError::MapFailed)?;
    Ok(Box::leak(Box::new(region)))
}

/// Boot self-test of the capabilities: claims the local APIC and the first
/// declared I/O APIC, reads their version registers, and checks that the
/// dangerous writes are refused **before** reaching the hardware. Returns
/// `(local APIC version, I/O APIC version if any)`, or `None` on failure.
/// The capabilities are released at the end so drivers can claim them.
pub fn selftest() -> Option<(u8, Option<u8>)> {
    let lapic = local_apic().ok()?;
    let lapic_version = lapic.read32(0x30)? as u8;
    let refused = lapic.write32(0x300, 0).is_none() // ICR
        && lapic.write32(0x310, 0).is_none()
        && lapic.write32(0x350, DELIVERY_INIT << 8).is_none()
        && lapic.write32(0x360, DELIVERY_SMI << 8).is_none()
        && lapic.write32(0x30, 0).is_none() // read-only version register
        && lapic.read32(0x31).is_none() // misaligned
        && lapic.read64(0x30).is_none()
        && local_apic().err() == Some(ApicError::InUse);
    drop(lapic);
    if !refused || local_apic().is_err() {
        return None;
    }

    let first = IO_APICS.lock().first().map(|s| s.phys);
    let io_version = match first {
        None => None,
        Some(phys) => {
            let io = io_apic(phys).ok()?;
            io.write32(IOAPIC_SELECT, 0x01)?; // version register
            let version = io.read32(IOAPIC_WINDOW_REG)? as u8;
            // Select redirection entry 0 (low dword) and try INIT delivery.
            io.write32(IOAPIC_SELECT, IOAPIC_REDIRECTION_START)?;
            let io_refused = io
                .write32(IOAPIC_WINDOW_REG, (DELIVERY_INIT << 8) | 0x10000)
                .is_none()
                && io.write32(0x04, 0).is_none()
                && io_apic(phys).err() == Some(ApicError::InUse)
                && io_apic(phys ^ 0x1000).err() == Some(ApicError::NotDeclared);
            if !io_refused {
                return None;
            }
            Some(version)
        }
    };
    Some((lapic_version, io_version))
}
