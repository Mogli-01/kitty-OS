//! Bounded tracking of areas that need to be redrawn or presented.

use crate::{Error, Point, Rect, clipped_steps};

const MAX_REGIONS: usize = 16;

/// Tracks dirty rectangles in the coordinates of one frame. Overlapping
/// regions can grow to their bounding rectangle. If storage fills, the next
/// distinct region invalidates the entire frame.
pub struct Damage {
    frame: Rect,
    regions: [Rect; MAX_REGIONS],
    len: usize,
}

impl Damage {
    pub fn new(width: u32, height: u32) -> Result<Self, Error> {
        if width == 0 || height == 0 || width > i32::MAX as u32 || height > i32::MAX as u32 {
            return Err(Error::InvalidDimensions);
        }
        let frame = Rect {
            origin: Point { x: 0, y: 0 },
            width,
            height,
        };
        Ok(Self {
            frame,
            regions: [frame; MAX_REGIONS],
            len: 0,
        })
    }

    /// Records visible pixels only. The order of the regions is unspecified.
    pub fn record(&mut self, rect: Rect) {
        let x = clipped_steps(rect.origin.x, self.frame.width, rect.width);
        let y = clipped_steps(rect.origin.y, self.frame.height, rect.height);
        if x.is_empty() || y.is_empty() {
            return;
        }
        let mut region = Rect {
            origin: Point {
                x: (i64::from(rect.origin.x) + x.start) as i32,
                y: (i64::from(rect.origin.y) + y.start) as i32,
            },
            width: (x.end - x.start) as u32,
            height: (y.end - y.start) as u32,
        };
        let mut index = 0;
        while index < self.len {
            if let Some(combined) = overlapping_bounds(region, self.regions[index]) {
                region = combined;
                self.len -= 1;
                self.regions[index] = self.regions[self.len];
                // A bounding rectangle can now overlap an earlier region.
                index = 0;
            } else {
                index += 1;
            }
        }
        if self.len == MAX_REGIONS {
            self.invalidate_all();
            return;
        }
        self.regions[self.len] = region;
        self.len += 1;
    }

    /// Marks the whole frame, for example on the first frame or after reset.
    pub fn invalidate_all(&mut self) {
        self.regions[0] = self.frame;
        self.len = 1;
    }

    /// Regions that need to be redrawn. This may include pixels outside the
    /// original rectangles after coalescing or falling back to a full frame.
    pub fn regions(&self) -> &[Rect] {
        &self.regions[..self.len]
    }

    pub fn clear(&mut self) {
        self.len = 0;
    }

    pub(crate) const fn dimensions(&self) -> (u32, u32) {
        (self.frame.width, self.frame.height)
    }
}

// The inputs have already been clipped to the same frame.
fn overlapping_bounds(a: Rect, b: Rect) -> Option<Rect> {
    let a_right = a.origin.x as u32 + a.width;
    let b_right = b.origin.x as u32 + b.width;
    let a_bottom = a.origin.y as u32 + a.height;
    let b_bottom = b.origin.y as u32 + b.height;
    if a.origin.x as u32 >= b_right
        || b.origin.x as u32 >= a_right
        || a.origin.y as u32 >= b_bottom
        || b.origin.y as u32 >= a_bottom
    {
        return None;
    }
    let x = a.origin.x.min(b.origin.x);
    let y = a.origin.y.min(b.origin.y);
    Some(Rect {
        origin: Point { x, y },
        width: a_right.max(b_right) - x as u32,
        height: a_bottom.max(b_bottom) - y as u32,
    })
}

#[cfg(test)]
mod tests;
