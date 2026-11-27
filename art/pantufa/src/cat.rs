//! Pantufa, the fat ginger cat. Each pose is a handful of ellipses and
//! triangles drawn into a scratch sprite, outlined automatically, then copied
//! onto the scene. Coordinates are relative to the point between the cat's
//! feet (`x` right, `y` down, so the body is at negative `y`).

use crate::canvas::Canvas;
use crate::math::round;
use crate::palette::TRANSPARENT;
use crate::palette::idx::{CREAM, FUR, FUR_MID, HIGHLIGHT, NIGHT, NOSE, OUTLINE, STRIPES};

pub const SPRITE_WIDTH: usize = 96;
pub const SPRITE_HEIGHT: usize = 72;
const ANCHOR_X: f32 = 48.0;
const ANCHOR_Y: f32 = 66.0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Pose {
    /// Facing the viewer, sitting up.
    Sit,
    /// Sitting, seen from behind (turning round in the bed).
    Back,
    /// Asleep, curled into a loaf with the head on the left.
    Curl,
    /// Front paws forward, rear up; [`CatStyle::stretch`] goes from 0 to 1.
    Stretch,
    /// Tucked into a ball, mid-jump.
    Ball,
    /// Side view walking left; [`CatStyle::step`] is the frame (0..4).
    Walk,
    /// Side view, arched and bristling, hissing (security alerts);
    /// [`CatStyle::puff`] inflates him from 0 to 1.
    Puff,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Eyes {
    Closed,
    Open,
    Half,
    Happy,
    Wide,
    /// Left eye closed, right eye open: "is it morning already?".
    OneOpen,
    /// Half closed with the lids slanting down to the nose.
    Angry,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Mouth {
    Closed,
    Open,
    Yawn,
}

/// How to draw the cat this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CatStyle {
    pub pose: Pose,
    pub eyes: Eyes,
    pub mouth: Mouth,
    /// Belly up (breathing in).
    pub breathe: bool,
    /// Tail tip offset, about -1.5..1.5 (feed it a sine for a wag).
    pub tail: f32,
    /// Progress of [`Pose::Stretch`], 0..1.
    pub stretch: f32,
    /// Walk cycle frame for [`Pose::Walk`], taken modulo 4.
    pub step: u8,
    /// Inflation of [`Pose::Puff`], 0..1.
    pub puff: f32,
    /// Squash and stretch: 1.0 is the normal size.
    pub scale_x: f32,
    pub scale_y: f32,
    /// One ear flicked.
    pub ear_twitch: bool,
    /// Ears flat back (annoyed); [`Pose::Sit`] only.
    pub ear_flat: bool,
    /// Mirror horizontally.
    pub flip: bool,
}

impl CatStyle {
    pub const fn new(pose: Pose) -> Self {
        Self {
            pose,
            eyes: Eyes::Closed,
            mouth: Mouth::Closed,
            breathe: false,
            tail: 0.0,
            stretch: 1.0,
            step: 0,
            puff: 0.0,
            scale_x: 1.0,
            scale_y: 1.0,
            ear_twitch: false,
            ear_flat: false,
            flip: false,
        }
    }
}

/// Scratch space for one cat sprite (6 912 bytes). Keep it next to the
/// scene instead of on a small stack.
pub struct CatSprite {
    px: [u8; SPRITE_WIDTH * SPRITE_HEIGHT],
}

impl CatSprite {
    pub const fn new() -> Self {
        Self {
            px: [TRANSPARENT; SPRITE_WIDTH * SPRITE_HEIGHT],
        }
    }
}

impl Default for CatSprite {
    fn default() -> Self {
        Self::new()
    }
}

/// Maps cat coordinates to sprite pixels, with squash and stretch.
struct Pen {
    sx: f32,
    sy: f32,
}

type Pt = (f32, f32);

impl Pen {
    fn at(&self, p: Pt) -> Pt {
        (ANCHOR_X + p.0 * self.sx, ANCHOR_Y + p.1 * self.sy)
    }

    fn ellipse(&self, s: &mut Canvas<'_>, c: Pt, r: Pt, color: u8) {
        self.ellipse_rot(s, c, r, 0.0, color);
    }

    fn ellipse_rot(&self, s: &mut Canvas<'_>, c: Pt, r: Pt, angle: f32, color: u8) {
        let (x, y) = self.at(c);
        s.ellipse_rotated(x, y, r.0 * self.sx, r.1 * self.sy, angle, color);
    }

    fn triangle(&self, s: &mut Canvas<'_>, p: [Pt; 3], color: u8) {
        s.triangle([self.at(p[0]), self.at(p[1]), self.at(p[2])], color);
    }

    fn dot(&self, s: &mut Canvas<'_>, p: Pt, color: u8) {
        let (x, y) = self.at(p);
        s.set_at(x, y, color);
    }

    fn line(&self, s: &mut Canvas<'_>, a: Pt, b: Pt, color: u8) {
        s.line(self.at(a), self.at(b), color);
    }

    /// A tail is a chain of round segments with alternating stripe colours.
    fn tail(&self, s: &mut Canvas<'_>, points: &[Pt], r: f32, colors: &[u8]) {
        for (i, &p) in points.iter().enumerate() {
            self.ellipse(s, p, (r, r), colors[i % colors.len()]);
        }
    }
}

/// Where the face features of a pose are, filled by the pose drawing.
struct Face {
    /// Eye centres and side (-1 left, 1 right); side views have one eye.
    eyes: [Option<(Pt, i8)>; 2],
    nose: [Pt; 2],
    mouth: Option<Pt>,
    whiskers: Whiskers,
    /// Bristling fur all around after the outline ([`Pose::Puff`]).
    fuzz: bool,
}

impl Face {
    const NONE: Self = Self {
        eyes: [None, None],
        nose: [(0.0, 0.0); 2],
        mouth: None,
        whiskers: Whiskers::None,
        fuzz: false,
    };
}

#[derive(Clone, Copy, PartialEq)]
enum Whiskers {
    None,
    Front,
    Curl,
    /// Side view; the value is the head's vertical bob.
    Walk(f32),
}

/// Draws Pantufa with its feet at `(x, y)` on `target`. `sprite` is scratch.
pub fn draw_cat(target: &mut Canvas<'_>, sprite: &mut CatSprite, x: f32, y: f32, style: &CatStyle) {
    let Some(mut s) = Canvas::new(SPRITE_WIDTH, SPRITE_HEIGHT, &mut sprite.px) else {
        return;
    };
    s.clear(TRANSPARENT);
    // Squash and stretch stays a small effect; garbage values are ignored.
    let sane = |v: f32, lo: f32, hi: f32, default: f32| {
        if v.is_finite() {
            v.clamp(lo, hi)
        } else {
            default
        }
    };
    let pen = Pen {
        sx: sane(style.scale_x, 0.25, 4.0, 1.0),
        sy: sane(style.scale_y, 0.25, 4.0, 1.0),
    };
    let st = CatStyle {
        tail: sane(style.tail, -4.0, 4.0, 0.0),
        stretch: sane(style.stretch, 0.0, 1.0, 1.0),
        puff: sane(style.puff, 0.0, 1.0, 0.0),
        ..*style
    };
    let face = match st.pose {
        Pose::Sit => sit(&mut s, &pen, &st, false),
        Pose::Back => sit(&mut s, &pen, &st, true),
        Pose::Curl => curl(&mut s, &pen, &st),
        Pose::Stretch => stretch(&mut s, &pen, &st),
        Pose::Ball => ball(&mut s, &pen, &st),
        Pose::Walk => walk(&mut s, &pen, &st),
        Pose::Puff => puff(&mut s, &pen, &st),
    };
    let side_view = face.eyes[1].is_none();
    for &(p, side) in face.eyes.iter().flatten() {
        let kind = match st.eyes {
            Eyes::OneOpen if side_view => Eyes::Open,
            e => e,
        };
        let (ex, ey) = pen.at(p);
        eye(&mut s, round(ex), round(ey), kind, side);
    }
    if face.eyes[0].is_some() {
        for &p in &face.nose {
            pen.dot(&mut s, p, NOSE);
        }
    }
    if let Some((mx, my)) = face.mouth {
        match st.mouth {
            Mouth::Closed => {}
            Mouth::Open => {
                pen.ellipse(&mut s, (mx, my), (1.6, 1.5), OUTLINE);
                pen.ellipse(&mut s, (mx, my + 0.1), (0.9, 0.8), NOSE);
            }
            Mouth::Yawn => {
                pen.ellipse(&mut s, (mx, my), (2.3, 2.6), OUTLINE);
                pen.ellipse(&mut s, (mx, my + 0.2), (1.4, 1.7), NOSE);
            }
        }
    }
    s.outline(OUTLINE);
    if face.fuzz {
        s.fuzz(OUTLINE, round(ANCHOR_Y) - 3);
    }
    // Whiskers stick out past the contour, so they go on after it.
    match face.whiskers {
        Whiskers::Front => {
            pen.line(&mut s, (-5.0, -17.0), (-11.0, -18.0), CREAM);
            pen.line(&mut s, (-5.0, -16.0), (-11.0, -15.5), CREAM);
            pen.line(&mut s, (4.0, -17.0), (10.0, -18.0), CREAM);
            pen.line(&mut s, (4.0, -16.0), (10.0, -15.5), CREAM);
        }
        Whiskers::Curl => {
            pen.line(&mut s, (-15.0, -7.0), (-20.0, -8.0), CREAM);
            pen.line(&mut s, (-15.0, -6.0), (-20.0, -5.5), CREAM);
        }
        Whiskers::Walk(bob) => {
            let hy = -15.0 + bob;
            pen.line(&mut s, (-15.0, hy + 2.0), (-20.0, hy + 1.0), CREAM);
            pen.line(&mut s, (-15.0, hy + 3.0), (-20.0, hy + 3.5), CREAM);
        }
        Whiskers::None => {}
    }
    target.blit(&s, round(x - ANCHOR_X), round(y - ANCHOR_Y), st.flip);
}

/// Sitting up, from the front or (`back`) from behind.
fn sit(s: &mut Canvas<'_>, pen: &Pen, st: &CatStyle, back: bool) -> Face {
    let (tp, br) = (st.tail, if st.breathe { 0.4 } else { 0.0 });
    pen.tail(
        s,
        &[
            (8.0, -3.0),
            (11.0, -3.5),
            (14.0, -5.0),
            (15.5, -8.0),
            (16.0 + tp * 0.6, -11.0),
            (15.5 + tp * 1.2, -14.0),
            (14.5 + tp * 2.0, -16.5),
        ],
        2.1,
        &[FUR, FUR, STRIPES, FUR, STRIPES, FUR, FUR_MID],
    );
    pen.ellipse(s, (0.0, -8.5), (11.0, 8.5 + br), FUR_MID);
    pen.ellipse(s, (-0.5, -9.5), (10.0, 7.5 + br), FUR);
    if back {
        pen.ellipse_rot(s, (-6.0, -11.0), (1.2, 2.5), 0.4, STRIPES);
        pen.ellipse_rot(s, (5.0, -11.0), (1.2, 2.5), -0.4, STRIPES);
        pen.ellipse(s, (-0.5, -13.0), (1.0, 2.5), STRIPES);
    } else {
        pen.ellipse(s, (0.0, -9.0), (5.2, 6.0), CREAM);
        pen.ellipse_rot(s, (-8.0, -10.0), (1.0, 2.2), 0.3, STRIPES);
        pen.ellipse_rot(s, (7.0, -10.0), (1.0, 2.2), -0.3, STRIPES);
    }
    let paws = if back { FUR } else { CREAM };
    pen.ellipse(s, (-4.5, -1.2), (3.0, 1.7), paws);
    pen.ellipse(s, (4.5, -1.2), (3.0, 1.7), paws);
    if st.ear_flat {
        pen.triangle(s, [(-9.0, -21.0), (-13.0, -26.0), (-5.0, -25.0)], FUR);
        pen.triangle(s, [(9.0, -21.0), (13.0, -26.0), (5.0, -25.0)], FUR);
    } else {
        let twitch = if st.ear_twitch {
            (-1.0, 2.0)
        } else {
            (0.0, 0.0)
        };
        pen.triangle(
            s,
            [
                (-8.5, -23.0),
                (-7.5 + twitch.0, -31.0 + twitch.1),
                (-2.5, -26.0),
            ],
            FUR,
        );
        pen.triangle(s, [(8.5, -23.0), (7.5, -31.0), (2.5, -26.0)], FUR);
        if !back {
            pen.triangle(s, [(-7.0, -24.5), (-7.0, -29.0), (-4.0, -26.0)], NOSE);
            pen.triangle(s, [(7.0, -24.5), (7.0, -29.0), (4.0, -26.0)], NOSE);
        }
    }
    pen.ellipse(s, (0.0, -20.0), (9.5, 7.2), FUR_MID);
    pen.ellipse(s, (0.0, -20.6), (9.0, 6.6), FUR);
    for x in [-3.0, 2.0] {
        pen.line(s, (x, -26.0), (x, -24.0), STRIPES);
    }
    for x in [-1.0, 0.0] {
        pen.line(s, (x, -27.0), (x, -25.0), STRIPES);
    }
    if back {
        pen.line(s, (-2.0, -19.0), (-2.0, -16.0), STRIPES);
        pen.line(s, (1.0, -19.0), (1.0, -16.0), STRIPES);
        return Face::NONE;
    }
    pen.ellipse(s, (0.0, -16.8), (4.4, 2.4), CREAM);
    Face {
        eyes: [Some(((-4.0, -20.0), -1)), Some(((4.0, -20.0), 1))],
        nose: [(-1.0, -18.4), (0.0, -18.4)],
        mouth: Some((0.0, -15.4)),
        whiskers: Whiskers::Front,
        fuzz: false,
    }
}

fn curl(s: &mut Canvas<'_>, pen: &Pen, st: &CatStyle) -> Face {
    let (tp, br) = (st.tail, if st.breathe { 0.5 } else { 0.0 });
    pen.ellipse(s, (2.0, -8.0), (15.5, 8.2 + br), FUR_MID);
    pen.ellipse(s, (1.5, -9.0), (14.5, 7.0 + br), FUR);
    pen.ellipse_rot(s, (-1.0, -15.0), (1.0, 2.4), 0.35, STRIPES);
    pen.ellipse(s, (5.0, -15.6), (1.0, 2.4), STRIPES);
    pen.ellipse_rot(s, (11.0, -14.0), (1.0, 2.4), -0.45, STRIPES);
    pen.tail(
        s,
        &[
            (16.0, -4.0),
            (13.0, -2.6),
            (9.0, -1.9),
            (5.0, -1.6),
            (1.0, -1.6),
            (-3.0, -1.9 - tp * 0.6),
            (-6.0, -2.4 - tp * 1.4),
        ],
        2.2,
        &[FUR, STRIPES, FUR, STRIPES, FUR, STRIPES, FUR_MID],
    );
    pen.ellipse(s, (-4.0, -3.4), (2.8, 1.6), CREAM);
    let twitch = if st.ear_twitch { 1.5 } else { 0.0 };
    pen.triangle(
        s,
        [(-16.0, -11.0), (-17.5, -18.0 + twitch), (-12.0, -14.0)],
        FUR,
    );
    pen.triangle(s, [(-9.5, -14.0), (-6.0, -19.0), (-5.0, -12.5)], FUR);
    pen.triangle(s, [(-15.5, -12.5), (-16.4, -16.0), (-13.3, -14.0)], NOSE);
    pen.triangle(s, [(-8.8, -14.0), (-6.4, -17.4), (-6.0, -13.6)], NOSE);
    pen.ellipse(s, (-10.5, -9.5), (7.2, 6.0), FUR_MID);
    pen.ellipse(s, (-10.5, -10.0), (6.8, 5.4), FUR);
    pen.ellipse(s, (-11.5, -6.8), (3.8, 2.1), CREAM);
    Face {
        eyes: [Some(((-13.5, -10.0), -1)), Some(((-7.5, -10.0), 1))],
        nose: [(-11.5, -8.0), (-10.5, -8.0)],
        mouth: Some((-11.2, -5.4)),
        whiskers: Whiskers::Curl,
        fuzz: false,
    }
}

fn stretch(s: &mut Canvas<'_>, pen: &Pen, st: &CatStyle) -> Face {
    let (tp, k) = (st.tail, st.stretch);
    pen.tail(
        s,
        &[
            (12.0, -12.0 - 2.0 * k),
            (14.0, -15.0 - 3.0 * k),
            (15.0, -18.0 - 4.0 * k),
            (14.5 + tp, -21.0 - 5.0 * k),
            (13.0 + tp * 1.5, -23.0 - 6.0 * k),
        ],
        2.0,
        &[FUR, STRIPES, FUR, STRIPES, FUR_MID],
    );
    pen.ellipse(s, (12.0, -3.5), (2.6, 4.0), FUR_MID);
    pen.ellipse(s, (8.5, -3.5), (2.4, 4.0), FUR);
    let tilt = -0.25 - 0.15 * k;
    pen.ellipse_rot(s, (2.0, -9.0 - 2.0 * k), (13.5, 6.0), tilt, FUR_MID);
    pen.ellipse_rot(s, (2.0, -10.0 - 2.0 * k), (12.5, 5.0), tilt, FUR);
    pen.ellipse_rot(s, (-1.0, -12.0 - 2.0 * k), (1.0, 2.2), 0.4, STRIPES);
    pen.ellipse_rot(s, (5.0, -14.0 - 2.0 * k), (1.0, 2.2), 0.2, STRIPES);
    pen.ellipse(s, (-12.0 - 4.0 * k, -1.6), (5.0 + 2.0 * k, 1.6), FUR);
    pen.ellipse(s, (-17.0 - 6.0 * k, -1.4), (2.4, 1.4), CREAM);
    let (hx, hy) = (-9.0 - 2.0 * k, -6.5);
    pen.triangle(
        s,
        [
            (hx - 5.5, hy - 3.5),
            (hx - 6.5, hy - 10.0),
            (hx - 1.5, hy - 5.5),
        ],
        FUR,
    );
    pen.triangle(
        s,
        [
            (hx + 1.5, hy - 5.5),
            (hx + 5.0, hy - 9.5),
            (hx + 5.5, hy - 3.0),
        ],
        FUR,
    );
    pen.ellipse(s, (hx, hy), (7.0, 5.5), FUR_MID);
    pen.ellipse(s, (hx, hy - 0.5), (6.6, 5.0), FUR);
    pen.ellipse(s, (hx - 1.0, hy + 2.6), (3.6, 2.0), CREAM);
    let mouth = (hx - 1.0, hy + 3.6);
    Face {
        eyes: [
            Some(((hx - 3.5, hy - 0.5), -1)),
            Some(((hx + 2.5, hy - 0.5), 1)),
        ],
        nose: [
            (mouth.0 - 0.5, mouth.1 - 1.8),
            (mouth.0 + 0.5, mouth.1 - 1.8),
        ],
        mouth: Some(mouth),
        whiskers: Whiskers::None,
        fuzz: false,
    }
}

fn ball(s: &mut Canvas<'_>, pen: &Pen, _st: &CatStyle) -> Face {
    pen.tail(
        s,
        &[
            (8.0, -9.0),
            (12.0, -8.0),
            (15.0, -9.5),
            (17.0, -12.0),
            (18.0, -15.0),
        ],
        2.0,
        &[FUR, STRIPES, FUR, STRIPES, FUR_MID],
    );
    pen.ellipse(s, (1.0, -10.0), (10.5, 9.5), FUR_MID);
    pen.ellipse(s, (0.5, -11.0), (9.8, 8.6), FUR);
    pen.ellipse(s, (-1.0, -6.5), (5.0, 3.6), CREAM);
    pen.ellipse(s, (-5.0, -2.5), (2.6, 1.6), CREAM);
    pen.ellipse(s, (2.0, -2.2), (2.6, 1.6), CREAM);
    let (hx, hy) = (-4.0, -16.0);
    pen.triangle(
        s,
        [
            (hx - 6.5, hy - 3.0),
            (hx - 6.0, hy - 10.5),
            (hx - 1.5, hy - 5.5),
        ],
        FUR,
    );
    pen.triangle(
        s,
        [
            (hx + 1.5, hy - 5.5),
            (hx + 5.5, hy - 10.0),
            (hx + 6.5, hy - 3.0),
        ],
        FUR,
    );
    pen.triangle(
        s,
        [
            (hx - 5.4, hy - 4.4),
            (hx - 5.2, hy - 8.4),
            (hx - 2.8, hy - 5.6),
        ],
        NOSE,
    );
    pen.ellipse(s, (hx, hy), (7.5, 6.0), FUR_MID);
    pen.ellipse(s, (hx, hy - 0.5), (7.0, 5.5), FUR);
    pen.ellipse(s, (hx - 0.5, hy + 3.0), (3.8, 2.2), CREAM);
    let mouth = (hx - 0.5, hy + 4.4);
    Face {
        eyes: [
            Some(((hx - 3.5, hy - 0.5), -1)),
            Some(((hx + 3.0, hy - 0.5), 1)),
        ],
        nose: [
            (mouth.0 - 0.5, mouth.1 - 1.8),
            (mouth.0 + 0.5, mouth.1 - 1.8),
        ],
        mouth: Some(mouth),
        whiskers: Whiskers::None,
        fuzz: false,
    }
}

/// Side view walking left (flip for right), with a fat belly that bobs.
fn walk(s: &mut Canvas<'_>, pen: &Pen, st: &CatStyle) -> Face {
    let f = st.step % 4;
    let bob = if f % 2 == 1 { -1.0 } else { 0.0 };
    let a = [1.5, 0.0, -1.5, 0.0][f as usize];
    let tp = st.tail;
    pen.tail(
        s,
        &[
            (11.0, -11.0 + bob),
            (13.0, -15.0 + bob),
            (14.0, -19.0 + bob),
            (13.0 + tp, -23.0 + bob),
            (11.0 + tp * 1.6, -25.0 + bob),
        ],
        2.0,
        &[FUR, STRIPES, FUR, STRIPES, FUR_MID],
    );
    // Far legs darker, near legs lighter, alternating with the step.
    pen.ellipse(s, (9.0 + a, -3.5), (1.8, 3.5), FUR_MID);
    pen.ellipse(s, (-6.0 - a, -3.5), (1.8, 3.5), FUR_MID);
    pen.ellipse(s, (7.0 - a, -3.5), (1.8, 3.5), FUR);
    pen.ellipse(s, (-8.0 + a, -3.5), (1.8, 3.5), FUR);
    pen.ellipse(s, (7.0 - a, -0.8), (1.9, 1.0), CREAM);
    pen.ellipse(s, (-8.0 + a, -0.8), (1.9, 1.0), CREAM);
    let by = -9.0 + bob;
    pen.ellipse(s, (1.0, by), (12.5, 6.5), FUR_MID);
    pen.ellipse(s, (1.0, by - 1.0), (11.5, 5.5), FUR);
    pen.ellipse(s, (0.0, -5.5 + bob), (8.5, 3.0), FUR);
    pen.ellipse(s, (-1.0, -4.8 + bob), (6.0, 1.8), CREAM);
    pen.ellipse(s, (2.0, by - 5.5), (1.0, 2.0), STRIPES);
    pen.ellipse_rot(s, (6.0, by - 5.0), (1.0, 2.0), -0.3, STRIPES);
    pen.ellipse_rot(s, (-2.0, by - 5.0), (1.0, 2.0), 0.3, STRIPES);
    let (hx, hy) = (-11.0, -15.0 + bob);
    pen.triangle(
        s,
        [
            (hx - 4.0, hy - 3.0),
            (hx - 4.5, hy - 9.0),
            (hx - 0.5, hy - 5.0),
        ],
        FUR,
    );
    pen.triangle(
        s,
        [
            (hx + 1.0, hy - 5.0),
            (hx + 3.5, hy - 9.0),
            (hx + 4.5, hy - 3.0),
        ],
        FUR,
    );
    pen.triangle(
        s,
        [
            (hx - 3.4, hy - 4.0),
            (hx - 3.8, hy - 7.4),
            (hx - 1.4, hy - 5.0),
        ],
        NOSE,
    );
    pen.ellipse(s, (hx, hy), (6.5, 5.5), FUR_MID);
    pen.ellipse(s, (hx, hy - 0.5), (6.1, 5.0), FUR);
    pen.ellipse(s, (hx - 3.5, hy + 2.0), (3.0, 2.0), CREAM);
    Face {
        eyes: [Some(((hx - 2.5, hy - 0.5), -1)), None],
        nose: [(hx - 6.6, hy + 1.0), (hx - 6.6, hy + 0.2)],
        mouth: None,
        whiskers: Whiskers::Walk(bob),
        fuzz: false,
    }
}

/// Arched, ears back, fur on end, hissing with a slit-pupil eye.
fn puff(s: &mut Canvas<'_>, pen: &Pen, st: &CatStyle) -> Face {
    let inf = st.puff;
    pen.tail(
        s,
        &[
            (11.0, -17.0),
            (12.5, -21.0),
            (13.0, -25.0),
            (13.0, -29.0),
            (12.5, -33.0),
        ],
        3.2 + inf * 0.8,
        &[FUR, STRIPES, FUR, STRIPES, FUR_MID],
    );
    for (x, c) in [(9.0, FUR_MID), (-6.0, FUR_MID), (7.0, FUR), (-8.0, FUR)] {
        pen.ellipse(s, (x, -6.0), (1.9, 6.0), c);
    }
    pen.ellipse(s, (7.0, -0.8), (2.0, 1.0), CREAM);
    pen.ellipse(s, (-8.0, -0.8), (2.0, 1.0), CREAM);
    pen.ellipse(s, (1.0, -15.0), (12.5 + inf, 7.0 + inf), FUR_MID);
    pen.ellipse(s, (1.0, -16.0), (11.5 + inf, 6.0 + inf), FUR);
    pen.ellipse(s, (1.0, -20.0 - inf), (8.0 + inf, 4.5), FUR);
    pen.ellipse(s, (-1.0, -24.0 - inf), (1.0, 2.0), STRIPES);
    pen.ellipse(s, (3.0, -24.5 - inf), (1.0, 2.0), STRIPES);
    pen.ellipse_rot(s, (7.0, -22.5 - inf), (1.0, 2.0), -0.4, STRIPES);
    pen.ellipse_rot(s, (-5.0, -22.0 - inf), (1.0, 2.0), 0.4, STRIPES);
    let (hx, hy) = (-12.0, -13.0);
    pen.triangle(
        s,
        [
            (hx + 1.0, hy - 4.0),
            (hx + 8.0, hy - 7.0),
            (hx + 3.0, hy - 1.5),
        ],
        FUR,
    );
    pen.triangle(
        s,
        [
            (hx - 1.0, hy - 5.0),
            (hx + 4.0, hy - 9.0),
            (hx + 2.0, hy - 3.0),
        ],
        FUR,
    );
    pen.ellipse(s, (hx, hy), (6.8, 5.8), FUR_MID);
    pen.ellipse(s, (hx, hy - 0.5), (6.4, 5.3), FUR);
    pen.ellipse(s, (hx - 3.5, hy + 2.2), (3.2, 2.2), CREAM);
    // Yellow eye with a slit pupil and an angry brow.
    pen.ellipse(s, (hx - 2.6, hy - 1.0), (2.2, 2.0), OUTLINE);
    pen.ellipse(s, (hx - 2.6, hy - 1.0), (1.6, 1.4), HIGHLIGHT);
    pen.line(s, (hx - 2.6, hy - 2.4), (hx - 2.6, hy + 0.2), NIGHT);
    pen.line(s, (hx - 5.0, hy - 3.4), (hx - 1.0, hy - 2.6), OUTLINE);
    // Open mouth with fangs.
    pen.ellipse(s, (hx - 4.2, hy + 3.4), (2.8, 2.4), OUTLINE);
    pen.ellipse(s, (hx - 4.2, hy + 3.6), (2.0, 1.5), NOSE);
    for p in [
        (hx - 5.6, hy + 2.2),
        (hx - 2.8, hy + 2.2),
        (hx - 5.6, hy + 4.6),
    ] {
        pen.dot(s, p, CREAM);
    }
    pen.dot(s, (hx - 7.0, hy + 0.8), NOSE);
    Face {
        fuzz: true,
        ..Face::NONE
    }
}

/// A 4-pixel-wide eye centred between pixels `x - 1` and `x`.
fn eye(s: &mut Canvas<'_>, x: i32, y: i32, eyes: Eyes, side: i8) {
    let kind = match eyes {
        Eyes::OneOpen if side < 0 => Eyes::Closed,
        Eyes::OneOpen => Eyes::Open,
        e => e,
    };
    match kind {
        Eyes::Closed => {
            for (dx, dy) in [(-2, -1), (-1, 0), (0, 0), (1, -1)] {
                s.set(x + dx, y + dy, OUTLINE);
            }
        }
        Eyes::Happy => {
            for (dx, dy) in [(-2, 0), (-1, -1), (0, -1), (1, 0)] {
                s.set(x + dx, y + dy, OUTLINE);
            }
        }
        Eyes::Half | Eyes::Angry => {
            s.rect(x - 1, y, 2, 1, NIGHT);
            s.rect(x - 2, y - 1, 4, 1, OUTLINE);
            if kind == Eyes::Angry {
                // The lid drops on the side next to the nose.
                s.set(if side < 0 { x + 1 } else { x - 2 }, y, OUTLINE);
            }
        }
        Eyes::Wide => {
            s.rect(x - 2, y - 2, 4, 4, CREAM);
            s.rect(x - 1, y - 1, 2, 2, NIGHT);
        }
        Eyes::Open | Eyes::OneOpen => {
            s.rect(x - 1, y - 1, 2, 2, NIGHT);
            s.set(x - 1, y - 1, CREAM);
        }
    }
}
