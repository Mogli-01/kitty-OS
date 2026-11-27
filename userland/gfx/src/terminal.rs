//! A bounded, fixed-width text surface for user-space terminal output.

use core::{fmt, ops::Range};

use crate::{Canvas, Color, Error, Point, Rect, clipped_steps, font::Psf2Font};

/// A viewport of Unicode scalars backed by a caller-owned cell buffer.
/// Each scalar occupies one cell. Scrolling reuses the oldest row.
pub struct TextGrid<'a> {
    cells: &'a mut [char],
    columns: u32,
    rows: u32,
    first_row: u32,
    row: u32,
    column: u32,
}

impl<'a> TextGrid<'a> {
    /// Blanks the cells in the viewport and leaves extra storage untouched.
    pub fn new(storage: &'a mut [char], columns: u32, rows: u32) -> Result<Self, Error> {
        if columns == 0 || rows == 0 {
            return Err(Error::InvalidDimensions);
        }
        let count = usize::try_from(u64::from(columns) * u64::from(rows))
            .map_err(|_| Error::SizeOverflow)?;
        let cells = storage.get_mut(..count).ok_or(Error::BufferTooShort)?;
        cells.fill(' ');
        Ok(Self {
            cells,
            columns,
            rows,
            first_row: 0,
            row: 0,
            column: 0,
        })
    }

    /// Returns `(column, row)` of the visible cursor.
    pub fn cursor(&self) -> (u32, u32) {
        (self.column.min(self.columns - 1), self.row)
    }

    pub fn cell(&self, column: u32, row: u32) -> Option<char> {
        if column >= self.columns || row >= self.rows {
            return None;
        }
        Some(self.cells[self.row_offset(row) + column as usize])
    }

    pub fn clear(&mut self) {
        self.cells.fill(' ');
        self.first_row = 0;
        self.row = 0;
        self.column = 0;
    }

    /// Draws the viewport with the glyph dimensions of the given PSF2 font.
    /// Fills the background, then draws only cells that intersect the canvas.
    pub fn draw_psf2(
        &self,
        canvas: &mut Canvas<'_>,
        origin: Point,
        font: Psf2Font<'_>,
        foreground: Color,
        background: Color,
    ) {
        let (width, height) = font.bitmap().dimensions();
        let (cell_width, cell_height) = (u32::from(width), u32::from(height));
        // No visible pixel can be more than u32::MAX steps from an i32
        // origin, so saturating the extent preserves the visible area.
        let bounds = Rect {
            origin,
            width: self.columns.saturating_mul(cell_width),
            height: self.rows.saturating_mul(cell_height),
        };
        let columns = visible_cells(origin.x, canvas.layout.width, bounds.width, cell_width);
        let rows = visible_cells(origin.y, canvas.layout.height, bounds.height, cell_height);
        if columns.is_empty() || rows.is_empty() {
            return;
        }

        canvas.fill_rect(bounds, background);
        let fallback = font
            .glyph_for_char('\u{fffd}')
            .or_else(|| font.glyph_for_char('?'));
        for row in rows {
            let offset = self.row_offset(row);
            for column in columns.clone() {
                let character = self.cells[offset + column as usize];
                if character == ' ' {
                    continue;
                }
                if let Some(glyph) = font.glyph_for_char(character).or(fallback) {
                    // The starting point of each visible cell fits in i32.
                    let at = Point {
                        x: (i64::from(origin.x) + i64::from(column) * i64::from(cell_width)) as i32,
                        y: (i64::from(origin.y) + i64::from(row) * i64::from(cell_height)) as i32,
                    };
                    canvas.draw_glyph(at, glyph, foreground);
                }
            }
        }
    }

    fn row_offset(&self, row: u32) -> usize {
        let physical = (u64::from(self.first_row) + u64::from(row)) % u64::from(self.rows);
        // The constructor validated columns * rows against the cell slice.
        physical as usize * self.columns as usize
    }

    fn next_line(&mut self) {
        self.column = 0;
        if self.row + 1 < self.rows {
            self.row += 1;
        } else {
            self.first_row = (self.first_row + 1) % self.rows;
            let offset = self.row_offset(self.row);
            self.cells[offset..offset + self.columns as usize].fill(' ');
        }
    }

    fn push(&mut self, character: char) {
        match character {
            '\n' => self.next_line(),
            '\r' => self.column = 0,
            '\u{8}' => self.column = self.column.saturating_sub(1),
            '\t' => {
                let next_tab = self.column.saturating_add(8 - self.column % 8);
                self.column = next_tab.min(self.columns - 1);
            }
            character if !character.is_control() => {
                // Defer wrapping until another printable character, so a
                // newline after filling a row advances exactly once.
                if self.column == self.columns {
                    self.next_line();
                }
                let offset = self.row_offset(self.row);
                self.cells[offset + self.column as usize] = character;
                self.column += 1;
            }
            _ => {}
        }
    }
}

impl fmt::Write for TextGrid<'_> {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        for character in text.chars() {
            self.push(character);
        }
        Ok(())
    }

    fn write_char(&mut self, character: char) -> fmt::Result {
        self.push(character);
        Ok(())
    }
}

fn visible_cells(origin: i32, canvas: u32, pixels: u32, cell: u32) -> Range<u32> {
    let clipped = clipped_steps(origin, canvas, pixels);
    if clipped.is_empty() {
        return 0..0;
    }
    (clipped.start as u32 / cell)..(clipped.end as u32).div_ceil(cell)
}

#[cfg(test)]
mod tests;
