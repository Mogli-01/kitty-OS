//! Boot progress and the optional boot screen (issue #87, D-062).
//!
//! The kernel records which boot stages are done ([`Stage`]); a boot screen
//! registered by `kernel/src/devices` (Codex) with [`register_splash`] runs
//! in its own thread from the moment interrupts are on, so it animates
//! *while* the kernel keeps booting and never delays it. It draws through a
//! [`crate::console::FramebufferLease`] and paces itself with
//! [`crate::time`].

use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use crate::sync::IrqSpinLock;

/// Boot stages, in order (the footprints of the boot screen). Stages that
/// do not exist yet are simply never reached.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Stage {
    /// Memory management up.
    Memory = 1,
    /// Interrupts enabled.
    Interrupts = 2,
    /// Drivers started (`devices::init` returned).
    Drivers = 3,
    /// Root file system mounted (initrd).
    Files = 4,
    /// Network (Phase 17).
    Network = 5,
    /// Firewall (Phase 17).
    Firewall = 6,
    /// Graphical interface (Phase 13).
    Interface = 7,
    /// Login screen (Phase 15).
    Login = 8,
}

static STAGES_DONE: AtomicU8 = AtomicU8::new(0);
static READY: AtomicBool = AtomicBool::new(false);

/// Marks `stage` (and everything before it) as done. Never goes back.
pub fn reached(stage: Stage) {
    STAGES_DONE.fetch_max(stage as u8, Ordering::AcqRel);
}

/// Marks the boot as complete.
pub fn mark_ready() {
    READY.store(true, Ordering::Release);
}

/// `(stages done 0-8, boot complete)`.
pub fn progress() -> (u8, bool) {
    (
        STAGES_DONE.load(Ordering::Acquire),
        READY.load(Ordering::Acquire),
    )
}

static SPLASH: IrqSpinLock<Option<fn()>> = IrqSpinLock::new(None);

/// Registers the boot screen. Call it from `devices::init`; only the first
/// registration counts. The function runs in its own kernel thread once
/// interrupts are enabled; it should take the framebuffer with
/// `console::lend_framebuffer`, draw at its own pace (`time::sleep_ms`),
/// and return (dropping the lease) when it is done. If there is no
/// framebuffer or no timer it must return at once.
pub fn register_splash(splash: fn()) {
    let mut slot = SPLASH.lock();
    if slot.is_none() {
        *slot = Some(splash);
    }
}

/// Called by `kmain` right after interrupts are enabled: starts the boot
/// screen thread if one was registered.
pub fn start_splash() {
    let splash = SPLASH.lock().take();
    if let Some(splash) = splash
        && let Err(e) = crate::sched::spawn("boot-splash", splash)
    {
        crate::kprintln!("[boot] boot screen not started ({:?})", e);
    }
}

/// Boot self-test of the clock: without a registered timer there is no
/// time and `sleep_ms` refuses to wait; with one, time is monotonic. Never
/// sleeps (the default boot must not get slower).
pub fn selftest_time() -> bool {
    match crate::time::monotonic_ms() {
        Some(t0) => crate::time::monotonic_ms().is_some_and(|t1| t1 >= t0),
        None => !crate::time::sleep_ms(30),
    }
}

/// Feature self-test of the lease (it clears the screen, so it is not in
/// the default boot): the framebuffer can be lent once, not twice, has a
/// consistent geometry, comes back, and messages printed meanwhile are
/// replayed. With a timer, `sleep_ms` really waits.
#[cfg(feature = "selftest-fb-lease")]
pub fn selftest_lease() -> bool {
    let lease_ok = match crate::console::lend_framebuffer() {
        Some(mut lease) => {
            let geometry = lease.pitch() >= lease.width() * 4
                && lease.pixels_mut().len() == lease.pitch() * lease.height();
            let exclusive = crate::console::lend_framebuffer().is_none();
            crate::kprintln!("[boot] (printed while the framebuffer was lent)");
            drop(lease);
            let back = crate::console::lend_framebuffer().is_some();
            geometry && exclusive && back
        }
        // No framebuffer (headless): nothing to lend, nothing to check.
        None => true,
    };
    let time_ok = match crate::time::monotonic_ms() {
        Some(t0) => {
            crate::time::sleep_ms(30) && crate::time::monotonic_ms().is_some_and(|t| t >= t0 + 30)
        }
        None => !crate::time::sleep_ms(30),
    };
    lease_ok && time_ok
}
