//! Optional boot animation. Timer IRQs advance the clock without drawing.

use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering};

use kitty_pantufa::{BootSplash, Canvas, SCENE_HEIGHT, SCENE_WIDTH, Target, present_bgra};

use crate::arch::x86_64::cpu;
use crate::boot_info::FrameBuffer;
use crate::sync::SpinLock;

const TICK_MS: u32 = 1000 / super::pit::TICK_HZ;
const FRAME_TICKS: u32 = 4;

#[repr(u8)]
pub(crate) enum Stage {
    Memory,
    Interrupts,
    Drivers,
    Files,
}

struct Presentation {
    fb: Option<FrameBuffer>,
    scene: [u8; SCENE_WIDTH * SCENE_HEIGHT],
    splash: BootSplash,
    previous_tick: Option<u32>,
    frames: u32,
    max_cycles: u64,
}

static PRESENTATION: SpinLock<Presentation> = SpinLock::new(Presentation {
    fb: None,
    scene: [0; SCENE_WIDTH * SCENE_HEIGHT],
    splash: BootSplash::new(),
    previous_tick: None,
    frames: 0,
    max_cycles: 0,
});
static ACTIVE: AtomicBool = AtomicBool::new(false);
static CLOCK_READY: AtomicBool = AtomicBool::new(false);
static BOOT_READY: AtomicBool = AtomicBool::new(false);
static TICKS: AtomicU32 = AtomicU32::new(0);
static STAGES: AtomicU8 = AtomicU8::new(0);

pub(crate) fn begin(mut fb: FrameBuffer) -> Option<FrameBuffer> {
    if fb.width() < SCENE_WIDTH || fb.height() < SCENE_HEIGHT || fb.bgra_view().is_none() {
        return Some(fb);
    }
    PRESENTATION.lock().fb = Some(fb);
    ACTIVE.store(true, Ordering::Relaxed);
    None
}

pub(super) fn clock_ready() {
    CLOCK_READY.store(true, Ordering::Relaxed);
}

pub(super) fn tick() {
    if ACTIVE.load(Ordering::Relaxed) {
        TICKS.fetch_add(1, Ordering::Relaxed);
    }
}

pub(crate) fn stage(stage: Stage) {
    STAGES.fetch_or(1 << stage as u8, Ordering::Relaxed);
}

pub(crate) fn boot_ready() {
    BOOT_READY.store(true, Ordering::Relaxed);
    if !CLOCK_READY.load(Ordering::Relaxed) && ACTIVE.swap(false, Ordering::Relaxed) {
        let fb = PRESENTATION.lock().fb.take();
        if let Some(fb) = fb {
            crate::console::attach_framebuffer(fb);
        }
    }
}

/// Advances the presentation on the boot thread. Returns whether it is active.
pub(crate) fn refresh() -> bool {
    if !ACTIVE.load(Ordering::Relaxed) {
        return false;
    }
    if !CLOCK_READY.load(Ordering::Relaxed) {
        return true;
    }
    let ticks = TICKS.load(Ordering::Relaxed);
    let mut presentation = PRESENTATION.lock();
    let Presentation {
        fb,
        scene,
        splash,
        previous_tick,
        frames,
        max_cycles,
    } = &mut *presentation;
    if previous_tick.is_some_and(|previous| ticks.wrapping_sub(previous) < FRAME_TICKS) {
        return true;
    }
    *previous_tick = Some(ticks);
    let ready = BOOT_READY.load(Ordering::Relaxed);
    let stages = STAGES.load(Ordering::Relaxed).trailing_ones() as u8;
    let before = cpu::rdtsc();
    let presented = fb.as_mut().and_then(|fb| {
        let width = fb.width();
        let height = fb.height();
        let (pixels, pitch) = fb.bgra_view()?;
        let mut canvas = Canvas::new(SCENE_WIDTH, SCENE_HEIGHT, scene)?;
        let frame = splash.draw(&mut canvas, ticks.saturating_mul(TICK_MS), stages, ready);
        let target = Target {
            width,
            height,
            pitch,
        };
        present_bgra(&canvas, frame.brightness, pixels, target).ok()?;
        Some(frame)
    });
    if let Some(frame) = presented {
        *frames += 1;
        *max_cycles = (*max_cycles).max(cpu::rdtsc().wrapping_sub(before));
        if !frame.finished {
            return true;
        }
    }
    let (fb, frames, max_cycles) = (fb.take(), *frames, *max_cycles);
    ACTIVE.store(false, Ordering::Relaxed);
    drop(presentation);
    if let Some(fb) = fb {
        crate::console::attach_framebuffer(fb);
    }
    if presented.is_some() {
        crate::kprintln!(
            "[pantufa] animation complete ({frames} frames, max {max_cycles} TSC cycles/frame)"
        );
    } else {
        crate::kprintln!("[pantufa] presentation unavailable, using text console");
    }
    false
}
