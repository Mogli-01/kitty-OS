//! Fixed-size, one-bit glyphs. Rows start at the most significant bit.

use crate::{Canvas, Color, PIXEL_BYTES, Point, clipped_steps};

mod ascii;
mod psf2;

pub use psf2::{Psf2Error, Psf2Font};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    EmptyDimensions,
    InvalidGlyphData,
}

#[derive(Clone, Copy)]
struct GlyphLayout {
    width: u8,
    height: u8,
    row_bytes: usize,
}

/// A contiguous bitmap atlas with the same dimensions for each glyph.
#[derive(Clone, Copy)]
pub struct BitmapFont<'a> {
    data: &'a [u8],
    layout: GlyphLayout,
}

/// The original kitty-OS monospace font. Indices 0–94 map ASCII space to `~`.
pub const ASCII_5X7: BitmapFont<'static> = BitmapFont {
    data: &ascii::DATA,
    layout: GlyphLayout {
        width: 5,
        height: 7,
        row_bytes: 1,
    },
};

impl<'a> BitmapFont<'a> {
    pub fn new(data: &'a [u8], width: u8, height: u8) -> Result<Self, Error> {
        if width == 0 || height == 0 {
            return Err(Error::EmptyDimensions);
        }
        let row_bytes = usize::from(width).div_ceil(8);
        if data.is_empty() || !data.len().is_multiple_of(row_bytes * usize::from(height)) {
            return Err(Error::InvalidGlyphData);
        }
        Ok(Self {
            data,
            layout: GlyphLayout {
                width,
                height,
                row_bytes,
            },
        })
    }

    pub fn glyph_count(self) -> usize {
        self.data.len() / self.glyph_bytes()
    }

    pub fn glyph(self, index: usize) -> Option<Glyph<'a>> {
        let offset = index.checked_mul(self.glyph_bytes())?;
        let data = self.data.get(offset..)?.get(..self.glyph_bytes())?;
        Some(Glyph {
            data,
            layout: self.layout,
        })
    }

    fn glyph_bytes(self) -> usize {
        self.layout.row_bytes * usize::from(self.layout.height)
    }
}

#[derive(Clone, Copy)]
pub struct Glyph<'a> {
    data: &'a [u8],
    layout: GlyphLayout,
}

impl Canvas<'_> {
    /// Draws the set bits of a glyph, preserving other pixels.
    pub fn draw_glyph(&mut self, at: Point, glyph: Glyph<'_>, color: Color) {
        let columns = clipped_steps(at.x, self.layout.width, u32::from(glyph.layout.width));
        let rows = clipped_steps(at.y, self.layout.height, u32::from(glyph.layout.height));
        if columns.is_empty() || rows.is_empty() {
            return;
        }
        let left = (i64::from(at.x) + columns.start) as usize;
        let width = (columns.end - columns.start) as usize;
        for row in rows {
            let y = (i64::from(at.y) + row) as usize;
            let bits = &glyph.data[row as usize * glyph.layout.row_bytes..];
            let range = self.layout.row(left, y, width);
            let pixels = self.bytes[range].as_chunks_mut::<PIXEL_BYTES>().0;
            for (pixel, column) in pixels.iter_mut().zip(columns.clone()) {
                let column = column as usize;
                if bits[column / 8] & (0x80 >> (column % 8)) != 0 {
                    *pixel = color.to_bgra();
                }
            }
        }
    }

    /// Draws ASCII text using a 6 × 8 cell. Newlines start a new line and
    /// characters outside printable ASCII use the `?` glyph.
    pub fn draw_ascii(&mut self, at: Point, text: &str, color: Color) {
        let left = i64::from(at.x);
        let (mut x, mut y) = (left, i64::from(at.y));
        for character in text.chars() {
            if y >= i64::from(self.layout.height) {
                break;
            }
            if character == '\n' {
                x = left;
                y += 8;
                continue;
            }
            if x < i64::from(self.layout.width) {
                let ascii = if (' '..='~').contains(&character) {
                    character as u8
                } else {
                    b'?'
                };
                if let Some(glyph) = ASCII_5X7.glyph(usize::from(ascii - b' ')) {
                    self.draw_glyph(
                        Point {
                            x: x as i32,
                            y: y as i32,
                        },
                        glyph,
                        color,
                    );
                }
            }
            x = x.saturating_add(6);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INK: Color = Color::rgb(1, 2, 3);
    const CLEAR: Color = Color::rgba(0, 0, 0, 0);

    #[test]
    fn validates_atlas_dimensions_and_glyph_boundaries() {
        assert!(matches!(
            BitmapFont::new(&[1], 0, 1),
            Err(Error::EmptyDimensions)
        ));
        assert!(matches!(
            BitmapFont::new(&[], 5, 7),
            Err(Error::InvalidGlyphData)
        ));
        assert!(matches!(
            BitmapFont::new(&[0; 3], 9, 2),
            Err(Error::InvalidGlyphData)
        ));
        let atlas = BitmapFont::new(&[0; 8], 9, 2).unwrap();
        assert_eq!(atlas.glyph_count(), 2);
        assert!(atlas.glyph(1).is_some());
        assert!(atlas.glyph(2).is_none());
        assert!(atlas.glyph(usize::MAX).is_none());
    }

    #[test]
    fn clips_glyphs_and_reads_bits_in_each_row() {
        let atlas = BitmapFont::new(&[0xff, 0xff, 0b10110000, 0b10000000], 9, 2).unwrap();
        let mut bytes = [0; 40];
        let mut canvas = Canvas::new(&mut bytes, 9, 1, 40).unwrap();
        canvas.draw_glyph(Point { x: -1, y: -1 }, atlas.glyph(0).unwrap(), INK);
        for x in 0..9 {
            let color = if [1, 2, 7].contains(&x) { INK } else { CLEAR };
            assert_eq!(canvas.as_surface().pixel(Point { x, y: 0 }), Some(color));
        }
        assert_eq!(&bytes[36..], &[0; 4]);
    }

    #[test]
    fn draws_printable_ascii_in_fixed_cells_and_falls_back_for_unicode() {
        assert_eq!(ASCII_5X7.glyph_count(), 95);
        let mut bytes = [0; 12 * 16 * PIXEL_BYTES];
        let mut expected = [0; 12 * 16 * PIXEL_BYTES];
        let mut text = Canvas::new(&mut bytes, 12, 16, 48).unwrap();
        let mut manual = Canvas::new(&mut expected, 12, 16, 48).unwrap();
        text.draw_ascii(Point { x: 0, y: 0 }, "A0\né", INK);
        for (x, y, byte) in [(0, 0, b'A'), (6, 0, b'0'), (0, 8, b'?')] {
            manual.draw_glyph(
                Point { x, y },
                ASCII_5X7.glyph(usize::from(byte - b' ')).unwrap(),
                INK,
            );
        }
        assert_eq!(bytes, expected);
        assert!(bytes.iter().any(|&byte| byte != 0));
    }
}
