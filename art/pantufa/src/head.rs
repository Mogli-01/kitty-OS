//! Pantufa's head on its own: login screen, notifications, the start button.
//! At `scale >= 1.2` the eyes are big (white, pupil, highlight) and the
//! pupil can look in 9 directions; smaller heads use the 4-pixel body eyes.

use crate::canvas::Canvas;
use crate::math::round;
use crate::palette::TRANSPARENT;
use crate::palette::idx::{CREAM, FUR, FUR_MID, NIGHT, NOSE, OUTLINE, STRIPES};

pub const HEAD_SPRITE_WIDTH: usize = 80;
pub const HEAD_SPRITE_HEIGHT: usize = 72;
const ANCHOR: f32 = 40.0;
/// Largest head scale; bigger requests are clamped.
pub const MAX_HEAD_SCALE: f32 = 2.5;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HeadEyes {
    Open,
    Closed,
    Half,
    Happy,
    Wide,
    Angry,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HeadMouth {
    /// The little "w".
    Smile,
    Open,
    /// Flat and crooked: not amused.
    Hmpf,
    /// Open with fangs.
    Hiss,
}

/// Which paw covers the eyes while the password is typed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Paws {
    None,
    /// Both eyes covered.
    Cover,
    /// Covered, but the right paw slides down to peek.
    Peek,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HeadStyle {
    pub eyes: HeadEyes,
    pub mouth: HeadMouth,
    /// Where the pupils look: each axis -1, 0 or 1.
    pub look: (i8, i8),
    pub ear_flat: bool,
    /// Fur on end along the top of the head.
    pub bristle: bool,
    pub paws: Paws,
}

impl HeadStyle {
    pub const fn new() -> Self {
        Self {
            eyes: HeadEyes::Open,
            mouth: HeadMouth::Smile,
            look: (0, 0),
            ear_flat: false,
            bristle: false,
            paws: Paws::None,
        }
    }
}

impl Default for HeadStyle {
    fn default() -> Self {
        Self::new()
    }
}

/// Scratch for one head sprite (5 760 bytes).
pub struct HeadSprite {
    px: [u8; HEAD_SPRITE_WIDTH * HEAD_SPRITE_HEIGHT],
}

impl HeadSprite {
    pub const fn new() -> Self {
        Self {
            px: [TRANSPARENT; HEAD_SPRITE_WIDTH * HEAD_SPRITE_HEIGHT],
        }
    }
}

impl Default for HeadSprite {
    fn default() -> Self {
        Self::new()
    }
}

type Pt = (f32, f32);

struct HeadPen(f32);

impl HeadPen {
    fn at(&self, p: Pt) -> Pt {
        (ANCHOR + p.0 * self.0, ANCHOR + p.1 * self.0)
    }
    fn ellipse(&self, s: &mut Canvas<'_>, c: Pt, r: Pt, color: u8) {
        let (x, y) = self.at(c);
        s.ellipse(x, y, r.0 * self.0, r.1 * self.0, color);
    }
    fn triangle(&self, s: &mut Canvas<'_>, p: [Pt; 3], color: u8) {
        s.triangle([self.at(p[0]), self.at(p[1]), self.at(p[2])], color);
    }
    fn line(&self, s: &mut Canvas<'_>, a: Pt, b: Pt, color: u8) {
        s.line(self.at(a), self.at(b), color);
    }
}

/// Draws the head centred at `(x, y)`; `scale` 1.0 is about 20×18 pixels.
pub fn draw_head(
    target: &mut Canvas<'_>,
    sprite: &mut HeadSprite,
    x: f32,
    y: f32,
    scale: f32,
    style: &HeadStyle,
) {
    let Some(mut s) = Canvas::new(HEAD_SPRITE_WIDTH, HEAD_SPRITE_HEIGHT, &mut sprite.px) else {
        return;
    };
    s.clear(TRANSPARENT);
    let k = if scale.is_finite() {
        scale.clamp(0.4, MAX_HEAD_SCALE)
    } else {
        1.0
    };
    let p = HeadPen(k);
    if style.ear_flat {
        p.triangle(&mut s, [(-8.0, -1.0), (-13.5, -6.0), (-4.5, -5.5)], FUR);
        p.triangle(&mut s, [(8.0, -1.0), (13.5, -6.0), (4.5, -5.5)], FUR);
    } else {
        p.triangle(&mut s, [(-8.5, -3.0), (-7.5, -11.5), (-2.5, -6.0)], FUR);
        p.triangle(&mut s, [(8.5, -3.0), (7.5, -11.5), (2.5, -6.0)], FUR);
        p.triangle(&mut s, [(-7.0, -4.5), (-7.0, -9.6), (-4.0, -6.0)], NOSE);
        p.triangle(&mut s, [(7.0, -4.5), (7.0, -9.6), (4.0, -6.0)], NOSE);
    }
    // Cheek fluff.
    p.triangle(&mut s, [(-9.2, 0.5), (-11.4, 2.8), (-8.4, 3.6)], FUR_MID);
    p.triangle(&mut s, [(9.2, 0.5), (11.4, 2.8), (8.4, 3.6)], FUR_MID);
    p.ellipse(&mut s, (0.0, 0.4), (9.6, 7.2), FUR_MID);
    p.ellipse(&mut s, (0.0, -0.3), (9.1, 6.6), FUR);
    p.line(&mut s, (-3.0, -6.6), (-3.0, -4.6), STRIPES);
    p.line(&mut s, (2.5, -6.6), (2.5, -4.6), STRIPES);
    p.line(&mut s, (-0.3, -7.0), (-0.3, -5.0), STRIPES);
    p.ellipse(&mut s, (0.0, 3.0), (4.4, 2.5), CREAM);

    let ey = -0.4;
    for side in [-1i8, 1] {
        let (ex, eyy) = p.at((4.0 * side as f32, ey));
        if k >= 1.2 {
            big_eye(
                &mut s,
                ex,
                eyy,
                (1.25 * k).max(1.3),
                style.eyes,
                style.look,
                side,
            );
        } else {
            small_eye(&mut s, round(ex), round(eyy), style.eyes);
        }
    }
    p.triangle(&mut s, [(-1.1, 1.3), (1.1, 1.3), (0.0, 2.5)], NOSE);
    match style.mouth {
        HeadMouth::Smile => {
            p.line(&mut s, (0.0, 2.6), (-1.3, 3.6), OUTLINE);
            p.line(&mut s, (0.0, 2.6), (1.3, 3.6), OUTLINE);
        }
        HeadMouth::Open => {
            p.ellipse(&mut s, (0.0, 4.3), (1.7, 1.5), OUTLINE);
            p.ellipse(&mut s, (0.0, 4.4), (1.0, 0.8), NOSE);
        }
        HeadMouth::Hmpf => p.line(&mut s, (-1.4, 3.9), (1.4, 3.4), OUTLINE),
        HeadMouth::Hiss => {
            p.ellipse(&mut s, (0.0, 4.4), (2.4, 1.8), OUTLINE);
            p.ellipse(&mut s, (0.0, 4.5), (1.6, 1.0), NOSE);
            p.line(&mut s, (-1.0, 3.4), (-1.0, 3.9), CREAM);
            p.line(&mut s, (1.0, 3.4), (1.0, 3.9), CREAM);
        }
    }
    s.outline(OUTLINE);
    if style.bristle {
        s.spikes(OUTLINE);
    }
    if k >= 1.0 {
        p.line(&mut s, (-4.6, 2.6), (-11.5, 1.6), CREAM);
        p.line(&mut s, (-4.6, 3.5), (-11.5, 4.2), CREAM);
        p.line(&mut s, (4.6, 2.6), (11.5, 1.6), CREAM);
        p.line(&mut s, (4.6, 3.5), (11.5, 4.2), CREAM);
    }
    if style.paws != Paws::None {
        for side in [-1.0f32, 1.0] {
            let drop = if style.paws == Paws::Peek && side > 0.0 {
                3.0
            } else {
                0.0
            };
            let (px, py) = (4.0 * side, ey + drop);
            p.ellipse(&mut s, (px, py + 0.4), (3.6, 3.0), OUTLINE);
            p.ellipse(&mut s, (px, py + 0.4), (3.0, 2.4), CREAM);
            p.line(&mut s, (px - 1.2, py - 1.6), (px - 1.2, py - 0.4), OUTLINE);
            p.line(&mut s, (px + 1.0, py - 1.6), (px + 1.0, py - 0.4), OUTLINE);
            p.ellipse(&mut s, (px, py + 1.2), (1.1, 0.8), NOSE);
        }
    }
    target.blit(&s, round(x - ANCHOR), round(y - ANCHOR), false);
}

fn small_eye(s: &mut Canvas<'_>, x: i32, y: i32, eyes: HeadEyes) {
    match eyes {
        HeadEyes::Closed => {
            for (dx, dy) in [(-2, -1), (-1, 0), (0, 0), (1, -1)] {
                s.set(x + dx, y + dy, OUTLINE);
            }
        }
        HeadEyes::Happy => {
            for (dx, dy) in [(-2, 0), (-1, -1), (0, -1), (1, 0)] {
                s.set(x + dx, y + dy, OUTLINE);
            }
        }
        HeadEyes::Half | HeadEyes::Angry => {
            s.rect(x - 1, y, 2, 1, NIGHT);
            s.rect(x - 2, y - 1, 4, 1, OUTLINE);
        }
        HeadEyes::Wide => {
            s.rect(x - 2, y - 2, 4, 4, CREAM);
            s.rect(x - 1, y - 1, 2, 2, NIGHT);
        }
        HeadEyes::Open => {
            s.rect(x - 1, y - 1, 2, 2, NIGHT);
            s.set(x - 1, y - 1, CREAM);
        }
    }
}

/// A big eye of radius `r` at `(x, y)` in sprite pixels.
fn big_eye(s: &mut Canvas<'_>, x: f32, y: f32, r: f32, eyes: HeadEyes, look: (i8, i8), side: i8) {
    let arc = |s: &mut Canvas<'_>, height: f32| {
        let mut dx = -r;
        while dx <= r {
            let u = dx / r;
            s.set_at(x + dx, y + (1.0 - u * u) * height, OUTLINE);
            dx += 0.5;
        }
    };
    match eyes {
        HeadEyes::Closed => return arc(s, r * 0.45),
        HeadEyes::Happy => return arc(s, -r * 0.7),
        _ => {}
    }
    let rr = if eyes == HeadEyes::Wide { r * 1.25 } else { r };
    s.ellipse(x, y, rr + 0.9, rr * 1.15 + 0.9, OUTLINE);
    s.ellipse(x, y, rr, rr * 1.15, CREAM);
    let pr = if eyes == HeadEyes::Wide {
        rr * 0.38
    } else {
        rr * 0.62
    };
    let lx = f32::from(look.0.clamp(-1, 1));
    let ly = f32::from(look.1.clamp(-1, 1));
    let (cx, cy) = (x + lx * rr * 0.42, y + ly * rr * 0.38);
    s.ellipse(cx, cy, pr, pr * 1.25, NIGHT);
    s.set_at(cx - pr * 0.45, cy - pr * 0.7, CREAM);
    match eyes {
        HeadEyes::Half => {
            let top = y - rr * 1.2 - 2.0;
            s.rect(
                round(x - rr - 1.5),
                round(top),
                round(2.0 * rr + 3.0),
                round(y - top),
                FUR,
            );
            s.rect(
                round(x - rr - 0.5),
                round(y - 0.2),
                round(2.0 * rr + 1.4),
                1,
                OUTLINE,
            );
        }
        HeadEyes::Angry => {
            let top = y - rr * 1.3 - 1.5;
            let (l, r_) = (x - rr - 1.6, x + rr + 1.6);
            let inner = if side < 0 { r_ } else { l };
            let outer = if side < 0 { l } else { r_ };
            s.triangle([(l, top), (r_, top), (inner, y + 0.2)], FUR);
            s.line((outer, y - rr * 0.9), (inner, y - 0.3), OUTLINE);
        }
        _ => {}
    }
}
