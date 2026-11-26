//! Software raster operations on borrowed user-space BGRA8888 buffers.

#![no_std]
#![forbid(unsafe_code)]

use core::ops::Range;

mod alpha;
mod buffering;
mod cursor;
mod damage;
pub use buffering::DoubleBuffer;
pub use cursor::Cursor;
pub use damage::Damage;

const PIXEL_BYTES: usize = 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Color([u8; PIXEL_BYTES]);

impl Color {
    pub const fn rgb(red: u8, green: u8, blue: u8) -> Self {
        Self::rgba(red, green, blue, 255)
    }

    pub const fn rgba(red: u8, green: u8, blue: u8, alpha: u8) -> Self {
        Self([blue, green, red, alpha])
    }

    pub const fn to_bgra(self) -> [u8; PIXEL_BYTES] {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Rect {
    pub origin: Point,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum Error {
    InvalidDimensions,
    InvalidPitch,
    SizeOverflow,
    BufferTooShort,
    InvalidHotspot,
    MismatchedDimensions,
}

#[derive(Clone, Copy)]
struct Layout {
    width: u32,
    height: u32,
    pitch: usize,
    length: usize,
}

impl Layout {
    fn new(width: u32, height: u32, pitch: usize) -> Result<Self, Error> {
        // Coordinates in a valid surface fit in Point, including the last
        // visible column and row.
        if width == 0 || height == 0 || width > i32::MAX as u32 || height > i32::MAX as u32 {
            return Err(Error::InvalidDimensions);
        }
        let columns = usize::try_from(width).map_err(|_| Error::SizeOverflow)?;
        let rows = usize::try_from(height).map_err(|_| Error::SizeOverflow)?;
        let row = columns
            .checked_mul(PIXEL_BYTES)
            .ok_or(Error::SizeOverflow)?;
        if pitch < row {
            return Err(Error::InvalidPitch);
        }
        let length = (rows - 1)
            .checked_mul(pitch)
            .and_then(|offset| offset.checked_add(row))
            .ok_or(Error::SizeOverflow)?;
        Ok(Self {
            width,
            height,
            pitch,
            length,
        })
    }

    fn pixel(self, point: Point) -> Option<Range<usize>> {
        let x = u32::try_from(point.x).ok()?;
        let y = u32::try_from(point.y).ok()?;
        if x >= self.width || y >= self.height {
            return None;
        }
        Some(self.row(x as usize, y as usize, 1))
    }

    // Used only with clipped coordinates. Layout::new validated all offsets
    // through the end of the last visible row.
    fn row(self, x: usize, y: usize, width: usize) -> Range<usize> {
        let start = y * self.pitch + x * PIXEL_BYTES;
        start..start + width * PIXEL_BYTES
    }
}

/// Read-only view of a surface. Pitch is in bytes; row padding is ignored.
#[derive(Clone, Copy)]
pub struct Surface<'a> {
    bytes: &'a [u8],
    layout: Layout,
}

impl<'a> Surface<'a> {
    pub fn new(bytes: &'a [u8], width: u32, height: u32, pitch: usize) -> Result<Self, Error> {
        let layout = Layout::new(width, height, pitch)?;
        let bytes = bytes.get(..layout.length).ok_or(Error::BufferTooShort)?;
        Ok(Self { bytes, layout })
    }

    pub const fn width(self) -> u32 {
        self.layout.width
    }

    pub const fn height(self) -> u32 {
        self.layout.height
    }

    pub fn pixel(self, at: Point) -> Option<Color> {
        let range = self.layout.pixel(at)?;
        let bytes = self.bytes[range].try_into().ok()?;
        Some(Color(bytes))
    }
}

/// Exclusive drawing view. Drawing and blitting preserve row padding.
pub struct Canvas<'a> {
    bytes: &'a mut [u8],
    layout: Layout,
}

impl<'a> Canvas<'a> {
    pub fn new(bytes: &'a mut [u8], width: u32, height: u32, pitch: usize) -> Result<Self, Error> {
        let layout = Layout::new(width, height, pitch)?;
        let bytes = bytes
            .get_mut(..layout.length)
            .ok_or(Error::BufferTooShort)?;
        Ok(Self { bytes, layout })
    }

    pub fn as_surface(&self) -> Surface<'_> {
        Surface {
            bytes: self.bytes,
            layout: self.layout,
        }
    }

    /// Returns whether the pixel fell inside the visible surface.
    pub fn put_pixel(&mut self, at: Point, color: Color) -> bool {
        let Some(range) = self.layout.pixel(at) else {
            return false;
        };
        self.bytes[range].copy_from_slice(&color.0);
        true
    }

    pub fn fill_rect(&mut self, rect: Rect, color: Color) {
        let x = clipped_steps(rect.origin.x, self.layout.width, rect.width);
        let y = clipped_steps(rect.origin.y, self.layout.height, rect.height);
        if x.is_empty() || y.is_empty() {
            return;
        }
        let left = (i64::from(rect.origin.x) + x.start) as usize;
        let width = (x.end - x.start) as usize;
        for offset in y {
            let row = (i64::from(rect.origin.y) + offset) as usize;
            let range = self.layout.row(left, row, width);
            for pixel in self.bytes[range].as_chunks_mut::<PIXEL_BYTES>().0 {
                *pixel = color.0;
            }
        }
    }

    /// Copies pixels from a source rectangle, clipping both surfaces while
    /// preserving the source-to-destination alignment. Copies alpha as stored.
    pub fn blit(&mut self, source: Surface<'_>, rect: Rect, at: Point) {
        self.transfer_rows(source, rect, at, |target, source| {
            target.copy_from_slice(source);
        });
    }

    fn transfer_rows(
        &mut self,
        source: Surface<'_>,
        rect: Rect,
        at: Point,
        mut transfer: impl FnMut(&mut [u8], &[u8]),
    ) {
        let Some((sx, dx, width)) = span(
            rect.origin.x,
            source.layout.width,
            at.x,
            self.layout.width,
            rect.width,
        ) else {
            return;
        };
        let Some((sy, dy, height)) = span(
            rect.origin.y,
            source.layout.height,
            at.y,
            self.layout.height,
            rect.height,
        ) else {
            return;
        };
        for row in 0..height {
            let source_range = source.layout.row(sx, sy + row, width);
            let target_range = self.layout.row(dx, dy + row, width);
            transfer(&mut self.bytes[target_range], &source.bytes[source_range]);
        }
    }

    /// Draws the visible pixels of an integer line including both endpoints.
    /// The loop walks at most one visible row or column on the major axis.
    pub fn line(&mut self, a: Point, b: Point, color: Color) {
        if a == b {
            self.put_pixel(a, color);
            return;
        }
        let steep = a.y.abs_diff(b.y) > a.x.abs_diff(b.x);
        // Work with increasing coordinates on the major axis so that
        // reversing the endpoints chooses the same pixels at tie points.
        let (mut start, mut end, major_extent, minor_extent) = if steep {
            (
                (a.y, a.x),
                (b.y, b.x),
                self.layout.height,
                self.layout.width,
            )
        } else {
            (
                (a.x, a.y),
                (b.x, b.y),
                self.layout.width,
                self.layout.height,
            )
        };
        if start.0 > end.0 {
            core::mem::swap(&mut start, &mut end);
        }
        if start.1.max(end.1) < 0 || i64::from(start.1.min(end.1)) >= i64::from(minor_extent) {
            return;
        }
        let first = start.0.max(0);
        let last = end.0.min(major_extent as i32 - 1);
        if first > last {
            return;
        }

        let major_length = u64::from(start.0.abs_diff(end.0));
        let minor_length = u64::from(start.1.abs_diff(end.1));
        let minor_step = (i64::from(end.1) - i64::from(start.1)).signum();
        // Jump to the first visible major coordinate with the same rounding
        // as the full line. Both lengths and the skipped distance fit in u32.
        let skipped = u64::from(start.0.abs_diff(first));
        let distance = minor_length * skipped + major_length / 2;
        let mut minor = i64::from(start.1) + minor_step * (distance / major_length) as i64;
        let mut error = distance % major_length;
        for major in first..=last {
            let (x, y) = if steep {
                (minor as i32, major)
            } else {
                (major, minor as i32)
            };
            self.put_pixel(Point { x, y }, color);
            error += minor_length;
            if error >= major_length {
                error -= major_length;
                minor += minor_step;
            }
        }
    }
}

// Both positions move by the same displacement. Clip the displacement to
// the intersection of the two visible spans before converting to indices.
fn span(
    a: i32,
    a_extent: u32,
    b: i32,
    b_extent: u32,
    length: u32,
) -> Option<(usize, usize, usize)> {
    let a_steps = clipped_steps(a, a_extent, length);
    let b_steps = clipped_steps(b, b_extent, length);
    let start = a_steps.start.max(b_steps.start);
    let end = a_steps.end.min(b_steps.end);
    if start >= end {
        return None;
    }
    Some((
        (i64::from(a) + start) as usize,
        (i64::from(b) + start) as usize,
        (end - start) as usize,
    ))
}

fn clipped_steps(origin: i32, extent: u32, length: u32) -> Range<i64> {
    let origin = i64::from(origin);
    (-origin).max(0)..(i64::from(extent) - origin).min(i64::from(length))
}

#[cfg(test)]
mod tests;
