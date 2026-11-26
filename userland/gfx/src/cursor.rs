//! A software cursor drawn above the recomposed scene.

use crate::{Canvas, Damage, Error, Point, Rect, Surface};

/// Pointer position and a borrowed sprite with a hotspot inside the image.
pub struct Cursor<'a> {
    sprite: Surface<'a>,
    hotspot: Point,
    position: Point,
}

impl<'a> Cursor<'a> {
    pub fn new(sprite: Surface<'a>, hotspot: Point) -> Result<Self, Error> {
        if hotspot.x < 0
            || hotspot.y < 0
            || hotspot.x as u32 >= sprite.width()
            || hotspot.y as u32 >= sprite.height()
        {
            return Err(Error::InvalidHotspot);
        }
        Ok(Self {
            sprite,
            hotspot,
            position: Point { x: 0, y: 0 },
        })
    }

    /// Screen position of the hotspot, for hit testing.
    pub const fn position(&self) -> Point {
        self.position
    }

    /// Moves the hotspot within the frame and invalidates both sprite bounds.
    pub fn move_to(&mut self, at: Point, damage: &mut Damage) {
        let (width, height) = damage.dimensions();
        let at = Point {
            x: at.x.clamp(0, width as i32 - 1),
            y: at.y.clamp(0, height as i32 - 1),
        };
        if self.position != at {
            damage.record(self.bounds());
            self.position = at;
            damage.record(self.bounds());
        }
    }

    /// Applies relative motion in screen coordinates, with positive y down.
    pub fn move_by(&mut self, delta: Point, damage: &mut Damage) {
        self.move_to(
            Point {
                x: self.position.x.saturating_add(delta.x),
                y: self.position.y.saturating_add(delta.y),
            },
            damage,
        );
    }

    /// Composites the sprite only where it intersects a freshly redrawn region.
    /// Use the same screen coordinates for movement, damage and the canvas.
    pub fn draw_in(&self, canvas: &mut Canvas<'_>, region: Rect) {
        let bounds = self.bounds();
        let x = bounds.origin.x.max(region.origin.x);
        let y = bounds.origin.y.max(region.origin.y);
        let right = (i64::from(bounds.origin.x) + i64::from(bounds.width))
            .min(i64::from(region.origin.x) + i64::from(region.width));
        let bottom = (i64::from(bounds.origin.y) + i64::from(bounds.height))
            .min(i64::from(region.origin.y) + i64::from(region.height));
        if right <= i64::from(x) || bottom <= i64::from(y) {
            return;
        }
        canvas.blend(
            self.sprite,
            Rect {
                origin: Point {
                    x: x - bounds.origin.x,
                    y: y - bounds.origin.y,
                },
                width: (right - i64::from(x)) as u32,
                height: (bottom - i64::from(y)) as u32,
            },
            Point { x, y },
        );
    }

    fn bounds(&self) -> Rect {
        Rect {
            origin: Point {
                x: self.position.x - self.hotspot.x,
                y: self.position.y - self.hotspot.y,
            },
            width: self.sprite.width(),
            height: self.sprite.height(),
        }
    }
}

#[cfg(test)]
mod tests;
