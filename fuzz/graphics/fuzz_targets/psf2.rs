#![no_main]

use kitty_gfx::{Canvas, Color, Point, font::Psf2Font};
use libfuzzer_sys::fuzz_target;

fn draw_and_check(font: Psf2Font<'_>) {
    let bitmap = font.bitmap();
    let glyph_count = bitmap.glyph_count();
    assert!(glyph_count > 0);
    let glyph = bitmap.glyph(glyph_count - 1).unwrap();
    assert!(bitmap.glyph(glyph_count).is_none());

    let mut pixels = [0xa5; 35];
    let mut canvas = Canvas::new(&mut pixels, 3, 2, 16).unwrap();
    let origin = Point { x: -1, y: -1 };
    let ink = Color::rgb(10, 20, 30);
    canvas.draw_glyph(origin, glyph, ink);
    if let Some(alias) = font.glyph_for_char('é') {
        canvas.draw_glyph(origin, alias, ink);
    }
    assert_eq!(&pixels[12..16], &[0xa5; 4]);
    assert_eq!(&pixels[28..], &[0xa5; 7]);
}

fn append_header(font: &mut Vec<u8>, flags: u32, count: u32, width: u32, height: u32) {
    let glyph_bytes = width.div_ceil(8) * height;
    for field in [
        0x864ab572_u32,
        0,
        32,
        flags,
        count,
        glyph_bytes,
        height,
        width,
    ] {
        font.extend_from_slice(&field.to_le_bytes());
    }
}

fuzz_target!(|input: &[u8]| {
    // Give the fuzzer a route into the Unicode parser without guessing the
    // entire header. The two glyphs are 9 × 2, four bytes each.
    let mut font = Vec::new();
    append_header(&mut font, 1, 2, 9, 2);
    font.extend_from_slice(&[0, 0x80, 0x80, 0, 0x80, 0, 0x80, 0x80]);
    font.extend_from_slice(input);
    for bytes in [input, font.as_slice()] {
        if let Ok(parsed) = Psf2Font::parse(bytes) {
            draw_and_check(parsed);
        }
    }

    let Some((shape, _)) = input.split_first_chunk::<3>() else {
        return;
    };
    // Build a complete atlas even from short inputs. This exercises changing
    // row strides and glyph offsets independently of the Unicode table.
    let width = u32::from(shape[0]) % 32 + 1;
    let height = u32::from(shape[1]) % 24 + 1;
    let count = u32::from(shape[2]) % 4 + 1;
    let bitmap_bytes = (width.div_ceil(8) * height * count) as usize;
    font.clear();
    append_header(&mut font, 0, count, width, height);
    font.extend(input.iter().copied().cycle().take(bitmap_bytes));
    let parsed = Psf2Font::parse(&font).unwrap();
    let bitmap = parsed.bitmap();
    assert_eq!(bitmap.glyph_count(), count as usize);
    assert!(parsed.glyph_for_char('é').is_none());
    draw_and_check(parsed);
});
