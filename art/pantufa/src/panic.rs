//! The kernel panic screen: a small scene of what Pantufa "did" (knocked a
//! mug off the table, ate all the memory, knocked two mugs) above the
//! technical report given by the kernel.
//!
//! Made for the worst moment: no allocation, no clock needed (pass a large
//! `t_ms` such as `u32::MAX` to get the final still frame directly) and the
//! text lines are borrowed from the caller. The kernel decides what goes in
//! the lines (and must not leak addresses where user space can read them).

use crate::boot::{SCENE_HEIGHT, SCENE_WIDTH, bubble};
use crate::canvas::{Canvas, with_outline};
use crate::cat::{CatSprite, CatStyle, Eyes, Mouth, Pose, draw_cat};
use crate::font::{GLYPH_ADVANCE, text, text_width};
use crate::math::{floor, lerp, round, sin};
use crate::palette::TRANSPARENT;
use crate::palette::idx::{
    CREAM, FUR, FUR_MID, HIGHLIGHT, LAMP, NIGHT, NOSE, OUTLINE, RAIN, STRIPES, WALL,
};

/// Detail lines shown under the title (more are ignored).
pub const PANIC_MAX_LINES: usize = 7;
/// Longest text drawn per line (and for the code); the rest is cut off
/// before it is measured, so a huge string costs nothing extra.
pub const PANIC_MAX_CHARS: usize = 50;
/// Scene time after which the picture stops moving.
pub const PANIC_SCENE_MS: u32 = 2000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PanicKind {
    /// The mug goes off the table.
    PageFault,
    /// Pantufa, fatter, next to an empty bowl: "Burp."
    OutOfMemory,
    /// Two mugs, one each way.
    DoubleFault,
    /// Any other fatal error (same scene as a page fault).
    Other,
}

impl PanicKind {
    const fn title(self) -> &'static str {
        match self {
            PanicKind::PageFault | PanicKind::Other => "UPS. O PANTUFA DEITOU O KERNEL AO CHAO.",
            PanicKind::OutOfMemory => "UPS. O PANTUFA COMEU A MEMORIA TODA.",
            PanicKind::DoubleFault => "UPS. DUAS CANECAS. E UM DOUBLE FAULT.",
        }
    }

    const fn label(self) -> &'static str {
        match self {
            PanicKind::PageFault => "PAGE FAULT",
            PanicKind::OutOfMemory => "SEM MEMORIA",
            PanicKind::DoubleFault => "DOUBLE FAULT",
            PanicKind::Other => "ERRO FATAL",
        }
    }
}

/// Scratch for the scenery pieces (table, mugs, shards, bowl).
const SCRATCH: usize = 104 * 40;

/// About 11 KiB: keep it in a static so a panic never needs memory.
pub struct PanicScene {
    cat: CatSprite,
    scratch: [u8; SCRATCH],
}

impl PanicScene {
    /// Large: create it only in a `static` (this is a `const fn`), never
    /// as a local value, which would land on a small kernel stack.
    // No `Default` on purpose: it would invite building this on the stack.
    #[allow(clippy::new_without_default)]
    pub const fn new() -> Self {
        Self {
            cat: CatSprite::new(),
            scratch: [TRANSPARENT; SCRATCH],
        }
    }

    /// Draws the panic screen. In the kernel, keep both this scene and
    /// the 57 600-byte canvas buffer in `static`s reserved at boot (a panic
    /// may run on a 16 KiB IST stack) and reach them with `try_lock`, so a
    /// panic while drawing cannot deadlock.
    /// `code` is the short code in the corner (for
    /// example "0X0E"); `details` are up to [`PANIC_MAX_LINES`] lines of
    /// up to 50 characters (upper case, no accents in the 5×7 font).
    /// `t_ms` animates the fall; the scene is still after
    /// [`PANIC_SCENE_MS`] and only the cursor keeps blinking.
    pub fn draw(
        &mut self,
        c: &mut Canvas<'_>,
        kind: PanicKind,
        code: &str,
        details: &[&str],
        t_ms: u32,
    ) {
        c.clear(NIGHT);
        if c.width() != SCENE_WIDTH || c.height() != SCENE_HEIGHT {
            return;
        }
        let u = t_ms.min(PANIC_SCENE_MS) as f32;
        let wag = sin(u / 200.0);
        let s = &mut self.scratch[..];
        c.rect(10, 88, 200, 1, WALL);
        with_outline(c, s, (38, 50, 104, 40), |q| {
            q.rect(2, 2, 100, 4, STRIPES);
            q.rect(2, 6, 100, 1, OUTLINE);
            q.rect(8, 7, 4, 31, STRIPES);
            q.rect(92, 7, 4, 31, STRIPES);
        });
        let cat = &mut self.cat;
        match kind {
            PanicKind::PageFault | PanicKind::Other => {
                let style = CatStyle {
                    eyes: if u < 500.0 {
                        Eyes::Half
                    } else if u < 1100.0 {
                        Eyes::Wide
                    } else {
                        Eyes::Open
                    },
                    mouth: if u > 700.0 && u < 1100.0 {
                        Mouth::Open
                    } else {
                        Mouth::Closed
                    },
                    tail: wag,
                    ..CatStyle::new(Pose::Sit)
                };
                draw_cat(c, cat, 84.0, 53.0, &style);
                if u < 700.0 {
                    arm(c, s, 94, 46, false);
                }
                falling_mug(c, s, u, 500.0, 108.0 + u.min(500.0) / 120.0, 138.0, true);
            }
            PanicKind::OutOfMemory => {
                let fat = 1.0 + 0.08 * (u / 1500.0).min(1.0) + 0.02 * sin(u / 300.0);
                let style = CatStyle {
                    eyes: Eyes::Happy,
                    scale_x: 1.18 * fat,
                    scale_y: 1.02,
                    tail: wag,
                    ..CatStyle::new(Pose::Sit)
                };
                draw_cat(c, cat, 80.0, 53.0, &style);
                // The empty bowl.
                with_outline(c, s, (104, 40, 26, 14), |q| {
                    q.ellipse(13.0, 4.0, 12.0, 8.0, NOSE);
                    q.rect(0, 0, 26, 4, TRANSPARENT);
                    q.rect(1, 4, 24, 2, CREAM);
                    q.rect(8, 11, 10, 2, NOSE);
                });
                for k in 0..3 {
                    let p = (t_ms as f32 / 700.0 + k as f32 / 3.0) % 1.0;
                    if p < 0.5 {
                        c.set(96 + k * 6, floor(40.0 - p * 10.0), LAMP);
                    }
                }
                if (t_ms / 600) % 2 == 1 {
                    bubble(c, 58, 10, "BURP.");
                }
            }
            PanicKind::DoubleFault => {
                let style = CatStyle {
                    eyes: if u < 400.0 { Eyes::Half } else { Eyes::Wide },
                    mouth: if u > 600.0 && u < 1200.0 {
                        Mouth::Open
                    } else {
                        Mouth::Closed
                    },
                    tail: wag,
                    ..CatStyle::new(Pose::Sit)
                };
                draw_cat(c, cat, 90.0, 53.0, &style);
                if u < 700.0 {
                    arm(c, s, 100, 46, false);
                    arm(c, s, 80, 46, true);
                }
                falling_mug(c, s, u, 450.0, 114.0, 142.0, false);
                falling_mug(c, s, u, 650.0, 50.0, 24.0, false);
            }
        }

        let code = clip(code, 12);
        let cw = text_width(code, 2);
        text(c, code, 300 - cw, 24, NOSE, 2);
        let label = kind.label();
        text(c, label, 300 - text_width(label, 1), 44, RAIN, 1);
        let title = kind.title();
        text(c, title, 160 - text_width(title, 1) / 2, 94, LAMP, 1);
        let lines = &details[..details.len().min(PANIC_MAX_LINES)];
        for (i, line) in (0i32..).zip(lines) {
            let line = clip(line, PANIC_MAX_CHARS);
            text(
                c,
                line,
                14,
                108 + i * 10,
                if i == 0 { CREAM } else { RAIN },
                1,
            );
        }
        if (t_ms / 500) % 2 == 1 {
            let last = clip(lines.last().copied().unwrap_or(""), PANIC_MAX_CHARS);
            let row = lines.len().saturating_sub(1) as i32;
            let x = text_width(last, 1).saturating_add(14 + GLYPH_ADVANCE / 2);
            c.rect(x, 108 + row * 10, 4, 7, HIGHLIGHT);
        }
    }
}

/// The first `n` characters of `s`, without walking the rest of it.
fn clip(s: &str, n: usize) -> &str {
    s.char_indices().nth(n).map_or(s, |(i, _)| &s[..i])
}

/// A paw stretched out of the cat towards the edge of the table.
fn arm(c: &mut Canvas<'_>, s: &mut [u8], x: i32, y: i32, flip: bool) {
    let (dx, sign, o) = if flip {
        (-14, -1.0, 14.0)
    } else {
        (0, 1.0, 2.0)
    };
    with_outline(c, s, (x + dx, y - 4, 16, 8), |q| {
        q.ellipse(o + sign * 4.0, 4.0, 5.0, 2.2, FUR);
        q.ellipse(o + sign * 9.0, 4.0, 2.6, 1.8, CREAM);
    });
}

/// A mug pushed off at scene time `start` from `x0` that lands at `x1`
/// 300 ms later and breaks (`big`: more shards).
fn falling_mug(c: &mut Canvas<'_>, s: &mut [u8], u: f32, start: f32, x0: f32, x1: f32, big: bool) {
    if u < start {
        mug(c, s, round(x0), 38, 0);
    } else if u < start + 300.0 {
        let k = (u - start) / 300.0;
        let frame = if k < 0.5 { 1 } else { 2 };
        mug(
            c,
            s,
            round(lerp(x0, x1, k)),
            round(lerp(38.0, 74.0, k * k)),
            frame,
        );
    } else {
        crash(c, s, round(x1) + 8, u - start - 300.0, big);
    }
}

fn mug(c: &mut Canvas<'_>, s: &mut [u8], x: i32, y: i32, frame: u8) {
    with_outline(c, s, (x, y, 16, 14), |m| match frame {
        0 => {
            m.rect(2, 2, 9, 10, CREAM);
            m.rect(2, 2, 9, 2, STRIPES);
            m.ring(12.0, 7.0, 3.0, 3.0, 0.5, CREAM);
        }
        1 => {
            m.triangle([(1.0, 6.0), (8.0, 0.0), (14.0, 8.0)], CREAM);
            m.triangle([(1.0, 6.0), (14.0, 8.0), (7.0, 13.0)], CREAM);
            m.ellipse_rotated(5.0, 3.0, 2.5, 1.2, -0.7, STRIPES);
        }
        _ => {
            m.rect(2, 2, 9, 10, CREAM);
            m.rect(2, 10, 9, 2, STRIPES);
            m.ring(12.0, 7.0, 3.0, 3.0, 0.5, CREAM);
        }
    });
}

/// Puddle of coffee spreading, shards and a few drops at impact.
fn crash(c: &mut Canvas<'_>, s: &mut [u8], x: i32, u: f32, big: bool) {
    let g = (u / 600.0).clamp(0.0, 1.0);
    let xf = x as f32;
    c.ellipse(xf, 89.5, 10.0 + 12.0 * g, 2.0 + 1.2 * g, OUTLINE);
    c.ellipse(xf, 89.3, 9.0 + 11.0 * g, 1.4 + g, STRIPES);
    c.rect(x - 8, 89, 6, 1, FUR_MID);
    c.rect(x + 4, 89, 3, 1, FUR_MID);
    let shards = [
        (-20, 82, 0.0),
        (1, 81, 1.0),
        (14, 83, 0.0),
        (-10, 84, 1.0),
        (7, 85, 0.0),
    ];
    for (i, &(dx, y, f)) in shards.iter().enumerate() {
        if !big && i > 2 {
            break;
        }
        with_outline(c, s, (x + dx, y, 10, 8), |q| {
            if i == 1 {
                q.rect(1, 1, 7, 6, CREAM);
                q.rect(1, 1, 7, 2, STRIPES);
            } else {
                q.triangle([(0.0, 7.0), (3.0 + f * 2.0, 0.0), (8.0, 7.0)], CREAM);
            }
        });
    }
    if u < 200.0 {
        for (dx, y, col) in [
            (-22, 76, STRIPES),
            (22, 77, STRIPES),
            (-2, 73, STRIPES),
            (-13, 74, CREAM),
            (12, 72, CREAM),
        ] {
            c.set(x + dx, y, col);
        }
    }
}
