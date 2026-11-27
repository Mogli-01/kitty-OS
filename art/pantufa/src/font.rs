//! A 5×7 pixel font for the art (logo, labels, speech bubbles).
//!
//! Upper case only: lower case is drawn as upper case and Portuguese accented
//! letters lose the accent (`Ç` → `C`, `Ã` → `A`). Unknown characters are
//! drawn as `?`. Each glyph row uses the low 5 bits, bit 4 on the left.

use crate::canvas::Canvas;

/// Glyph height in pixels at scale 1.
pub const GLYPH_HEIGHT: i32 = 7;
/// Horizontal advance per character at scale 1 (5 pixels + 1 gap).
pub const GLYPH_ADVANCE: i32 = 6;

type Glyph = [u8; 7];

const QUESTION: Glyph = [
    0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b00000, 0b00100,
];

const fn glyph(c: char) -> Option<Glyph> {
    Some(match c {
        'A' => [
            0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'B' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110,
        ],
        'C' => [
            0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110,
        ],
        'D' => [
            0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110,
        ],
        'E' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111,
        ],
        'F' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'G' => [
            0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01111,
        ],
        'H' => [
            0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'I' => [
            0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        'J' => [
            0b00111, 0b00010, 0b00010, 0b00010, 0b00010, 0b10010, 0b01100,
        ],
        'K' => [
            0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001,
        ],
        'L' => [
            0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111,
        ],
        'M' => [
            0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001,
        ],
        'N' => [
            0b10001, 0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001,
        ],
        'O' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'P' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'Q' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101,
        ],
        'R' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001,
        ],
        'S' => [
            0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        'T' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'U' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'V' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100,
        ],
        'W' => [
            0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b10101, 0b01010,
        ],
        'X' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001,
        ],
        'Y' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'Z' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111,
        ],
        '0' => [
            0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110,
        ],
        '1' => [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        '2' => [
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111,
        ],
        '3' => [
            0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        '4' => [
            0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
        ],
        '5' => [
            0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110,
        ],
        '6' => [
            0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110,
        ],
        '7' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ],
        '8' => [
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
        '9' => [
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100,
        ],
        '-' => [0, 0, 0, 0b01110, 0, 0, 0],
        ':' => [0, 0b00100, 0, 0, 0, 0b00100, 0],
        '.' => [0, 0, 0, 0, 0, 0, 0b00100],
        ',' => [0, 0, 0, 0, 0, 0b00100, 0b01000],
        '!' => [0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0, 0b00100],
        '_' => [0, 0, 0, 0, 0, 0, 0b11111],
        '(' => [
            0b00010, 0b00100, 0b01000, 0b01000, 0b01000, 0b00100, 0b00010,
        ],
        ')' => [
            0b01000, 0b00100, 0b00010, 0b00010, 0b00010, 0b00100, 0b01000,
        ],
        '[' => [
            0b01110, 0b01000, 0b01000, 0b01000, 0b01000, 0b01000, 0b01110,
        ],
        ']' => [
            0b01110, 0b00010, 0b00010, 0b00010, 0b00010, 0b00010, 0b01110,
        ],
        '<' => [
            0b00010, 0b00100, 0b01000, 0b10000, 0b01000, 0b00100, 0b00010,
        ],
        '>' => [
            0b01000, 0b00100, 0b00010, 0b00001, 0b00010, 0b00100, 0b01000,
        ],
        '#' => [
            0b01010, 0b01010, 0b11111, 0b01010, 0b11111, 0b01010, 0b01010,
        ],
        '/' => [
            0b00001, 0b00010, 0b00010, 0b00100, 0b01000, 0b01000, 0b10000,
        ],
        '+' => [0, 0b00100, 0b00100, 0b11111, 0b00100, 0b00100, 0],
        '=' => [0, 0, 0b11111, 0, 0b11111, 0, 0],
        '%' => [
            0b11001, 0b11010, 0b00010, 0b00100, 0b01000, 0b01011, 0b10011,
        ],
        '*' => [0, 0b10101, 0b01110, 0b11111, 0b01110, 0b10101, 0],
        ';' => [0, 0b00100, 0, 0, 0b00100, 0b00100, 0b01000],
        '\'' => [0b00100, 0b00100, 0b01000, 0, 0, 0, 0],
        '"' => [0b01010, 0b01010, 0, 0, 0, 0, 0],
        '?' => QUESTION,
        ' ' => [0; 7],
        _ => return None,
    })
}

/// Folds lower case and Portuguese accents onto the glyphs we have.
fn fold(c: char) -> char {
    match c {
        'á' | 'à' | 'â' | 'ã' | 'Á' | 'À' | 'Â' | 'Ã' => 'A',
        'é' | 'ê' | 'É' | 'Ê' => 'E',
        'í' | 'Í' => 'I',
        'ó' | 'ô' | 'õ' | 'Ó' | 'Ô' | 'Õ' => 'O',
        'ú' | 'Ú' => 'U',
        'ç' | 'Ç' => 'C',
        c => c.to_ascii_uppercase(),
    }
}

/// Whether `c` is drawn as itself (after folding) rather than as `?`.
pub fn has_glyph(c: char) -> bool {
    glyph(fold(c)).is_some()
}

/// Largest pixel scale for text; larger requests are clamped to it.
pub const MAX_TEXT_SCALE: i32 = 64;

/// Width in pixels of `s` at `scale` (clamped to 1..=[`MAX_TEXT_SCALE`]),
/// without the trailing gap. Saturates instead of overflowing.
pub fn text_width(s: &str, scale: i32) -> i32 {
    let scale = scale.clamp(1, MAX_TEXT_SCALE);
    let n = i32::try_from(s.chars().count()).unwrap_or(i32::MAX);
    if n == 0 {
        0
    } else {
        n.saturating_mul(GLYPH_ADVANCE)
            .saturating_sub(1)
            .saturating_mul(scale)
    }
}

/// Draws `s` with its top-left corner at `(x, y)`, each font pixel as a
/// `scale × scale` block (`scale` clamped to 1..=[`MAX_TEXT_SCALE`]).
/// Characters past the canvas edge are skipped. Returns the text width.
pub fn text(canvas: &mut Canvas<'_>, s: &str, x: i32, y: i32, color: u8, scale: i32) -> i32 {
    let scale = scale.clamp(1, MAX_TEXT_SCALE);
    let advance = GLYPH_ADVANCE * scale;
    let mut gx = x;
    for ch in s.chars() {
        if gx >= canvas.width() as i32 {
            break;
        }
        let g = glyph(fold(ch)).unwrap_or(QUESTION);
        for (row, bits) in (0i32..).zip(g.iter()) {
            for col in 0..5 {
                if bits & (0b10000 >> col) != 0 {
                    let px = gx.saturating_add(col * scale);
                    let py = y.saturating_add(row * scale);
                    canvas.rect(px, py, scale, scale, color);
                }
            }
        }
        gx = gx.saturating_add(advance);
    }
    text_width(s, scale)
}
