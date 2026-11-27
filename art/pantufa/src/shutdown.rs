//! The shutdown animation (3 s): the taskbar slides away, Pantufa hops off
//! the sofa, walks to his basket, turns round three times, yawns and falls
//! asleep; the lamp clicks off and only moonlight is left.
//!
//! Like the boot screen it never holds the system back: if the system is
//! still syncing when the cat is asleep, the "Zzz" keeps looping until
//! `done`, and only then the screen fades to black.
//!
//! Timeline (ms): 0–300 taskbar out, wakes on the sofa · 300–620 hop down ·
//! 620–1000 walk · 1000–1600 three turns · 1600–2400 lie down and yawn ·
//! 2400 lamp off · 2500–3000 asleep in the moonlight, fade.

use crate::boot::{BootFrame, SCENE_HEIGHT, SCENE_WIDTH, zz};
use crate::canvas::{Canvas, bayer};
use crate::cat::{CatSprite, CatStyle, Eyes, Mouth, Pose, draw_cat};
use crate::font::{text, text_width};
use crate::head::{HeadEyes, HeadSprite, HeadStyle, draw_head};
use crate::math::{floor, lerp, sin};
use crate::palette::DARKER;
use crate::palette::idx::{BLANKET, CREAM, LAMP, OUTLINE, RAIN, SHADOW, STRIPES, WALL};
use crate::room::{Room, Sky};

/// Scene time at which the screen is black.
pub const SHUTDOWN_LENGTH_MS: u32 = 3000;
const SLEEP_START: u32 = 2600;
const FADE_START: u32 = 2700;

const BED_X: f32 = 140.0;
const BED_Y: f32 = 163.0;

/// State and scratch buffers (about 77 KiB with the room): static or heap.
pub struct Shutdown {
    room: Room,
    cat: CatSprite,
    head: HeadSprite,
    done_at: Option<u32>,
}

impl Shutdown {
    /// Large: create it only in a `static` (this is a `const fn`), never
    /// as a local value, which would land on a small kernel stack.
    // No `Default` on purpose: it would invite building this on the stack.
    #[allow(clippy::new_without_default)]
    pub const fn new() -> Self {
        Self {
            room: Room::new(),
            cat: CatSprite::new(),
            head: HeadSprite::new(),
            done_at: None,
        }
    }

    /// Clock to scene time: once asleep, loop the "Zzz" until `done`.
    fn scene_time(&mut self, t: u32, done: bool) -> u32 {
        if done && self.done_at.is_none() {
            self.done_at = Some(t.max(SLEEP_START));
        }
        if t < SLEEP_START {
            return t;
        }
        match self.done_at {
            Some(d) if t >= d => SLEEP_START.saturating_add(t - d),
            _ => SLEEP_START + (t - SLEEP_START) % (FADE_START - SLEEP_START),
        }
    }

    /// Draws the frame at `t_ms` since shutdown began. `done` means the
    /// system has finished (disks synced, processes stopped): power off
    /// when `finished` is set. `scene` must be 320×180.
    pub fn draw(&mut self, scene: &mut Canvas<'_>, t_ms: u32, done: bool) -> BootFrame {
        let te = self.scene_time(t_ms, done);
        if scene.width() != SCENE_WIDTH || scene.height() != SCENE_HEIGHT {
            return BootFrame {
                brightness: 0,
                finished: te >= SHUTDOWN_LENGTH_MS,
            };
        }
        let lamp_on = te < 2400;
        self.room.build(Sky::Night, lamp_on);
        self.room.draw(scene, t_ms);
        if te >= SHUTDOWN_LENGTH_MS {
            return BootFrame {
                brightness: 0,
                finished: true,
            };
        }
        let tf = te as f32;
        let dims = if te < 250 {
            0
        } else if lamp_on {
            1
        } else {
            2
        };
        for _ in 0..dims {
            scene.remap(&DARKER);
        }
        if !lamp_on {
            moonlight(scene);
        }
        if te < 300 {
            self.taskbar(scene, tf);
        }
        bed_back(scene, BED_X, BED_Y);
        self.draw_cat_frame(scene, te, t_ms);
        bed_front(scene, BED_X, BED_Y);
        let w = text_width("PANTUFA", 1);
        text(
            scene,
            "PANTUFA",
            BED_X as i32 - w / 2,
            BED_Y as i32 + 2,
            LAMP,
            1,
        );
        if te > 2500 {
            for k in 0..3 {
                let kf = k as f32;
                let u = ((t_ms as f32 - 2500.0) / 450.0 + kf / 3.0) % 1.0;
                let color = if u > 0.8 { WALL } else { RAIN };
                zz(
                    scene,
                    BED_X - 14.0 - u * 6.0 - kf * 2.0,
                    BED_Y - 20.0 - u * 22.0 - kf * 4.0,
                    4 + k * 2,
                    color,
                );
            }
            let msg = "ATÉ AMANHÃ. MIAU.";
            let w = text_width(msg, 1);
            text(scene, msg, 160 - w / 2, 10, RAIN, 1);
        }
        let bright = if te < FADE_START {
            1.0
        } else {
            1.0 - (tf - FADE_START as f32) / (SHUTDOWN_LENGTH_MS - FADE_START) as f32
        };
        BootFrame {
            brightness: floor(bright * 4.0 + 0.5).clamp(0, 4) as u8,
            finished: false,
        }
    }

    /// The taskbar sliding down out of the screen.
    fn taskbar(&mut self, c: &mut Canvas<'_>, tf: f32) {
        let o = floor(tf / 300.0 * 16.0 + 0.5);
        c.rect(0, 165 + o, 320, 1, STRIPES);
        c.rect(0, 166 + o, 320, 15, OUTLINE);
        let style = HeadStyle {
            eyes: HeadEyes::Half,
            ..HeadStyle::new()
        };
        draw_head(c, &mut self.head, 11.0, 174.0 + o as f32, 0.62, &style);
        let msg = "A DESLIGAR...";
        let w = text_width(msg, 1);
        text(c, msg, 160 - w / 2, 169 + o, RAIN, 1);
    }

    fn draw_cat_frame(&mut self, c: &mut Canvas<'_>, te: u32, t_ms: u32) {
        let (sofa_x, sofa_y) = Room::SOFA_SPOT;
        let gy = BED_Y + 1.0;
        let tf = te as f32;
        let wag = |period: f32| sin(t_ms as f32 / period);
        let cat = &mut self.cat;
        let (x, y, style) = if te < 300 {
            let s = CatStyle {
                eyes: if te < 150 { Eyes::Closed } else { Eyes::Half },
                tail: wag(80.0),
                ..CatStyle::new(Pose::Sit)
            };
            (sofa_x, sofa_y, s)
        } else if te < 550 {
            let u = (tf - 300.0) / 250.0;
            let y = lerp(sofa_y, gy, u) - sin(core::f32::consts::PI * u) * 14.0;
            let s = CatStyle {
                eyes: Eyes::Open,
                ..CatStyle::new(Pose::Ball)
            };
            (lerp(sofa_x, 205.0, u), y, s)
        } else if te < 620 {
            let s = CatStyle {
                scale_x: 1.2,
                scale_y: 0.8,
                ..CatStyle::new(Pose::Sit)
            };
            (205.0, gy, s)
        } else if te < 1000 {
            let u = (tf - 620.0) / 380.0;
            let s = CatStyle {
                step: ((te / 90) % 4) as u8,
                eyes: Eyes::Half,
                tail: wag(120.0),
                ..CatStyle::new(Pose::Walk)
            };
            (lerp(205.0, BED_X + 2.0, u), gy, s)
        } else if te < 1600 {
            // Three turns: side, back, other side, back... with a stumble.
            let i = ((te - 1000) / 100).min(6);
            let s = match i {
                1 | 3 | 6 => CatStyle {
                    tail: wag(80.0),
                    ..CatStyle::new(Pose::Back)
                },
                5 => CatStyle {
                    eyes: Eyes::Wide,
                    mouth: Mouth::Open,
                    ..CatStyle::new(Pose::Sit)
                },
                _ => CatStyle {
                    step: (i % 4) as u8,
                    eyes: Eyes::Open,
                    flip: i != 2,
                    ..CatStyle::new(Pose::Walk)
                },
            };
            (BED_X, if i == 5 { gy - 3.0 } else { gy }, s)
        } else if te < 1750 {
            let s = CatStyle {
                eyes: Eyes::Half,
                scale_y: 0.9,
                ..CatStyle::new(Pose::Sit)
            };
            (BED_X, gy, s)
        } else {
            let (eyes, mouth) = if (1900..2400).contains(&te) {
                let m = if te < 2020 {
                    Mouth::Open
                } else if te < 2270 {
                    Mouth::Yawn
                } else {
                    Mouth::Closed
                };
                (Eyes::Closed, m)
            } else if te >= 2400 {
                (
                    if te < 2500 { Eyes::Half } else { Eyes::Closed },
                    Mouth::Closed,
                )
            } else {
                (Eyes::Half, Mouth::Closed)
            };
            let s = CatStyle {
                eyes,
                mouth,
                breathe: te > SLEEP_START && (t_ms / 400) % 2 == 1,
                ..CatStyle::new(Pose::Curl)
            };
            (BED_X + 4.0, gy, s)
        };
        draw_cat(c, cat, x, y, &style);
    }
}

/// Moonlight from the window falling diagonally onto the floor.
fn moonlight(c: &mut Canvas<'_>) {
    let lit = |c: &mut Canvas<'_>, x: i32, y: i32| {
        let d = (y - 94) as f32 / 86.0;
        if (bayer(x, y) as f32) / 16.0 < 0.42 * (1.0 - d * 0.55) {
            let v = c.get(x, y);
            c.set(
                x,
                y,
                if v <= 1 {
                    SHADOW
                } else if v == SHADOW {
                    WALL
                } else {
                    v
                },
            );
        }
    };
    c.triangle_each([(48.0, 94.0), (134.0, 94.0), (200.0, 180.0)], lit);
    c.triangle_each([(48.0, 94.0), (200.0, 180.0), (100.0, 180.0)], lit);
}

/// Back half of the wicker basket with its blanket.
fn bed_back(c: &mut Canvas<'_>, x: f32, y: f32) {
    c.ellipse(x, y, 29.0, 10.0, OUTLINE);
    c.ellipse(x, y - 1.0, 28.0, 9.0, STRIPES);
    for i in (-24..=24).step_by(4) {
        c.set(
            x as i32 + i,
            (y - 6.0 + (i.abs() as f32) / 6.0) as i32,
            OUTLINE,
        );
    }
    c.ellipse(x, y - 2.0, 22.0, 5.0, BLANKET);
    c.dither_rect(x as i32 - 16, y as i32 - 4, 32, 2, CREAM, true);
}

/// Front rim of the basket, drawn over the cat so he sits inside it.
fn bed_front(c: &mut Canvas<'_>, x: f32, y: f32) {
    let (xi, yi) = (x as i32, y as i32);
    for j in 2..12 {
        for i in -30..=30 {
            let dx = (i as f32 + 0.5) / 29.0;
            let dy = (j as f32 - 0.5) / 10.0;
            if dx * dx + dy * dy > 1.0 {
                continue;
            }
            let inner_dy = (j as f32 - 0.5) / 8.6;
            let inside = dx * dx / (0.88 * 0.88) + inner_dy * inner_dy <= 1.0;
            if inside && j < 6 {
                continue; // the opening of the basket: leave the cat visible
            }
            c.set(
                xi + i,
                yi + j - 1,
                if (i + j) % 4 == 0 { OUTLINE } else { STRIPES },
            );
        }
    }
    c.ellipse(x, y + 8.0, 26.0, 1.6, OUTLINE);
}
