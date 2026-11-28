//! The INPUT object: keyboard and mouse events for the compositor
//! (Phase 6e, docs/FASE6.md §7, D-053, D-058, D-065).
//!
//! - **One** INPUT object exists. The kernel hands out at most one
//!   [`InputGrant`] at a time; the handle that holds it has `READ` and
//!   `TRANSFER` but **no** `DUP`, so it can be moved (to the compositor)
//!   but never copied: exactly one process can read input at any moment
//!   (anti-keylogger).
//! - Drivers publish with [`push`] from interrupt handlers: fixed ring of
//!   256 events, one short lock, no heap; then the reader is woken.
//! - If the ring is full the **new** event is dropped; once the reader has
//!   drained the events that were queued, it gets a `kind = 3` ("lost")
//!   event, so it resets its key and button state *after* applying the
//!   old ones (whose releases may be among the dropped).
//! - **Keyboard routing (anti-keylogger):** drivers hand every key to
//!   [`push_key`] together with its text. While nobody reads INPUT, the
//!   text goes to the kernel console (the shell reads it with
//!   `console_read`); once the INPUT holder has started reading, the text
//!   is **not** given to the console any more, so no other process with a
//!   CONSOLE handle can read keys typed for the compositor.
//! - Whenever the grant changes owner (handle closed, owner dead, handle
//!   moved), the ring is **emptied**: the next reader never sees keys typed
//!   for the previous one (a password, say).

use alloc::sync::Arc;
use core::sync::atomic::{AtomicBool, Ordering};

use crate::sched::WaitQueue;
use crate::sync::IrqSpinLock;

/// One input event, exactly as user space receives it (`#[repr(C)]`, 24
/// bytes, little-endian; docs/FASE6.md §7).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[repr(C)]
pub struct InputEvent {
    /// Monotonic time in ns since boot (0 until the kernel clock exists).
    pub time_ns: u64,
    /// 1 = key, 2 = relative mouse, 3 = events were lost.
    pub kind: u16,
    /// Key: physical key code; mouse: 0.
    pub code: u16,
    /// Key: 0 released, 1 pressed, 2 repeat; mouse: buttons.
    pub value: u32,
    /// Mouse X movement; key: 0.
    pub dx: i32,
    /// Mouse Y movement (positive = up); key: 0.
    pub dy: i32,
}

const _: () = assert!(core::mem::size_of::<InputEvent>() == 24);

/// `kind` of a key event.
pub const KIND_KEY: u16 = 1;
/// `kind` of a relative mouse event.
pub const KIND_MOUSE: u16 = 2;
/// `kind` of the "events were lost" marker.
pub const KIND_LOST: u16 = 3;

impl InputEvent {
    /// The 24 bytes user space receives.
    pub fn to_bytes(self) -> [u8; 24] {
        let mut b = [0u8; 24];
        b[0..8].copy_from_slice(&self.time_ns.to_le_bytes());
        b[8..10].copy_from_slice(&self.kind.to_le_bytes());
        b[10..12].copy_from_slice(&self.code.to_le_bytes());
        b[12..16].copy_from_slice(&self.value.to_le_bytes());
        b[16..20].copy_from_slice(&self.dx.to_le_bytes());
        b[20..24].copy_from_slice(&self.dy.to_le_bytes());
        b
    }
}

/// Events kept for the reader.
pub const CAPACITY: usize = 256;

struct Ring {
    events: [InputEvent; CAPACITY],
    head: usize,
    len: usize,
    /// Events were dropped since the last read.
    lost: bool,
    /// Total dropped (diagnostics).
    dropped: u64,
}

impl Ring {
    fn clear(&mut self) {
        self.head = 0;
        self.len = 0;
        self.lost = false;
    }
}

static RING: IrqSpinLock<Ring> = IrqSpinLock::new(Ring {
    events: [InputEvent {
        time_ns: 0,
        kind: 0,
        code: 0,
        value: 0,
        dx: 0,
        dy: 0,
    }; CAPACITY],
    head: 0,
    len: 0,
    lost: false,
    dropped: 0,
});
static WAIT: WaitQueue = WaitQueue::new();
static GRANTED: AtomicBool = AtomicBool::new(false);
/// The grant holder has called `input_read` at least once.
static ACTIVE: AtomicBool = AtomicBool::new(false);

/// Publishes one event (called by input drivers, safe in IRQ handlers).
pub fn push(event: InputEvent) {
    {
        let mut ring = RING.lock();
        if ring.len == CAPACITY {
            ring.lost = true;
            ring.dropped += 1;
        } else {
            let tail = (ring.head + ring.len) % CAPACITY;
            ring.events[tail] = event;
            ring.len += 1;
        }
    }
    // Publish first, then wake (D-046).
    WAIT.wake_all();
}

/// A key event and the text it produces with the current layout (empty
/// for keys without text, or for releases). Safe in IRQ handlers. The
/// kernel decides where the text goes (see the module docs).
pub fn push_key(event: InputEvent, text: &[u8]) {
    push(event);
    if !ACTIVE.load(Ordering::Acquire) && !text.is_empty() {
        crate::console::input_push(text);
    }
}

/// Marks the grant holder as reading (first `input_read`): from now on
/// keyboard text no longer reaches the console.
pub fn mark_active() {
    ACTIVE.store(true, Ordering::Release);
}

/// Whether an INPUT reader is active (keys are not given to the console).
pub fn active() -> bool {
    ACTIVE.load(Ordering::Acquire)
}

/// Events dropped so far because the reader was too slow.
pub fn dropped() -> u64 {
    RING.lock().dropped
}

/// Empties the ring (the reader changes).
pub fn flush() {
    RING.lock().clear();
}

/// The right to read input. Only one exists at a time; dropping the last
/// reference (the handle is closed, or its process died) empties the ring
/// and lets the kernel grant it again.
#[derive(Debug)]
pub struct InputGrant {
    _private: (),
}

impl Drop for InputGrant {
    fn drop(&mut self) {
        flush();
        // Keys go back to the console until a new holder reads.
        ACTIVE.store(false, Ordering::Release);
        GRANTED.store(false, Ordering::Release);
    }
}

/// Creates the grant if none exists (the kernel gives it to `/sbin/init`).
pub fn grant() -> Option<Arc<InputGrant>> {
    if GRANTED.swap(true, Ordering::AcqRel) {
        return None;
    }
    // Start clean: nothing typed before this owner existed.
    flush();
    Some(Arc::new(InputGrant { _private: () }))
}

/// Wakes a blocked reader so it can re-check `cancel` (its process is
/// being killed).
pub fn wake_readers() {
    WAIT.wake_all();
}

/// Blocks until at least one event (or `cancel()`), then moves up to
/// `dst.len()` events into `dst`. A pending "lost" marker comes first.
/// Returns how many were written; 0 only if cancelled. Thread context only.
pub fn read(dst: &mut [InputEvent], mut cancel: impl FnMut() -> bool) -> usize {
    if dst.is_empty() {
        return 0;
    }
    loop {
        WAIT.wait_until(|| {
            let ready = {
                let ring = RING.lock();
                ring.len > 0 || ring.lost
            };
            ready || cancel()
        });
        if cancel() {
            return 0;
        }
        let mut ring = RING.lock();
        let mut n = 0;
        while n < dst.len() && ring.len > 0 {
            dst[n] = ring.events[ring.head];
            ring.head = (ring.head + 1) % CAPACITY;
            ring.len -= 1;
            n += 1;
        }
        // The "lost" marker comes after everything that was queued before
        // the loss (security review: before them, a reset would be undone
        // by old presses whose releases were dropped).
        if ring.len == 0
            && ring.lost
            && let Some(slot) = dst.get_mut(n)
        {
            *slot = InputEvent {
                kind: KIND_LOST,
                ..InputEvent::default()
            };
            ring.lost = false;
            n += 1;
        }
        drop(ring);
        if n > 0 {
            return n;
        }
    }
}
