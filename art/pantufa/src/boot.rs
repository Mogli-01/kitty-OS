//! The boot animation: Pantufa wakes up on his cushion, stretches, yawns and
//! jumps onto the "O" of the logo, while paw prints show the real boot
//! stages. The animation never delays the boot: it is drawn from a clock and
//! the boot progress, and it waits (crouched, wiggling) only if the system is
//! not ready by the time he would jump.
//!
//! Timeline (scene time, ms): 0–300 fade in · 0–1200 asleep, ear flick, one
//! eye opens · 1200–1500 stretch · 1500–1750 yawn, the logo appears ·
//! 1750–1950 crouch · 1950–2350 jump · 2350–2500 landing · 2500–3300
//! "MIAU!" and fade out.

use crate::canvas::{Canvas, bayer};
use crate::cat::{CatSprite, CatStyle, Eyes, Mouth, Pose, draw_cat};
use crate::font::{GLYPH_ADVANCE, text, text_width};
use crate::math::{floor, lerp, sin, sqrt};
use crate::palette::TRANSPARENT;
use crate::palette::idx::{
    BLANKET, CREAM, FUR, HIGHLIGHT, LAMP, NIGHT, OUTLINE, RAIN, SHADOW, WALL,
};

/// Base resolution of every scene. Present it with an integer scale.
pub const SCENE_WIDTH: usize = 320;
pub const SCENE_HEIGHT: usize = 180;
/// Number of paw prints: one per boot stage reported by the caller.
pub const BOOT_STAGES: u8 = 8;
/// Scene time at which the animation is over (fully faded out).
pub const BOOT_LENGTH_MS: u32 = 3300;

const CROUCH_START: u32 = 1750;
const JUMP_START: u32 = 1950;

const LOGO_TEXT: &str = "KITTY-OS";
const LOGO_SCALE: i32 = 2;
const LOGO_W: usize = 112;
const LOGO_H: usize = 24;
const LOGO_X: i32 = 150;
const LOGO_Y: i32 = 98;
/// Where the text sits inside the logo sprite (room for the outline).
const LOGO_INK_X: i32 = 3;
const LOGO_INK_Y: i32 = 5;

const CUSHION_X: f32 = 96.0;
const CUSHION_Y: f32 = 126.0;

const STAGE_NAMES: [&str; BOOT_STAGES as usize] = [
    "MEMORIA",
    "INTERRUPCOES",
    "DRIVERS",
    "FICHEIROS",
    "REDE",
    "FIREWALL",
    "INTERFACE",
    "LOGIN",
];

const PAW_PRINT: [&str; 5] = ["F.F.F", ".....", ".FFF.", "FFFFF", ".FFF."];

/// What the caller needs to present a frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BootFrame {
    /// Brightness in quarters (0 = black, 4 = full): pass it to
    /// [`crate::present_bgra`]. Fades are a palette operation.
    pub brightness: u8,
    /// The animation is over; show the login screen.
    pub finished: bool,
}

/// State and scratch buffers of the boot animation (about 12 KiB): keep it
/// in a static or on the heap, not on a small kernel stack.
pub struct BootSplash {
    cat: CatSprite,
    logo: [u8; LOGO_W * LOGO_H],
    shadow: [u8; LOGO_W * LOGO_H],
    ready_at: Option<u32>,
}

impl Default for BootSplash {
    fn default() -> Self {
        Self::new()
    }
}

impl BootSplash {
    pub const fn new() -> Self {
        Self {
            cat: CatSprite::new(),
            logo: [TRANSPARENT; LOGO_W * LOGO_H],
            shadow: [TRANSPARENT; LOGO_W * LOGO_H],
            ready_at: None,
        }
    }

    /// Converts the clock into scene time. Before the jump it is the clock.
    /// After that the scene only moves on once `ready` has been seen;
    /// until then it loops the crouch, so a slow boot shows a cat about to
    /// pounce instead of a frozen picture.
    fn scene_time(&mut self, t: u32, ready: bool) -> u32 {
        if ready && self.ready_at.is_none() {
            self.ready_at = Some(t.max(JUMP_START));
        }
        if t < JUMP_START {
            return t;
        }
        match self.ready_at {
            Some(r) if t >= r => JUMP_START.saturating_add(t - r),
            _ => CROUCH_START + (t - CROUCH_START) % (JUMP_START - CROUCH_START),
        }
    }

    /// Draws the frame at `t_ms` since the splash started. `stages_done`
    /// (0..=8) lights the paw prints; `ready` lets the cat jump.
    /// `canvas` must be [`SCENE_WIDTH`]×[`SCENE_HEIGHT`].
    pub fn draw(
        &mut self,
        canvas: &mut Canvas<'_>,
        t_ms: u32,
        stages_done: u8,
        ready: bool,
    ) -> BootFrame {
        let te = self.scene_time(t_ms, ready);
        canvas.clear(NIGHT);
        stars(canvas, t_ms);
        if te > BOOT_LENGTH_MS {
            return BootFrame {
                brightness: 0,
                finished: true,
            };
        }
        let tf = te as f32;
        let bright = if te < 300 {
            tf / 300.0
        } else if te > 3000 {
            1.0 - (tf - 3000.0) / 300.0
        } else {
            1.0
        };

        glow(canvas, tf);
        cushion(canvas, CUSHION_X, CUSHION_Y);
        let letters = if te < 1500 {
            0
        } else {
            ((te - 1500) / 40 + 1).min(8) as usize
        };
        let dip = if te > 2350 && te < 2440 { 2 } else { 0 };
        let gleam = (te > 2550 && te < 2950).then_some((tf - 2550.0) / 400.0 * 150.0 - 10.0);
        self.draw_logo(canvas, LOGO_Y + dip, letters, gleam);
        // The "O" is the 7th letter: its centre and the top of its ink.
        let o_x =
            (LOGO_X + LOGO_INK_X + 6 * GLYPH_ADVANCE * LOGO_SCALE + 5 * LOGO_SCALE / 2) as f32;
        let land_y = (LOGO_Y + LOGO_INK_Y) as f32;

        paw_prints(canvas, stages_done.min(BOOT_STAGES));
        self.draw_cat_frame(canvas, te, o_x, land_y, dip as f32);
        BootFrame {
            brightness: floor(bright * 4.0 + 0.5).clamp(0, 4) as u8,
            finished: te >= BOOT_LENGTH_MS,
        }
    }

    fn draw_cat_frame(&mut self, c: &mut Canvas<'_>, te: u32, o_x: f32, land_y: f32, dip: f32) {
        let tf = te as f32;
        let (cx, gy) = (CUSHION_X, CUSHION_Y - 3.0);
        let cat = &mut self.cat;
        if te < 1200 {
            let style = CatStyle {
                breathe: (te / 300) % 2 == 1,
                eyes: if te < 1000 {
                    Eyes::Closed
                } else {
                    Eyes::OneOpen
                },
                ear_twitch: te > 900 && te < 1000,
                ..CatStyle::new(Pose::Curl)
            };
            draw_cat(c, cat, cx, gy, &style);
            if te < 900 {
                for k in 0..3 {
                    let kf = k as f32;
                    let u = (tf / 700.0 + kf / 3.0) % 1.0;
                    let color = if u > 0.8 { WALL } else { RAIN };
                    zz(
                        c,
                        cx - 20.0 - u * 6.0 - kf * 2.0,
                        gy - 18.0 - u * 20.0 - kf * 3.0,
                        4 + k,
                        color,
                    );
                }
            }
        } else if te < 1500 {
            let style = CatStyle {
                stretch: if te < 1350 { 0.5 } else { 1.0 },
                tail: sin(tf / 60.0),
                ..CatStyle::new(Pose::Stretch)
            };
            draw_cat(c, cat, cx + 6.0, gy, &style);
        } else if te < 1750 {
            let style = CatStyle {
                mouth: if te < 1620 { Mouth::Open } else { Mouth::Yawn },
                ..CatStyle::new(Pose::Sit)
            };
            draw_cat(c, cat, cx, gy, &style);
        } else if te < JUMP_START {
            let wiggle = if (te / 50) % 2 == 1 { 1.0 } else { -1.0 };
            let style = CatStyle {
                eyes: Eyes::Open,
                scale_x: 1.12,
                scale_y: 0.84,
                tail: sin(tf / 40.0),
                ..CatStyle::new(Pose::Sit)
            };
            draw_cat(c, cat, cx + wiggle, gy, &style);
        } else if te < 2350 {
            let u = (tf - 1950.0) / 400.0;
            let x = lerp(cx, o_x, u);
            let y = lerp(gy, land_y, u) - sin(core::f32::consts::PI * u) * 42.0;
            let style = if u < 0.22 {
                CatStyle {
                    eyes: Eyes::Wide,
                    scale_x: 0.86,
                    scale_y: 1.2,
                    ..CatStyle::new(Pose::Sit)
                }
            } else if u < 0.8 {
                CatStyle {
                    eyes: Eyes::Happy,
                    mouth: Mouth::Open,
                    ..CatStyle::new(Pose::Ball)
                }
            } else {
                CatStyle {
                    eyes: Eyes::Wide,
                    scale_x: 0.9,
                    scale_y: 1.12,
                    ..CatStyle::new(Pose::Sit)
                }
            };
            draw_cat(c, cat, x, y, &style);
        } else if te < 2500 {
            let style = CatStyle {
                scale_x: 1.25,
                scale_y: 0.76,
                ..CatStyle::new(Pose::Sit)
            };
            draw_cat(c, cat, o_x, land_y + dip, &style);
        } else {
            let style = CatStyle {
                eyes: Eyes::Happy,
                tail: sin(tf / 90.0) * 1.2,
                ..CatStyle::new(Pose::Sit)
            };
            draw_cat(c, cat, o_x, land_y, &style);
            bubble(c, floor(o_x) + 12, floor(land_y) - 44, "MIAU!");
        }
        if te > 2350 && te < 2600 {
            dust(c, o_x, land_y, (tf - 2350.0) / 250.0);
        }
    }

    /// The logo: two-tone letters (cream over lamp amber), a highlight row,
    /// an outline, a drop shadow and an optional gleam sweeping across.
    fn draw_logo(&mut self, c: &mut Canvas<'_>, y: i32, letters: usize, gleam: Option<f32>) {
        if letters == 0 {
            return;
        }
        let Some(mut logo) = Canvas::new(LOGO_W, LOGO_H, &mut self.logo) else {
            return;
        };
        logo.clear(TRANSPARENT);
        let shown = &LOGO_TEXT[..letters.min(LOGO_TEXT.len())];
        text(&mut logo, shown, LOGO_INK_X, LOGO_INK_Y, CREAM, LOGO_SCALE);
        let lower_half = LOGO_INK_Y + 4 * LOGO_SCALE;
        for j in 0..LOGO_H as i32 {
            for i in 0..LOGO_W as i32 {
                if logo.get(i, j) != CREAM {
                    continue;
                }
                if j >= lower_half {
                    logo.set(i, j, LAMP);
                }
                if j == LOGO_INK_Y {
                    logo.set(i, j, HIGHLIGHT);
                }
                if let Some(g) = gleam {
                    let d = i as f32 - (j - LOGO_INK_Y) as f32 * 0.6 - g;
                    if (0.0..4.0).contains(&d) {
                        logo.set(i, j, if d < 1.5 { HIGHLIGHT } else { CREAM });
                    }
                }
            }
        }
        logo.outline(OUTLINE);
        let Some(mut shadow) = Canvas::new(LOGO_W, LOGO_H, &mut self.shadow) else {
            return;
        };
        shadow.clear(TRANSPARENT);
        shadow.blit(&logo, 0, 0, false);
        shadow.outline(OUTLINE);
        c.blit_tinted(&shadow, LOGO_X + 1, y + 2, SHADOW);
        c.blit(&logo, LOGO_X, y, false);
    }
}

fn stars(c: &mut Canvas<'_>, t_ms: u32) {
    let blink = t_ms / 350;
    for i in 0..40u32 {
        let (x, y) = ((i * 73) % 320, (i * 41) % 96 + 4);
        let color = if (i + blink) % 9 == 0 {
            RAIN
        } else if i % 3 == 0 {
            WALL
        } else {
            SHADOW
        };
        c.set(x as i32, y as i32, color);
    }
}

/// Warm dithered glow around the cushion that grows as he wakes up.
fn glow(c: &mut Canvas<'_>, tf: f32) {
    let strength = (0.25 + tf / 2000.0).clamp(0.0, 1.0);
    let (cx, cy, r) = (CUSHION_X as i32, CUSHION_Y as i32, 46.0);
    for y in cy - 50..cy + 14 {
        for x in cx - 80..cx + 170 {
            let dx = (x - cx - 40) as f32 / 2.2;
            let dy = (y - cy + 10) as f32;
            let d = sqrt(dx * dx + dy * dy) / r;
            if d < 1.0 && (bayer(x, y) as f32) / 16.0 < (1.0 - d) * (1.0 - d) * 0.5 * strength {
                c.set(x, y, SHADOW);
            }
        }
    }
}

fn cushion(c: &mut Canvas<'_>, x: f32, y: f32) {
    c.ellipse(x, y + 1.0, 29.0, 7.5, OUTLINE);
    c.ellipse(x, y, 28.0, 6.5, BLANKET);
    c.ellipse(x, y - 1.5, 23.0, 4.0, BLANKET);
    let (xi, yi) = (x as i32, y as i32);
    for j in yi - 3..yi - 1 {
        for i in xi - 20..xi + 20 {
            if i % 2 == 0 && j % 2 == 0 {
                c.set(i, j, CREAM);
            }
        }
    }
    c.ellipse(x, y + 3.5, 26.0, 2.2, WALL);
}

/// One paw print per finished boot stage, alternating left/right feet, and
/// the name of the stage under them.
fn paw_prints(c: &mut Canvas<'_>, done: u8) {
    for i in 0..BOOT_STAGES as i32 {
        let x = 107 + i * 14;
        let y = 150 + if i % 2 == 1 { 2 } else { -1 };
        if i < done as i32 {
            c.grid(&PAW_PRINT, x, y, Some((0xF, FUR)));
        } else {
            c.set(x + 2, y + 3, WALL);
        }
    }
    if done >= BOOT_STAGES {
        let w = text_width("PRONTO!", 1);
        text(c, "PRONTO!", 160 - w / 2, 164, RAIN, 1);
    } else {
        let name = STAGE_NAMES[done.saturating_sub(1) as usize];
        // "A ACORDAR:" + one space (1-pixel gap + a 6-pixel blank glyph).
        let prefix = "A ACORDAR:";
        let gap = 1 + GLYPH_ADVANCE;
        let w = text_width(prefix, 1) + gap + text_width(name, 1);
        let x = 160 - w / 2;
        let x = x + text(c, prefix, x, 164, RAIN, 1) + gap;
        text(c, name, x, 164, RAIN, 1);
    }
}

/// A "z" of `n`×`n` pixels with its top-left at `(x, y)`.
pub(crate) fn zz(c: &mut Canvas<'_>, x: f32, y: f32, n: i32, color: u8) {
    let (x, y) = (floor(x + 0.5), floor(y + 0.5));
    c.rect(x, y, n, 1, color);
    for i in 1..n - 1 {
        c.set(x + n - 1 - i, y + i, color);
    }
    c.rect(x, y + n - 1, n, 1, color);
}

/// Panel with a 1-pixel border and cut corners.
pub(crate) fn panel(c: &mut Canvas<'_>, x: i32, y: i32, w: i32, h: i32, fill: u8, border: u8) {
    c.rect(x + 1, y, w - 2, 1, border);
    c.rect(x + 1, y + h - 1, w - 2, 1, border);
    c.rect(x, y + 1, 1, h - 2, border);
    c.rect(x + w - 1, y + 1, 1, h - 2, border);
    c.rect(x + 1, y + 1, w - 2, h - 2, fill);
}

/// Speech bubble with its tail on the bottom-left.
pub(crate) fn bubble(c: &mut Canvas<'_>, x: i32, y: i32, s: &str) {
    let w = text_width(s, 1) + 9;
    panel(c, x, y, w, 13, CREAM, OUTLINE);
    let tx = x + 4;
    c.set(tx + 1, y + 12, CREAM);
    c.set(tx, y + 13, OUTLINE);
    c.set(tx + 1, y + 13, CREAM);
    c.set(tx + 2, y + 13, OUTLINE);
    c.set(tx + 1, y + 14, OUTLINE);
    text(c, s, x + 5, y + 3, OUTLINE, 1);
}

/// Puffs of dust on both sides of a landing, `u` from 0 to 1.
pub(crate) fn dust(c: &mut Canvas<'_>, x: f32, y: f32, u: f32) {
    for s in [-1.0, 1.0] {
        for k in 0..3 {
            let kf = k as f32;
            let r = (1.0 - u) * (2.2 - kf * 0.5);
            if r > 0.4 {
                let color = if k == 0 { CREAM } else { RAIN };
                c.ellipse(
                    x + s * (14.0 + u * 14.0 + kf * 5.0),
                    y - 1.0 - u * 4.0 - kf * 2.0,
                    r,
                    r * 0.8,
                    color,
                );
            }
        }
    }
}
