#![no_main]

use kitty_gfx::{Canvas, Color, Point, font::Psf2Font};
use libfuzzer_sys::fuzz_target;

fn parse_and_draw(input: &[u8]) {
    let Ok(font) = Psf2Font::parse(input) else {
        return;
    };
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

fuzz_target!(|input: &[u8]| {
    parse_and_draw(input);

    // Give the fuzzer a route into the Unicode parser without guessing the
    // entire header. The two glyphs are 9 × 2, four bytes each.
    let mut font = Vec::new();
    for field in [0x864ab572_u32, 0, 32, 1, 2, 4, 2, 9] {
        font.extend_from_slice(&field.to_le_bytes());
    }
    font.extend_from_slice(&[0, 0x80, 0x80, 0, 0x80, 0, 0x80, 0x80]);
    font.extend_from_slice(input);
    parse_and_draw(&font);
});
