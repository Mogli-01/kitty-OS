//! Text console rendered onto the linear framebuffer with an 8x8 bitmap font
//! scaled 2x (16x16 pixel cells). 100% safe code on top of [`FrameBuffer`].
//!
//! Performance note: scrolling copies the whole visible framebuffer, and
//! reading framebuffer memory is slow (it is usually mapped write-combining).
//! That is acceptable for a boot console; if it is ever measured to matter we
//! can keep a shadow buffer in RAM (Phase 3, once we have a heap).

use super::Console;
use super::font::{GLYPH_SIZE, glyph};
use crate::boot_info::FrameBuffer;

/// Integer scale applied to the 8x8 font.
const SCALE: usize = 2;
const CELL: usize = GLYPH_SIZE * SCALE;

pub struct FbConsole {
    fb: FrameBuffer,
    cols: usize,
    rows: usize,
    col: usize,
    row: usize,
    fg: u32,
    bg: u32,
}

impl FbConsole {
    pub fn new(mut fb: FrameBuffer) -> Self {
        let fg = fb.color(0xE6, 0xE6, 0xE6);
        let bg = fb.color(0x1E, 0x1A, 0x2E);
        let (w, h) = (fb.width(), fb.height());
        fb.fill_rect(0, 0, w, h, bg);
        Self {
            cols: (w / CELL).max(1),
            rows: (h / CELL).max(1),
            fb,
            col: 0,
            row: 0,
            fg,
            bg,
        }
    }

    /// Stops the text console and returns the framebuffer.
    pub fn into_framebuffer(self) -> FrameBuffer {
        self.fb
    }

    fn newline(&mut self) {
        self.col = 0;
        if self.row + 1 < self.rows {
            self.row += 1;
        } else {
            self.fb.scroll_up(CELL, self.bg);
        }
    }

    fn put_char(&mut self, c: char) {
        match c {
            '\n' => self.newline(),
            '\r' => self.col = 0,
            _ => {
                if self.col >= self.cols {
                    self.newline();
                }
                self.draw_glyph(c);
                self.col += 1;
            }
        }
    }

    fn draw_glyph(&mut self, c: char) {
        let bitmap = glyph(c);
        let x0 = self.col * CELL;
        let y0 = self.row * CELL;
        for (gy, bits) in bitmap.iter().enumerate() {
            for gx in 0..GLYPH_SIZE {
                // font8x8 convention: bit 0 is the leftmost pixel.
                let color = if bits & (1 << gx) != 0 {
                    self.fg
                } else {
                    self.bg
                };
                self.fb
                    .fill_rect(x0 + gx * SCALE, y0 + gy * SCALE, SCALE, SCALE, color);
            }
        }
    }
}

impl Console for FbConsole {
    fn write_str(&mut self, s: &str) {
        for c in s.chars() {
            self.put_char(c);
        }
    }
}
