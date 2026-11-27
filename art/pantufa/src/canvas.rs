//! A palette-indexed canvas over a borrowed buffer, and the raster shapes the
//! art is built from: ellipses, triangles, lines, an automatic outline and
//! sprite blits. Every write is clipped to the canvas.

use crate::math::{ceil, floor, round, sin_cos};
use crate::palette::{COLORS, TRANSPARENT};

/// Temporary value used while computing an outline; never left behind.
const MARK: u8 = 0xFE;

/// Coordinates beyond this are rejected by `line`, so a bad input cannot make
/// it walk billions of off-screen pixels.
const MAX_COORD: i32 = 1 << 14;

/// 4×4 ordered-dither matrix (values 0..16) for gradients with flat colours.
pub const BAYER: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];

/// Dither threshold for a pixel: compare `bayer(x, y) / 16` with the
/// desired coverage.
pub const fn bayer(x: i32, y: i32) -> u8 {
    BAYER[(y & 3) as usize][(x & 3) as usize]
}

/// `width × height` palette indices, row-major, borrowed from the caller.
/// [`TRANSPARENT`] marks empty pixels in sprites.
pub struct Canvas<'a> {
    width: usize,
    height: usize,
    px: &'a mut [u8],
}

impl<'a> Canvas<'a> {
    /// Uses the first `width * height` bytes of `px`. Returns `None` for
    /// empty or oversized dimensions, or when `px` is too short.
    pub fn new(width: usize, height: usize, px: &'a mut [u8]) -> Option<Self> {
        if width == 0 || height == 0 || width > MAX_COORD as usize || height > MAX_COORD as usize {
            return None;
        }
        let len = width.checked_mul(height)?;
        let px = px.get_mut(..len)?;
        Some(Self { width, height, px })
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    /// The indices, row-major.
    pub fn pixels(&self) -> &[u8] {
        self.px
    }

    pub fn clear(&mut self, color: u8) {
        self.px.fill(color);
    }

    fn index(&self, x: i32, y: i32) -> Option<usize> {
        let x = usize::try_from(x).ok()?;
        let y = usize::try_from(y).ok()?;
        (x < self.width && y < self.height).then(|| y * self.width + x)
    }

    /// The index at `(x, y)`, or [`TRANSPARENT`] outside the canvas.
    pub fn get(&self, x: i32, y: i32) -> u8 {
        self.index(x, y).map_or(TRANSPARENT, |i| self.px[i])
    }

    pub fn set(&mut self, x: i32, y: i32, color: u8) {
        if let Some(i) = self.index(x, y) {
            self.px[i] = color;
        }
    }

    /// Sets the pixel that contains the point `(x, y)`.
    pub fn set_at(&mut self, x: f32, y: f32, color: u8) {
        self.set(floor(x), floor(y), color);
    }

    pub fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: u8) {
        let x0 = x.max(0);
        let y0 = y.max(0);
        let x1 = x.saturating_add(w).min(self.width as i32);
        let y1 = y.saturating_add(h).min(self.height as i32);
        for yy in y0..y1 {
            for xx in x0..x1 {
                self.set(xx, yy, color);
            }
        }
    }

    /// Clamped inclusive pixel range `[lo, hi]` on one axis.
    fn span(lo: i32, hi: i32, len: usize) -> (i32, i32) {
        (lo.max(0), hi.min(len as i32 - 1))
    }

    /// Fills the pixels whose centres fall inside the axis-aligned ellipse.
    pub fn ellipse(&mut self, cx: f32, cy: f32, rx: f32, ry: f32, color: u8) {
        self.ellipse_rotated(cx, cy, rx, ry, 0.0, color);
    }

    /// Fills an ellipse rotated by `angle` radians around its centre.
    pub fn ellipse_rotated(&mut self, cx: f32, cy: f32, rx: f32, ry: f32, angle: f32, color: u8) {
        if !(rx > 0.0 && ry > 0.0) {
            return;
        }
        let (sa, ca) = sin_cos(angle);
        let r = ceil(rx.max(ry)).saturating_add(1);
        let (x0, x1) = Self::span(
            floor(cx).saturating_sub(r),
            floor(cx).saturating_add(r),
            self.width,
        );
        let (y0, y1) = Self::span(
            floor(cy).saturating_sub(r),
            floor(cy).saturating_add(r),
            self.height,
        );
        for y in y0..=y1 {
            for x in x0..=x1 {
                let dx = x as f32 + 0.5 - cx;
                let dy = y as f32 + 0.5 - cy;
                let u = (dx * ca + dy * sa) / rx;
                let v = (-dx * sa + dy * ca) / ry;
                if u * u + v * v <= 1.0 {
                    self.set(x, y, color);
                }
            }
        }
    }

    /// Fills the band of an ellipse between `1 - thickness` and `1` of its
    /// radius (meridians, rings).
    pub fn ring(&mut self, cx: f32, cy: f32, rx: f32, ry: f32, thickness: f32, color: u8) {
        if !(rx > 0.0 && ry > 0.0) {
            return;
        }
        let inner = (1.0 - thickness) * (1.0 - thickness);
        let r = ceil(rx.max(ry)).saturating_add(1);
        let (x0, x1) = Self::span(
            floor(cx).saturating_sub(r),
            floor(cx).saturating_add(r),
            self.width,
        );
        let (y0, y1) = Self::span(
            floor(cy).saturating_sub(r),
            floor(cy).saturating_add(r),
            self.height,
        );
        for y in y0..=y1 {
            for x in x0..=x1 {
                let dx = (x as f32 + 0.5 - cx) / rx;
                let dy = (y as f32 + 0.5 - cy) / ry;
                let q = dx * dx + dy * dy;
                if q <= 1.0 && q >= inner {
                    self.set(x, y, color);
                }
            }
        }
    }

    /// Calls `f` for every in-canvas pixel whose centre is inside the
    /// triangle (either winding).
    pub fn triangle_each(&mut self, p: [(f32, f32); 3], mut f: impl FnMut(&mut Self, i32, i32)) {
        let [a, b, c] = p;
        let min_x = floor(a.0.min(b.0).min(c.0));
        let max_x = ceil(a.0.max(b.0).max(c.0));
        let min_y = floor(a.1.min(b.1).min(c.1));
        let max_y = ceil(a.1.max(b.1).max(c.1));
        let (x0, x1) = Self::span(min_x, max_x, self.width);
        let (y0, y1) = Self::span(min_y, max_y, self.height);
        let edge = |p1: (f32, f32), p2: (f32, f32), x: f32, y: f32| {
            (p2.0 - p1.0) * (y - p1.1) - (p2.1 - p1.1) * (x - p1.0)
        };
        for y in y0..=y1 {
            for x in x0..=x1 {
                let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                let w1 = edge(a, b, px, py);
                let w2 = edge(b, c, px, py);
                let w3 = edge(c, a, px, py);
                if (w1 >= 0.0 && w2 >= 0.0 && w3 >= 0.0) || (w1 <= 0.0 && w2 <= 0.0 && w3 <= 0.0) {
                    f(self, x, y);
                }
            }
        }
    }

    pub fn triangle(&mut self, p: [(f32, f32); 3], color: u8) {
        self.triangle_each(p, |c, x, y| c.set(x, y, color));
    }

    /// Bresenham line between pixel centres (endpoints rounded half up).
    pub fn line(&mut self, from: (f32, f32), to: (f32, f32), color: u8) {
        let (mut x0, mut y0) = (round(from.0), round(from.1));
        let (x1, y1) = (round(to.0), round(to.1));
        // `round` never returns i32::MIN (math.rs clamps), so `unsigned_abs`
        // is exact and the differences below cannot overflow.
        if [x0, y0, x1, y1]
            .iter()
            .any(|v| v.unsigned_abs() > MAX_COORD as u32)
        {
            return;
        }
        let dx = (x1 - x0).abs();
        let dy = -(y1 - y0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;
        loop {
            self.set(x0, y0, color);
            if x0 == x1 && y0 == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x0 += sx;
            }
            if e2 <= dx {
                err += dx;
                y0 += sy;
            }
        }
    }

    /// Paints `color` on every transparent pixel that touches (4-neighbour)
    /// an opaque one. This is the 1-pixel contour every sprite gets.
    pub fn outline(&mut self, color: u8) {
        let (w, h) = (self.width, self.height);
        for y in 0..h {
            for x in 0..w {
                let i = y * w + x;
                if self.px[i] != TRANSPARENT {
                    continue;
                }
                let touches = {
                    let s: &[u8] = self.px;
                    let solid = |j: usize| s[j] != TRANSPARENT && s[j] != MARK;
                    (x > 0 && solid(i - 1))
                        || (x + 1 < w && solid(i + 1))
                        || (y > 0 && solid(i - w))
                        || (y + 1 < h && solid(i + w))
                };
                if touches {
                    self.px[i] = MARK;
                }
            }
        }
        for p in self.px.iter_mut() {
            if *p == MARK {
                *p = color;
            }
        }
    }

    /// Copies the opaque pixels of `src` with its top-left at `(dx, dy)`,
    /// mirrored horizontally when `flip` is set.
    pub fn blit(&mut self, src: &Canvas<'_>, dx: i32, dy: i32, flip: bool) {
        self.blit_with(src, dx, dy, flip, None);
    }

    /// Paints every opaque pixel of `src` with one colour (shadows).
    pub fn blit_tinted(&mut self, src: &Canvas<'_>, dx: i32, dy: i32, color: u8) {
        self.blit_with(src, dx, dy, false, Some(color));
    }

    fn blit_with(&mut self, src: &Canvas<'_>, dx: i32, dy: i32, flip: bool, tint: Option<u8>) {
        for (y, row) in src.px.chunks_exact(src.width).enumerate() {
            for (x, &c) in row.iter().enumerate() {
                if c == TRANSPARENT {
                    continue;
                }
                let tx = if flip { src.width - 1 - x } else { x };
                self.set(
                    dx.saturating_add(tx as i32),
                    dy.saturating_add(y as i32),
                    tint.unwrap_or(c),
                );
            }
        }
    }

    /// Ordered 50 % dither: every other pixel in a checkerboard (`sparse`:
    /// one pixel in four, on even rows and columns).
    pub fn dither_rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: u8, sparse: bool) {
        let (x0, x1) = Self::span(x, x.saturating_add(w).saturating_sub(1), self.width);
        let (y0, y1) = Self::span(y, y.saturating_add(h).saturating_sub(1), self.height);
        for j in y0..=y1 {
            for i in x0..=x1 {
                let on = if sparse {
                    i % 2 == 0 && j % 2 == 0
                } else {
                    (i + j) % 2 == 0
                };
                if on {
                    self.set(i, j, color);
                }
            }
        }
    }

    /// Short spikes on top of each column of the sprite (bristling fur
    /// seen from the front): 1 pixel every 3 columns, 2 every 6.
    pub fn spikes(&mut self, color: u8) {
        for x in 0..self.width {
            if let Some(y) = (0..self.height).find(|&y| self.px[y * self.width + x] != TRANSPARENT)
            {
                let (x, y) = (x as i32, y as i32);
                if x % 3 == 0 {
                    self.set(x, y - 1, color);
                }
                if x % 6 == 0 {
                    self.set(x, y - 2, color);
                }
            }
        }
    }

    /// Bristling fur all around a sprite: some pixels of the `color`
    /// contour grow outwards by 1–2 pixels. Rows from `floor_y` down are
    /// left alone so the paws stay on the ground.
    pub fn fuzz(&mut self, color: u8, floor_y: i32) {
        let (w, h) = (self.width as i32, self.height as i32);
        // Decide on the original contour, then grow: no cascading.
        for y in 0..h.min(floor_y) {
            for x in 0..w {
                if self.get(x, y) != color || (x * 7 + y * 13) % 3 != 0 {
                    continue;
                }
                for (dx, dy) in [(0, -1), (-1, 0), (1, 0), (-1, -1), (1, -1)] {
                    let out = self.get(x + dx, y + dy);
                    let inside = self.get(x - dx, y - dy);
                    if out == TRANSPARENT && inside != TRANSPARENT && inside != MARK {
                        self.set(x + dx, y + dy, MARK);
                        if (x + y) % 2 == 0 && self.get(x + 2 * dx, y + 2 * dy) == TRANSPARENT {
                            self.set(x + 2 * dx, y + 2 * dy, MARK);
                        }
                        break;
                    }
                }
            }
        }
        for p in self.px.iter_mut() {
            if *p == MARK {
                *p = color;
            }
        }
    }

    /// Replaces each palette index with `table[index]`; transparent pixels
    /// stay transparent. With [`crate::palette::DARKER`] this dims a scene.
    pub fn remap(&mut self, table: &[u8; COLORS]) {
        for p in self.px.iter_mut() {
            if let Some(&n) = table.get(*p as usize) {
                *p = n;
            }
        }
    }

    /// Draws a small sprite written as text: one hex digit (`0`–`F`) per
    /// palette index, `.` or space for transparent. `recolor` swaps one
    /// index for another (e.g. a paw print drawn in fur colour).
    pub fn grid(&mut self, rows: &[&str], x: i32, y: i32, recolor: Option<(u8, u8)>) {
        for (j, row) in rows.iter().enumerate() {
            for (i, ch) in row.chars().enumerate() {
                let Some(d) = ch.to_digit(16) else { continue };
                let mut c = d as u8;
                if let Some((from, to)) = recolor
                    && c == from
                {
                    c = to;
                }
                self.set(x.saturating_add(i as i32), y.saturating_add(j as i32), c);
            }
        }
    }
}

/// Draws a piece of scenery into `scratch` (as a `w`×`h` canvas), outlines
/// it and copies it onto `b` at `(x, y)`. Scratch must hold `w * h` bytes.
pub(crate) fn with_outline(
    b: &mut Canvas<'_>,
    scratch: &mut [u8],
    (x, y, w, h): (i32, i32, usize, usize),
    draw: impl FnOnce(&mut Canvas<'_>),
) {
    let Some(mut t) = Canvas::new(w, h, scratch) else {
        return;
    };
    t.clear(TRANSPARENT);
    draw(&mut t);
    t.outline(crate::palette::idx::OUTLINE);
    b.blit(&t, x, y, false);
}
