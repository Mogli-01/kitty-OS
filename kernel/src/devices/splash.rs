//! Optional boot animation before the console takes the framebuffer.

use kitty_hal::PortBlock;
use kitty_pit::{Period, Pit};

use crate::arch::legacy::{self, LegacyDevice};
use crate::boot_info::FrameBuffer;

const WIDTH: usize = 640;
const HEIGHT: usize = 400;
const PALETTE_BYTES: usize = 128 * 3;
const ART: &[u8] = include_bytes!("../../../assets/boot/kitty-welcome.rle");
const PERIOD: Period = match Period::from_hz(50) {
    Some(period) => period,
    None => panic!("unsupported splash clock rate"),
};
const FRAMES: usize = 30;
const TICKS_PER_FRAME: usize = 10;
const MAX_POLLS: usize = 1_000_000;

/// Called at boot with interrupts disabled, before the PIC and scheduler
/// take ownership of channel 0. Its temporary port grant ends before boot
/// proceeds. A missing timer or an unsuitable display skips the animation.
pub(crate) fn show(fb: &mut FrameBuffer) -> bool {
    if fb.width() < WIDTH || fb.height() < HEIGHT {
        return false;
    }
    let Some(ports) = legacy::claim(LegacyDevice::Pit) else {
        return false;
    };
    let mut pit = Pit::new(ports);
    let played = play(fb, &mut pit);
    let stopped = pit.stop().is_ok();
    played && stopped
}

fn play(fb: &mut FrameBuffer, pit: &mut Pit<impl PortBlock>) -> bool {
    let x = (fb.width() - WIDTH) / 2;
    let y = (fb.height() - HEIGHT) / 2;
    fb.fill_rect(0, 0, fb.width(), fb.height(), fb.color(22, 27, 39));
    if !paint_art(fb, x, y) || pit.start_periodic(PERIOD).is_err() {
        return false;
    }
    let Ok(mut previous) = pit.latch_count() else {
        return false;
    };
    if !wait_for_reload(pit, &mut previous) {
        return false;
    }

    let track = fb.color(48, 57, 74);
    let accent = fb.color(243, 180, 119);
    fb.fill_rect(x + 278, y + 328, 84, 4, track);
    for frame in 0..FRAMES {
        let blink = matches!(frame, 8 | 22);
        if blink || matches!(frame, 9 | 23) {
            paint_eyes(fb, x, y, blink);
        }
        fb.fill_rect(x + 278, y + 328, 84 * (frame + 1) / FRAMES, 4, accent);
        for _ in 0..TICKS_PER_FRAME {
            if !wait_for_reload(pit, &mut previous) {
                return false;
            }
        }
    }
    true
}

fn wait_for_reload(pit: &mut Pit<impl PortBlock>, previous: &mut u16) -> bool {
    for _ in 0..MAX_POLLS {
        let Ok(current) = pit.latch_count() else {
            return false;
        };
        let reloaded = current > *previous && u32::from(current) <= PERIOD.divisor();
        *previous = current;
        if reloaded {
            return true;
        }
    }
    false
}

fn paint_art(fb: &mut FrameBuffer, left: usize, top: usize) -> bool {
    let Some((palette, runs)) = ART.split_at_checked(PALETTE_BYTES) else {
        return false;
    };
    let mut palette_values = [0; PALETTE_BYTES / 3];
    for (value, &[red, green, blue]) in palette_values.iter_mut().zip(palette.as_chunks::<3>().0) {
        *value = fb.color(red, green, blue);
    }
    let mut x = 0;
    let mut y = 0;
    let (runs, remainder) = runs.as_chunks::<2>();
    for &[count, index] in runs {
        let count = usize::from(count);
        let Some(&colour) = palette_values.get(usize::from(index)) else {
            return false;
        };
        if count == 0 || y >= HEIGHT || x + count > WIDTH {
            return false;
        }
        fb.fill_rect(left + x, top + y, count, 1, colour);
        x += count;
        if x == WIDTH {
            x = 0;
            y += 1;
        }
    }
    remainder.is_empty() && y == HEIGHT
}

fn paint_eyes(fb: &mut FrameBuffer, x: usize, y: usize, blink: bool) {
    let ink = fb.color(22, 27, 39);
    let fur = fb.color(243, 180, 119);
    if blink {
        for eye in [280, 344] {
            fb.fill_rect(x + eye, y + 85, 16, 16, fur);
            fb.fill_rect(x + eye, y + 92, 16, 4, ink);
        }
    } else {
        let shine = fb.color(244, 237, 231);
        for eye in [280, 344] {
            fb.fill_rect(x + eye, y + 85, 16, 16, ink);
        }
        fb.fill_rect(x + 288, y + 93, 8, 8, shine);
        fb.fill_rect(x + 344, y + 93, 8, 8, shine);
    }
}
