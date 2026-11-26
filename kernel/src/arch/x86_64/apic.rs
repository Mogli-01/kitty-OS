//! Register capabilities for the local APIC and the I/O APICs (issue #18,
//! docs/DECISIONS.md D-042).
//!
//! The APIC drivers (Codex, `drivers/apic`) are `forbid(unsafe_code)`: they
//! receive these objects, which implement `kitty_hal::RegisterBlock`, and
//! can only touch what the kernel allows:
//!
//! - **Which device:** the local APIC base comes from the CPU's
//!   IA32_APIC_BASE MSR (never from the driver); an I/O APIC is only granted
//!   at an address the kernel's own validated MADT copy declares, whose
//!   register window does not overlap the local APIC page or another I/O
//!   APIC, and whose version register looks plausible. Each device can be
//!   claimed once. Mappings are uncached and never executable.
//! - **Which accesses:** 32-bit, 4-byte aligned, inside the register window.
//!   64-bit accesses are refused (xAPIC and the I/O APIC do not support them).
//! - **Which values** (allow-lists per register; same lesson as the i8042
//!   A20/reset commands, D-038):
//!   * local APIC **ICR** (0x300/0x310) sends INIT/SIPI to other CPUs, which
//!     then run code at a low physical address — a full takeover. Not
//!     writable (IPIs are kernel business, Phase 9).
//!   * delivery modes: LVT timer/error only Fixed; LINT0/LINT1 Fixed, NMI or
//!     ExtINT; I/O APIC redirections Fixed, Lowest-priority, NMI or ExtINT.
//!     SMI (enter firmware SMM), INIT (reset a CPU) and reserved modes are
//!     refused.
//!   * vectors: an unmasked Fixed/Lowest-priority entry must use a device
//!     vector 0x20–0xEF. Vectors 0x10–0x1F would land on CPU exception stubs
//!     (some expect an error code the hardware does not push for external
//!     interrupts, corrupting the stack); 0xF0–0xFF are kernel-internal. The
//!     spurious vector (SVR) must be 0xFF.
//! - **Not from the NMI handler**: the capabilities use `IrqSpinLock`s, and
//!   `cli` does not mask NMIs; an NMI arriving while the lock is held and
//!   trying to take it would deadlock.
//! - **I/O APIC selector/window**: the selector (IOREGSEL) and the window
//!   write are done under one internal `IrqSpinLock`, and the selector is
//!   reset on every claim, so the kernel always knows which register a
//!   window write hits (no time-of-check/time-of-use between the check and
//!   the write, even from an interrupt handler or another CPU).

use core::sync::atomic::{AtomicBool, Ordering};

use alloc::boxed::Box;
use alloc::vec::Vec;
use kitty_hal::RegisterBlock;

use super::cpu;
use crate::mm::CacheType;
use crate::mm::PhysAddr;
use crate::mm::vmm::{self, MmioRegion};
use crate::sync::IrqSpinLock;

/// Size of the xAPIC register page actually used.
const LAPIC_WINDOW: u64 = 0x400;
const LAPIC_PAGE: u64 = 0x1000;

const LAPIC_TPR: u64 = 0x080;
const LAPIC_EOI: u64 = 0x0B0;
const LAPIC_SVR: u64 = 0x0F0;
const LAPIC_LVT_TIMER: u64 = 0x320;
const LAPIC_LVT_LINT0: u64 = 0x350;
const LAPIC_LVT_LINT1: u64 = 0x360;
const LAPIC_LVT_ERROR: u64 = 0x370;
const LAPIC_TIMER_INITIAL: u64 = 0x380;
const LAPIC_TIMER_DIVIDE: u64 = 0x3E0;

/// I/O APIC: IOREGSEL at 0x00, IOWIN at 0x10.
const IOAPIC_SELECT: u64 = 0x00;
const IOAPIC_WINDOW_REG: u64 = 0x10;
const IOAPIC_WINDOW: u64 = 0x20;
const IOAPIC_VERSION_INDEX: u32 = 0x01;
const IOAPIC_REDIRECTION_START: u32 = 0x10;

const MODE_FIXED: u32 = 0b000;
const MODE_LOWEST_PRIORITY: u32 = 0b001;
const MODE_SMI: u32 = 0b010;
const MODE_NMI: u32 = 0b100;
const MODE_INIT: u32 = 0b101;
const MODE_EXTINT: u32 = 0b111;
const MASKED: u32 = 1 << 16;
const SPURIOUS_VECTOR: u32 = 0xFF;

/// Delivery mode field (bits 8–10) of an LVT / redirection low dword.
fn delivery_mode(value: u32) -> u32 {
    (value >> 8) & 0b111
}

fn device_vector(value: u32) -> bool {
    (0x20..=0xEF).contains(&(value & 0xFF))
}

/// Checks an LVT / redirection low dword against the allowed delivery
/// modes. Fixed and Lowest-priority entries that are not masked must use a
/// device vector.
fn entry_allowed(value: u32, modes: &[u32]) -> bool {
    let mode = delivery_mode(value);
    if !modes.contains(&mode) {
        return false;
    }
    let vectored = matches!(mode, MODE_FIXED | MODE_LOWEST_PRIORITY);
    !vectored || value & MASKED != 0 || device_vector(value)
}

/// Why a capability could not be granted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApicError {
    /// The local APIC is disabled in IA32_APIC_BASE.
    Disabled,
    /// The CPU runs the local APIC in x2APIC mode (MSR interface, no MMIO):
    /// not supported yet; keep the 8259 PIC.
    X2Apic,
    /// Address not declared by the validated MADT (or no MADT), or its
    /// window overlaps the local APIC / another I/O APIC.
    NotDeclared,
    /// The I/O APIC version register does not look like an I/O APIC.
    Implausible,
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

fn lapic_write_allowed(offset: u64, value: u32) -> bool {
    match offset {
        LAPIC_TPR | LAPIC_EOI | LAPIC_TIMER_INITIAL | LAPIC_TIMER_DIVIDE => true,
        LAPIC_SVR => value & 0xFF == SPURIOUS_VECTOR,
        LAPIC_LVT_TIMER | LAPIC_LVT_ERROR => entry_allowed(value, &[MODE_FIXED]),
        LAPIC_LVT_LINT0 | LAPIC_LVT_LINT1 => {
            entry_allowed(value, &[MODE_FIXED, MODE_NMI, MODE_EXTINT])
        }
        _ => false, // ICR, read-only and unused registers
    }
}

impl RegisterBlock for LocalApicRegisters {
    fn read32(&self, offset: u64) -> Option<u32> {
        aligned32(offset, LAPIC_WINDOW).then(|| self.regs.read(offset))?
    }

    fn write32(&self, offset: u64, value: u32) -> Option<()> {
        if !aligned32(offset, LAPIC_WINDOW) || !lapic_write_allowed(offset, value) {
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

fn overlaps(a: u64, a_len: u64, b: u64, b_len: u64) -> bool {
    a < b.saturating_add(b_len) && b < a.saturating_add(a_len)
}

/// Records the I/O APIC addresses declared by the validated MADT. Called
/// once by the kernel after ACPI discovery; later calls are ignored.
/// Addresses that are misaligned, whose window touches the local APIC page,
/// or that overlap another declared I/O APIC are dropped (a hostile MADT
/// could otherwise turn an "I/O APIC" capability into access to the local
/// APIC ICR or to another device).
pub fn set_declared_io_apics(addresses: &[u64]) {
    let mut slots = IO_APICS.lock();
    if !slots.is_empty() {
        return;
    }
    let lapic = cpu::local_apic_state().base;
    for &phys in addresses {
        let aligned = phys.is_multiple_of(16);
        let hits_lapic = overlaps(phys, IOAPIC_WINDOW, lapic, LAPIC_PAGE);
        let hits_other = addresses
            .iter()
            .any(|&o| o != phys && overlaps(phys, IOAPIC_WINDOW, o, IOAPIC_WINDOW));
        if aligned && !hits_lapic && !hits_other {
            slots.push(IoApicSlot {
                phys,
                claimed: false,
                mapping: None,
            });
        }
    }
}

/// Exclusive, filtered access to one I/O APIC's selector/window pair.
pub struct IoApicRegisters {
    regs: &'static MmioRegion,
    phys: u64,
    /// The value last written to IOREGSEL. The lock also serialises every
    /// selector/window access, so check and write cannot be separated.
    selected: IrqSpinLock<u32>,
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
    // The hardware selector survives previous owners: set it to a known
    // value before trusting `selected` (found by the security review).
    // This is a real write (0x01 to offset 0) before the plausibility check:
    // harmless on a real I/O APIC; the address already passed the MADT and
    // overlap checks, so it cannot be the local APIC.
    regs.write(IOAPIC_SELECT, IOAPIC_VERSION_INDEX)
        .ok_or(ApicError::MapFailed)?;
    let version = regs
        .read::<u32>(IOAPIC_WINDOW_REG)
        .ok_or(ApicError::MapFailed)?;
    let max_redirection = (version >> 16) & 0xFF;
    if matches!(version & 0xFF, 0x00 | 0xFF) || max_redirection >= 0xF0 {
        return Err(ApicError::Implausible);
    }
    slot.claimed = true;
    Ok(IoApicRegisters {
        regs,
        phys,
        selected: IrqSpinLock::new(IOAPIC_VERSION_INDEX),
    })
}

impl RegisterBlock for IoApicRegisters {
    fn read32(&self, offset: u64) -> Option<u32> {
        let _serialised = self.selected.lock();
        matches!(offset, IOAPIC_SELECT | IOAPIC_WINDOW_REG).then(|| self.regs.read(offset))?
    }

    fn write32(&self, offset: u64, value: u32) -> Option<()> {
        let mut selected = self.selected.lock();
        match offset {
            IOAPIC_SELECT => {
                let index = value & 0xFF;
                self.regs.write(IOAPIC_SELECT, index)?;
                *selected = index;
                Some(())
            }
            IOAPIC_WINDOW_REG => {
                let index = *selected;
                let redirection_low = index >= IOAPIC_REDIRECTION_START
                    && (index - IOAPIC_REDIRECTION_START).is_multiple_of(2);
                let modes = [MODE_FIXED, MODE_LOWEST_PRIORITY, MODE_NMI, MODE_EXTINT];
                if redirection_low && !entry_allowed(value, &modes) {
                    return None;
                }
                self.regs.write(IOAPIC_WINDOW_REG, value)
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
    // (addresses from the kernel-validated MADT, not overlapping the local
    // APIC page or each other) cannot perform DMA, and every write that
    // could affect other CPUs or the whole machine (ICR, SMI/INIT/reserved
    // delivery modes, non-device vectors) is filtered by the capability
    // types above before it reaches the device. `map_mmio` refuses RAM.
    let region = unsafe { vmm::map_mmio(phys, len, CacheType::Uncached) }
        .map_err(|_| ApicError::MapFailed)?;
    Ok(Box::leak(Box::new(region)))
}

/// Boot self-test of the capabilities: claims the local APIC and the first
/// declared I/O APIC, reads their version registers, and checks that the
/// dangerous writes are refused **before** reaching the hardware. The only
/// real writes are to IOREGSEL (no effect by itself). Returns
/// `(local APIC version, I/O APIC version if any)`, or `None` on failure.
/// The capabilities are released at the end so drivers can claim them.
pub fn selftest() -> Option<(u8, Option<u8>)> {
    let lapic = local_apic().ok()?;
    let lapic_version = lapic.read32(0x30)? as u8;
    let masked_init = (MODE_INIT << 8) | MASKED;
    let refused = lapic.write32(0x300, 0).is_none() // ICR
        && lapic.write32(0x310, 0).is_none()
        && lapic.write32(LAPIC_LVT_LINT0, masked_init).is_none()
        && lapic.write32(LAPIC_LVT_LINT1, MODE_SMI << 8).is_none()
        && lapic.write32(LAPIC_LVT_TIMER, 0x11).is_none() // exception vector
        && lapic.write32(LAPIC_LVT_TIMER, 0xF0).is_none() // kernel vector
        && lapic.write32(LAPIC_LVT_ERROR, (MODE_NMI << 8) | 0x40).is_none()
        && lapic.write32(LAPIC_SVR, 0x1_2F).is_none() // spurious must be 0xFF
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
            let version = io.read32(IOAPIC_WINDOW_REG)? as u8; // selector reset to 1
            io.write32(IOAPIC_SELECT, IOAPIC_REDIRECTION_START)?; // entry 0, low dword
            let io_refused = io.write32(IOAPIC_WINDOW_REG, (MODE_INIT << 8) | MASKED).is_none()
                && io.write32(IOAPIC_WINDOW_REG, (MODE_SMI << 8) | MASKED).is_none()
                && io.write32(IOAPIC_WINDOW_REG, 0x11).is_none() // unmasked, exception vector
                && io.write32(0x04, 0).is_none()
                && io.read64(0).is_none()
                && io_apic(phys).err() == Some(ApicError::InUse)
                && io_apic(phys ^ 0x1000).err() == Some(ApicError::NotDeclared);
            drop(io);
            // A new owner must find the selector reset, whatever we left.
            let again = io_apic(phys).ok()?;
            let reset = again.read32(IOAPIC_SELECT)? == IOAPIC_VERSION_INDEX;
            drop(again);
            if !(io_refused && reset) {
                return None;
            }
            Some(version)
        }
    };
    Some((lapic_version, io_version))
}
