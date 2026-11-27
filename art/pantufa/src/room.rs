//! The wallpaper: a small room on a rainy night (or a sunny day, or a
//! sunset) with a window, a shelf, a floor lamp, a sofa and a mug.
//!
//! The static room is drawn once into [`Room`]'s own buffer by
//! [`Room::build`]; each frame copies it and adds only what moves (rain,
//! clouds or birds, the steam of the mug). Pantufa is drawn by the caller on
//! top, at [`Room::SOFA_SPOT`], so the same room serves the desktop, the
//! login screen, the screen saver and the shutdown animation.

use crate::boot::{SCENE_HEIGHT, SCENE_WIDTH};
use crate::canvas::{Canvas, bayer, with_outline};
use crate::cat::{CatStyle, Eyes, Pose};
use crate::math::{abs, floor, lerp, sin};
use crate::palette::TRANSPARENT;
use crate::palette::idx::{
    BLANKET, CREAM, FUR_MID, GLASS, HIGHLIGHT, LAMP, NIGHT, NOSE, OUTLINE, PLANT, RAIN, SHADOW,
    STRIPES, WALL,
};

/// What the window shows. Pick it from the real hour if you like.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Sky {
    Night,
    Sunset,
    Day,
}

impl Sky {
    /// The two colours of this sky: only pixels of these get weather.
    const fn colors(self) -> [u8; 2] {
        match self {
            Sky::Night => [NIGHT, SHADOW],
            Sky::Sunset => [NOSE, LAMP],
            Sky::Day => [GLASS, RAIN],
        }
    }
}

const SCENE: usize = SCENE_WIDTH * SCENE_HEIGHT;
/// Largest piece drawn with its own outline (the sofa, 138×52).
const SCRATCH: usize = 138 * 52;

/// Glass of the window (x, y, w, h) and the posts that split it.
const GLASS_X: i32 = 48;
const GLASS_Y: i32 = 20;
const GLASS_W: i32 = 86;
const GLASS_H: i32 = 70;

/// The static room plus a scratch buffer (about 64 KiB): keep it in a
/// static or on the heap.
pub struct Room {
    base: [u8; SCENE],
    scratch: [u8; SCRATCH],
    sky: Sky,
    lamp_on: bool,
    built: bool,
}

impl Room {
    /// Where Pantufa sleeps on the sofa (feet point for `draw_cat`).
    pub const SOFA_SPOT: (f32, f32) = (232.0, 119.0);
    /// The window sill, where he sits to watch the rain.
    pub const SILL_SPOT: (f32, f32) = (91.0, 93.0);

    /// Large: create it only in a `static` (this is a `const fn`), never
    /// as a local value, which would land on a small kernel stack.
    // No `Default` on purpose: it would invite building this on the stack.
    #[allow(clippy::new_without_default)]
    pub const fn new() -> Self {
        Self {
            base: [TRANSPARENT; SCENE],
            scratch: [TRANSPARENT; SCRATCH],
            sky: Sky::Night,
            lamp_on: true,
            built: false,
        }
    }

    pub fn sky(&self) -> Sky {
        self.sky
    }

    pub fn lamp_on(&self) -> bool {
        self.lamp_on
    }

    /// Redraws the static room if the sky or the lamp changed (or on first
    /// use). Cheap to call every frame: it does nothing when unchanged.
    pub fn build(&mut self, sky: Sky, lamp_on: bool) {
        if self.built && sky == self.sky && lamp_on == self.lamp_on {
            return;
        }
        self.sky = sky;
        self.lamp_on = lamp_on;
        self.built = true;
        let Some(mut b) = Canvas::new(SCENE_WIDTH, SCENE_HEIGHT, &mut self.base) else {
            return;
        };
        draw_walls_and_floor(&mut b, lamp_on);
        draw_window(&mut b, sky);
        let scratch = &mut self.scratch[..];
        with_outline(&mut b, scratch, (158, 36, 82, 30), draw_shelf);
        with_outline(&mut b, scratch, (280, 44, 32, 98), |t| {
            draw_lamp(t, lamp_on)
        });
        with_outline(&mut b, scratch, (136, 98, 30, 42), draw_table_and_mug);
        with_outline(&mut b, scratch, (166, 90, 138, 52), draw_sofa);
    }

    /// Copies the room into `scene` (320×180) and adds what moves at
    /// `t_ms`. Builds the default room (night, lamp on) if never built.
    pub fn draw(&mut self, scene: &mut Canvas<'_>, t_ms: u32) {
        if !self.built {
            self.build(Sky::Night, true);
        }
        if scene.width() != SCENE_WIDTH || scene.height() != SCENE_HEIGHT {
            return;
        }
        scene.clear(TRANSPARENT);
        if let Some(base) = Canvas::new(SCENE_WIDTH, SCENE_HEIGHT, &mut self.base) {
            scene.blit(&base, 0, 0, false);
        }
        weather(scene, self.sky, t_ms);
        // Steam of the mug: 3 frames.
        let f = ((t_ms / 250) % 3) as i32;
        for k in 0..3 {
            scene.set(150 + (k + f) % 2, 96 - k * 2 - f, RAIN);
        }
    }

    /// How Pantufa sleeps on the sofa at `t_ms`: breathing every 1.2 s, a
    /// tail flick every 6 s, one eye peeking every 9 s, an ear twitch
    /// every 23 s. Draw it with `draw_cat` at [`Room::SOFA_SPOT`].
    pub fn sleeping_cat(t_ms: u32) -> CatStyle {
        let flick = t_ms % 6000;
        CatStyle {
            breathe: (t_ms / 1200) % 2 == 1,
            eyes: if t_ms % 9000 < 900 {
                Eyes::OneOpen
            } else {
                Eyes::Closed
            },
            ear_twitch: t_ms % 23_000 < 140,
            tail: if flick < 450 {
                sin(flick as f32 / 450.0 * core::f32::consts::TAU) * 1.5
            } else {
                0.0
            },
            ..CatStyle::new(Pose::Curl)
        }
    }
}

fn draw_walls_and_floor(b: &mut Canvas<'_>, lamp_on: bool) {
    b.clear(TRANSPARENT);
    b.rect(0, 0, 320, 140, WALL);
    for x in (8..320).step_by(20) {
        b.rect(x, 0, 1, 136, SHADOW);
    }
    b.rect(0, 136, 320, 4, OUTLINE);
    b.rect(0, 140, 320, 40, OUTLINE);
    for y in (150..180).step_by(12) {
        b.rect(0, y, 320, 1, STRIPES);
        let start = if y % 24 != 0 { 10 } else { 30 };
        for x in (start..320).step_by(40) {
            b.rect(x, y - 11, 1, 11, STRIPES);
        }
    }
    if lamp_on {
        // Cone of lamp light: dithered, strongest near the shade and in
        // the middle of the cone.
        let lit = |c: &mut Canvas<'_>, x: i32, y: i32| {
            let d = ((y - 63) as f32 / 77.0).clamp(0.0, 1.0);
            let (cx, hw) = (lerp(296.0, 267.0, d), lerp(8.0, 53.0, d));
            let side = abs(x as f32 - cx) / hw;
            let k = 0.32 * (1.0 - d * 0.7) * (1.0 - side * side);
            if (bayer(x, y) as f32) / 16.0 < k {
                match c.get(x, y) {
                    WALL => c.set(x, y, LAMP),
                    SHADOW => c.set(x, y, WALL),
                    _ => {}
                }
            }
        };
        b.triangle_each([(288.0, 63.0), (304.0, 63.0), (214.0, 140.0)], lit);
        b.triangle_each([(304.0, 63.0), (320.0, 140.0), (214.0, 140.0)], lit);
    }
    // Rug with little fish.
    b.ellipse(214.0, 162.0, 74.0, 12.0, NOSE);
    b.ellipse(214.0, 162.0, 71.0, 10.5, BLANKET);
    for i in (-60..=60).step_by(15) {
        b.rect(214 + i, 161, 4, 2, NOSE);
        b.set(214 + i + 4, 160, NOSE);
        b.set(214 + i + 4, 163, NOSE);
    }
}

fn draw_window(b: &mut Canvas<'_>, sky: Sky) {
    let [s1, s2] = sky.colors();
    b.rect(44, 16, 94, 78, OUTLINE);
    b.rect(GLASS_X, GLASS_Y, GLASS_W, GLASS_H, s1);
    b.dither_rect(GLASS_X, 52, GLASS_W, 20, s2, false);
    b.rect(GLASS_X, 72, GLASS_W, 18, s2);
    match sky {
        Sky::Night => {
            b.ellipse(120.0, 34.0, 6.0, 6.0, CREAM);
            b.ellipse(118.0, 33.0, 1.5, 1.5, RAIN);
            b.ellipse(122.0, 37.0, 1.0, 1.0, RAIN);
        }
        Sky::Sunset => {
            b.ellipse(110.0, 70.0, 10.0, 10.0, HIGHLIGHT);
            b.rect(GLASS_X, 72, GLASS_W, 18, LAMP);
        }
        Sky::Day => {
            b.ellipse(118.0, 32.0, 6.0, 6.0, HIGHLIGHT);
            b.set(116, 30, CREAM);
        }
    }
    let city = [
        (48, 74, 12, 16),
        (62, 66, 10, 24),
        (74, 78, 14, 12),
        (90, 70, 9, 20),
        (101, 62, 8, 28),
        (111, 76, 12, 14),
        (124, 68, 10, 22),
    ];
    for (x, y, w, h) in city {
        b.rect(x, y, w, h, if sky == Sky::Day { WALL } else { SHADOW });
        if sky != Sky::Day {
            for j in (y + 3..y + h).step_by(4) {
                for i in (x + 2..x + w - 1).step_by(3) {
                    if (i * 7 + j * 3) % 5 == 0 {
                        b.set(i, j, LAMP);
                    }
                }
            }
        }
    }
    b.rect(89, 20, 3, 70, OUTLINE);
    b.rect(48, 53, 86, 3, OUTLINE);
    b.rect(40, 93, 102, 4, STRIPES);
    b.rect(40, 97, 102, 1, OUTLINE);
}

fn draw_shelf(t: &mut Canvas<'_>) {
    t.rect(2, 22, 78, 3, STRIPES);
    t.rect(8, 25, 3, 4, STRIPES);
    t.rect(70, 25, 3, 4, STRIPES);
    for (x, y, w, h, c) in [
        (6, 8, 6, 14, BLANKET),
        (12, 10, 5, 12, GLASS),
        (17, 6, 6, 16, PLANT),
        (23, 11, 5, 11, NOSE),
    ] {
        t.rect(x, y, w, h, c);
    }
    t.triangle([(29.0, 22.0), (34.0, 9.0), (38.0, 11.0)], LAMP);
    t.triangle([(29.0, 22.0), (38.0, 11.0), (33.0, 22.0)], LAMP);
    t.rect(55, 14, 12, 8, STRIPES);
    t.rect(54, 13, 14, 2, FUR_MID);
    t.ellipse(61.0, 9.0, 6.0, 4.0, PLANT);
    t.ellipse_rotated(55.0, 5.0, 3.0, 5.0, -0.6, PLANT);
    t.ellipse_rotated(67.0, 5.0, 3.0, 5.0, 0.6, PLANT);
}

fn draw_lamp(t: &mut Canvas<'_>, on: bool) {
    let (shade, top, rim) = if on {
        (LAMP, HIGHLIGHT, FUR_MID)
    } else {
        (RAIN, CREAM, WALL)
    };
    t.ellipse(16.0, 95.0, 8.0, 2.0, OUTLINE);
    t.rect(15, 19, 2, 76, OUTLINE);
    t.triangle([(6.0, 18.0), (26.0, 18.0), (21.0, 4.0)], shade);
    t.triangle([(6.0, 18.0), (21.0, 4.0), (11.0, 4.0)], shade);
    t.rect(11, 4, 10, 1, top);
    t.rect(6, 17, 20, 1, rim);
}

fn draw_table_and_mug(t: &mut Canvas<'_>) {
    t.rect(2, 12, 26, 3, STRIPES);
    t.rect(5, 15, 2, 25, STRIPES);
    t.rect(23, 15, 2, 25, STRIPES);
    t.rect(10, 3, 9, 9, CREAM);
    t.rect(10, 3, 9, 2, STRIPES);
    t.ring(21.0, 7.0, 3.0, 3.0, 0.5, CREAM);
}

fn draw_sofa(t: &mut Canvas<'_>) {
    t.rect(14, 6, 110, 24, GLASS);
    t.ellipse(42.0, 8.0, 28.0, 5.0, GLASS);
    t.ellipse(98.0, 8.0, 28.0, 5.0, GLASS);
    t.rect(14, 24, 110, 4, WALL);
    t.rect(8, 28, 122, 12, GLASS);
    t.rect(12, 28, 114, 1, RAIN);
    t.rect(8, 37, 122, 4, WALL);
    t.rect(16, 41, 3, 6, OUTLINE);
    t.rect(118, 41, 3, 6, OUTLINE);
    t.ellipse(12.0, 31.0, 8.0, 11.0, GLASS);
    t.ellipse(126.0, 31.0, 8.0, 11.0, GLASS);
    t.rect(6, 24, 12, 1, RAIN);
    t.rect(120, 24, 12, 1, RAIN);
    // Blanket over the back, with a fringe and stripes.
    t.ellipse_rotated(102.0, 15.0, 20.0, 10.0, 0.12, BLANKET);
    for i in (84..122).step_by(3) {
        t.set(i, 25 + (i % 2), BLANKET);
    }
    t.rect(86, 12, 30, 1, NOSE);
    t.rect(88, 17, 28, 1, NOSE);
    // Pink cushion.
    t.ellipse(32.0, 21.0, 10.0, 7.5, NOSE);
    t.rect(26, 21, 12, 1, BLANKET);
}

/// Rain and splashes at night, drifting clouds by day, birds at sunset.
/// Paints only over sky pixels, so the weather passes behind the city and
/// the window posts for free.
fn weather(b: &mut Canvas<'_>, sky: Sky, t_ms: u32) {
    let colors = sky.colors();
    let paint = |b: &mut Canvas<'_>, x: f32, y: f32, c: u8| {
        let (x, y) = (floor(x), floor(y));
        let inside =
            (GLASS_X..GLASS_X + GLASS_W).contains(&x) && (GLASS_Y..GLASS_Y + GLASS_H).contains(&y);
        if inside && colors.contains(&b.get(x, y)) {
            b.set(x, y, c);
        }
    };
    let t = t_ms as f32;
    match sky {
        Sky::Night => {
            for i in 0..36u32 {
                let x = 48 + ((i * 37) % 86) as i32;
                if (89..=91).contains(&x) {
                    continue;
                }
                let speed = 0.09 * (1.0 + (i % 3) as f32 * 0.3);
                let y = 20.0 + ((i * 53) as f32 + t * speed) % 70.0;
                for k in 0..3 {
                    let yy = floor(y) + k;
                    if yy < 90 && !(53..=55).contains(&yy) {
                        b.set(x, yy, RAIN);
                    }
                }
            }
            for k in 0..4u32 {
                let kf = k as f32;
                let p = (t / 260.0 + kf * 0.37) % 1.0;
                let hop = floor(t / 1040.0 + kf * 0.37).max(0) as u32;
                let x = 44 + ((k * 29 + hop * 17) % 94) as i32;
                if p < 0.35 {
                    b.set(x, 92, RAIN);
                    b.set(x - 1, 91, RAIN);
                    b.set(x + 1, 91, RAIN);
                } else if p < 0.55 {
                    b.set(x, 91, RAIN);
                }
            }
        }
        Sky::Day => {
            for k in 0..3 {
                let kf = k as f32;
                let cx = 40.0 + (t * 0.005 + kf * 47.0) % 130.0;
                let cy = 28.0 + kf * 11.0;
                let rx = 8.0 - kf;
                for y in -4..=3 {
                    for x in -(rx as i32) - 5..=(rx as i32) + 5 {
                        let (xf, yf) = (x as f32, y as f32);
                        let body = (xf / rx) * (xf / rx) + (yf / 3.2) * (yf / 3.2) <= 1.0;
                        let (px, py) = ((xf - rx * 0.6) / (rx * 0.6), (yf + 2.0) / 2.6);
                        let puff = px * px + py * py <= 1.0;
                        if body || puff {
                            paint(b, cx + xf, cy + yf, if y > 1 { RAIN } else { CREAM });
                        }
                    }
                }
            }
        }
        Sky::Sunset => {
            for k in 0..3 {
                let kf = k as f32;
                let x = 40.0 + (t * 0.022 + kf * 33.0) % 104.0;
                let y = 34.0 + kf * 8.0 + sin(t / 400.0 + kf) * 2.0;
                let up = (floor(t / 160.0) + k) % 2 == 1;
                let wings: [(f32, f32); 5] = if up {
                    [(0.0, 1.0), (1.0, 0.0), (2.0, 1.0), (3.0, 0.0), (4.0, 1.0)]
                } else {
                    [(0.0, 0.0), (1.0, 1.0), (2.0, 1.0), (3.0, 1.0), (4.0, 0.0)]
                };
                for (i, j) in wings {
                    paint(b, x + i, y + j, OUTLINE);
                }
            }
        }
    }
}
