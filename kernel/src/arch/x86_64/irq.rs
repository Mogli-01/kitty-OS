//! Hardware interrupt core: vector allocation, handler registration,
//! dispatch and end-of-interrupt (EOI). The contract for drivers is in
//! docs/INTERFACES.md ("Interrupções (Fase 4)").
//!
//! Vector map:
//!
//! | vectors     | use                                                    |
//! |-------------|--------------------------------------------------------|
//! | 0x00–0x1F   | CPU exceptions (`exceptions.rs`)                       |
//! | 0x20–0x2F   | legacy 8259 PIC lines 0–15 (remapped; see `pic.rs`)    |
//! | 0x30–0xEF   | device interrupts allocated by [`register_isa`]        |
//! | 0xF0–0xFE   | kernel-internal (self-test, future IPIs / scheduler)   |
//! | 0xFF        | local APIC spurious vector                             |
//!
//! Handler context (what a handler may assume and must respect):
//! - runs in ring 0 on the current kernel stack, with **interrupts
//!   disabled** (interrupt gates), so handlers never nest;
//! - must be short and must not block or sleep;
//! - may take `IrqSpinLock`s (all kernel globals use them), never a plain
//!   `SpinLock` that non-interrupt code also takes;
//! - must not allocate (it is technically safe since the heap lock is an
//!   `IrqSpinLock`, but it makes interrupt latency unbounded);
//! - **does not send EOI**: the core does it after the handler returns.
//!
//! EOI rule: every non-spurious interrupt on a device vector (0x20–0xEF) is
//! acknowledged, *whether or not a handler is registered*. Deciding by
//! "is it registered?" would leave the controller's in-service bit set if a
//! line is unregistered while its interrupt is in flight (on SMP), blocking
//! that line and every lower-priority one. Kernel-internal vectors
//! (0xF0–0xFE) get no controller EOI.

use super::pic::Pic8259;
use crate::console;
use crate::sync::IrqSpinLock;

/// First vector that is an interrupt rather than a CPU exception.
pub const FIRST_INTERRUPT_VECTOR: u8 = 0x20;
const FIRST_DYNAMIC: u8 = 0x30;
const LAST_DYNAMIC: u8 = 0xEF;
/// Vector used by the boot self-test (`int 0xF0`).
pub const SELFTEST_VECTOR: u8 = 0xF0;
/// Local APIC spurious-interrupt vector (no EOI must be sent for it).
pub const APIC_SPURIOUS_VECTOR: u8 = 0xFF;
/// Number of legacy ISA interrupt lines.
pub const ISA_LINES: u8 = 16;

/// An interrupt vector number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Vector(pub u8);

/// An interrupt handler. Receives the vector it was registered on.
pub type IrqHandler = fn(Vector);

/// Errors from the interrupt API.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IrqError {
    /// ISA line number ≥ 16.
    InvalidLine,
    /// Someone already registered this line / vector.
    InUse,
    /// No free vector left in 0x30–0xEF.
    NoFreeVector,
    /// The interrupt controller refused the request.
    ControllerError,
    /// `arch::init` has not run yet.
    NotInitialised,
}

/// An interrupt controller (8259 PIC now; I/O APIC + local APIC from the
/// Codex driver later). Methods are called with the IRQ table lock held
/// and interrupts disabled; they must be short and must not call back into
/// this module.
pub trait InterruptController: Sync {
    /// Short name for logs.
    fn name(&self) -> &'static str;
    /// Routes ISA `line` to a vector, leaving the line **masked**. `preferred`
    /// is a free vector chosen by the core; controllers with a fixed mapping
    /// (the PIC) may return a different one.
    fn connect_isa(&self, line: u8, preferred: Vector) -> Result<Vector, IrqError>;
    /// Unmasks ISA `line`.
    fn unmask_isa(&self, line: u8);
    /// Masks ISA `line`.
    fn mask_isa(&self, line: u8);
    /// Signals end of interrupt for `vector`.
    fn eoi(&self, vector: Vector);
    /// Whether `vector` is a spurious interrupt of this controller (no
    /// handler call, no EOI).
    fn is_spurious(&self, vector: Vector) -> bool;
    /// Masks everything (called when another controller takes over).
    fn disable(&self);
}

struct IrqTable {
    handlers: [Option<IrqHandler>; 256],
    /// Vector each ISA line is connected to.
    isa: [Option<u8>; ISA_LINES as usize],
    /// Vectors handed out by `register_local` (local APIC sources).
    local: [bool; 256],
    controller: Option<&'static dyn InterruptController>,
    unexpected: u64,
}

static TABLE: IrqSpinLock<IrqTable> = IrqSpinLock::new(IrqTable {
    handlers: [None; 256],
    isa: [None; ISA_LINES as usize],
    local: [false; 256],
    controller: None,
    unexpected: 0,
});

static PIC: Pic8259 = Pic8259::new();

/// Remaps and masks the legacy PIC and makes it the active controller.
/// Called by `arch::init` after the IDT is loaded.
pub fn init() {
    PIC.init();
    let mut t = TABLE.lock();
    if t.controller.is_none() {
        t.controller = Some(&PIC);
    }
}

/// Replaces the active interrupt controller (e.g. the I/O APIC driver).
/// The previous one is fully masked. Lines connected through the old
/// controller are forgotten: drivers must register again.
pub fn set_controller(controller: &'static dyn InterruptController) {
    let mut t = TABLE.lock();
    if let Some(old) = t.controller.replace(controller) {
        old.disable();
    }
    let lines = t.isa;
    for (line, vector) in lines.iter().enumerate() {
        if let Some(v) = vector {
            t.handlers[*v as usize] = None;
            t.isa[line] = None;
        }
    }
}

/// Connects legacy ISA interrupt `line` (0–15; e.g. 1 = PS/2 keyboard) to
/// `handler` and unmasks it. Returns the vector used.
pub fn register_isa(line: u8, handler: IrqHandler) -> Result<Vector, IrqError> {
    if line >= ISA_LINES {
        return Err(IrqError::InvalidLine);
    }
    let mut t = TABLE.lock();
    let controller = t.controller.ok_or(IrqError::NotInitialised)?;
    if t.isa[line as usize].is_some() {
        return Err(IrqError::InUse);
    }
    let preferred = (FIRST_DYNAMIC..=LAST_DYNAMIC)
        .find(|&v| t.handlers[v as usize].is_none())
        .ok_or(IrqError::NoFreeVector)?;
    let vector = controller.connect_isa(line, Vector(preferred))?;
    // A controller may only hand out device vectors: never an exception,
    // a kernel-internal vector (0xF0+) or the APIC spurious vector.
    if !(FIRST_INTERRUPT_VECTOR..=LAST_DYNAMIC).contains(&vector.0)
        || t.handlers[vector.0 as usize].is_some()
    {
        return Err(IrqError::ControllerError);
    }
    // Install the handler *before* unmasking, so the first interrupt finds it.
    t.handlers[vector.0 as usize] = Some(handler);
    t.isa[line as usize] = Some(vector.0);
    controller.unmask_isa(line);
    Ok(vector)
}

/// Masks ISA `line` and removes its handler.
pub fn unregister_isa(line: u8) -> Result<(), IrqError> {
    let mut t = TABLE.lock();
    let controller = t.controller.ok_or(IrqError::NotInitialised)?;
    let vector = t
        .isa
        .get_mut(line as usize)
        .ok_or(IrqError::InvalidLine)?
        .take()
        .ok_or(IrqError::InvalidLine)?;
    controller.mask_isa(line);
    t.handlers[vector as usize] = None;
    Ok(())
}

/// Reserves a free **device vector** (0x30–0xEF) for an interrupt source
/// that is not an ISA line — the local APIC timer, error or thermal LVT
/// entries (issue #36) — and installs `handler` on it. No controller
/// routing is done: the driver programs the source itself with the
/// returned vector, **masked**, and unmasks it only after this returns.
///
/// EOI is sent by the core after the handler, like for every device vector
/// (with the APIC controller that is a write to the local APIC EOI
/// register). Usable before or after `sti`; boot CPU only (single CPU).
pub fn register_local(handler: IrqHandler) -> Result<Vector, IrqError> {
    let mut t = TABLE.lock();
    let vector = (FIRST_DYNAMIC..=LAST_DYNAMIC)
        .find(|&v| t.handlers[v as usize].is_none() && !t.local[v as usize])
        .ok_or(IrqError::NoFreeVector)?;
    t.handlers[vector as usize] = Some(handler);
    t.local[vector as usize] = true;
    Ok(Vector(vector))
}

/// Removes a handler installed by [`register_local`]. Mask the source
/// first. An interrupt already in flight that arrives afterwards finds no
/// handler: it is counted as unexpected and still gets its EOI, so the
/// controller is never left with an in-service bit set. The vector can be
/// handed out again only after this returns.
pub fn unregister_local(vector: Vector) -> Result<(), IrqError> {
    let mut t = TABLE.lock();
    let v = vector.0 as usize;
    if !t.local.get(v).copied().unwrap_or(false) {
        return Err(IrqError::InvalidLine);
    }
    t.handlers[v] = None;
    t.local[v] = false;
    Ok(())
}

/// Registers a handler on a kernel-internal vector (0xF0–0xFE). No
/// controller is involved and no EOI is sent.
pub(crate) fn register_internal(vector: u8, handler: IrqHandler) -> Result<(), IrqError> {
    if !(SELFTEST_VECTOR..APIC_SPURIOUS_VECTOR).contains(&vector) {
        return Err(IrqError::InvalidLine);
    }
    let mut t = TABLE.lock();
    if t.handlers[vector as usize].is_some() {
        return Err(IrqError::InUse);
    }
    t.handlers[vector as usize] = Some(handler);
    Ok(())
}

/// Number of interrupts that arrived on a vector with no handler.
pub fn unexpected_count() -> u64 {
    TABLE.lock().unexpected
}

/// Called by the trap stub for vectors ≥ 0x20, with interrupts disabled.
pub(super) fn dispatch(vector: u8) {
    let v = Vector(vector);
    // Copy what we need and drop the lock before calling the handler, so a
    // handler may itself use this API (e.g. unregister its own line).
    let (handler, controller) = {
        let mut t = TABLE.lock();
        let controller = t.controller;
        if vector == APIC_SPURIOUS_VECTOR || controller.is_some_and(|c| c.is_spurious(v)) {
            return;
        }
        let handler = t.handlers[vector as usize];
        if handler.is_none() {
            t.unexpected += 1;
        }
        (handler, controller)
    };
    let device_vector = (FIRST_INTERRUPT_VECTOR..=LAST_DYNAMIC).contains(&vector);

    match handler {
        Some(h) => h(v),
        None => console::debug_print(format_args!(
            "[irq] unexpected interrupt on vector {:#x}\n",
            vector
        )),
    }

    if device_vector && let Some(c) = controller {
        c.eoi(v);
    }

    // Only now (handler done, EOI sent, no IRQ-table lock held) may the
    // scheduler switch threads, if the timer asked for it.
    crate::sched::preempt_if_requested();
}

/// Boot self-test of the dispatch path without any hardware: registers a
/// handler on [`SELFTEST_VECTOR`] and raises it with `int`. Returns whether
/// the handler ran exactly once.
pub fn selftest() -> bool {
    use core::sync::atomic::{AtomicU32, Ordering};
    static HITS: AtomicU32 = AtomicU32::new(0);
    fn on_selftest(_: Vector) {
        HITS.fetch_add(1, Ordering::Relaxed);
    }
    if register_internal(SELFTEST_VECTOR, on_selftest).is_err() {
        return false;
    }
    // SAFETY: vector 0xF0 has a present IDT gate (all 256 do) whose stub
    // saves and restores every register; its handler only bumps an atomic.
    // No `nomem`/`nostack`: the CPU pushes a frame and the handler writes
    // memory (issue #7).
    unsafe {
        core::arch::asm!("int {v}", v = const SELFTEST_VECTOR);
    }
    let ok = HITS.load(Ordering::Relaxed) == 1;
    TABLE.lock().handlers[SELFTEST_VECTOR as usize] = None;

    // register_local: a device vector, distinct each time, released by
    // unregister_local, and only for vectors it handed out.
    let local_ok = match (register_local(on_selftest), register_local(on_selftest)) {
        (Ok(a), Ok(b)) => {
            let in_range = [a, b]
                .iter()
                .all(|v| (FIRST_DYNAMIC..=LAST_DYNAMIC).contains(&v.0));
            let distinct = a != b;
            let released = unregister_local(a).is_ok() && unregister_local(b).is_ok();
            let twice = unregister_local(a).is_err();
            let foreign = unregister_local(Vector(SELFTEST_VECTOR)).is_err();
            in_range && distinct && released && twice && foreign
        }
        _ => false,
    };
    ok && local_ok
}

/// Name of the active interrupt controller.
pub fn controller_name() -> &'static str {
    TABLE.lock().controller.map_or("none", |c| c.name())
}
