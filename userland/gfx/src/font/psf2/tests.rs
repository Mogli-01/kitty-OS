extern crate std;

use self::std::vec::Vec;
use super::*;
use crate::{Canvas, Color, Point};

fn font(flags: u32, bitmap: &[u8], table: &[u8]) -> Vec<u8> {
    let mut file = MAGIC.to_vec();
    for field in [0, 36, flags, 2, 4, 2, 9] {
        file.extend_from_slice(&u32::to_le_bytes(field));
    }
    file.extend_from_slice(&[0; 4]);
    file.extend_from_slice(bitmap);
    file.extend_from_slice(table);
    file
}

#[test]
fn maps_aliases_and_draws_glyphs_with_two_bytes_per_row() {
    // á and ä alias the first glyph. 'a' and U+0301 are one sequence.
    // An 'á' alias in the second entry must not override the first one.
    let file = font(
        HAS_UNICODE_TABLE,
        &[0x80, 0, 0, 0x80, 0, 0, 0, 0],
        b"\xc3\xa1\xc3\xa4\xfea\xcc\x81\xff\xc3\xa1\xff",
    );
    let font = Psf2Font::parse(&file).unwrap();
    assert_eq!(font.bitmap().glyph_count(), 2);
    assert!(font.glyph_for_char('a').is_none());
    assert!(font.glyph_for_char('\u{0301}').is_none());
    assert!(font.glyph_for_char('x').is_none());
    assert!(font.bitmap().glyph(2).is_none());

    let ink = Color::rgb(5, 6, 7);
    let mut bytes = [0; 72];
    for character in ['á', 'ä'] {
        bytes.fill(0);
        let mut canvas = Canvas::new(&mut bytes, 9, 2, 36).unwrap();
        canvas.draw_glyph(
            Point { x: 0, y: 0 },
            font.glyph_for_char(character).unwrap(),
            ink,
        );
        assert_eq!(&bytes[..4], &ink.to_bgra());
        assert_eq!(&bytes[4..68], &[0; 64]);
        assert_eq!(&bytes[68..], &ink.to_bgra());
    }
}

#[test]
fn handles_fonts_without_unicode_and_empty_alias_lists() {
    let file = font(0, &[0; 8], b"");
    let font_view = Psf2Font::parse(&file).unwrap();
    assert!(font_view.bitmap().glyph(1).is_some());
    assert!(font_view.glyph_for_char('A').is_none());
    let file = font(HAS_UNICODE_TABLE, &[0; 8], b"\xff\xfeab\xfecd\xff");
    assert!(Psf2Font::parse(&file).is_ok());
}

#[test]
fn rejects_truncated_or_oversized_header_and_glyph_fields() {
    let baseline = font(0, &[0; 8], b"");
    for end in 0..baseline.len() {
        assert!(Psf2Font::parse(&baseline[..end]).is_err());
    }
    for (field, value, error) in [
        (0, 0, Psf2Error::InvalidHeader),
        (1, 1, Psf2Error::UnsupportedFormat),
        (2, 31, Psf2Error::InvalidHeader),
        (2, u32::MAX, Psf2Error::InvalidHeader),
        (3, 2, Psf2Error::UnsupportedFormat),
        (4, 0, Psf2Error::InvalidGlyphs),
        (4, u32::MAX, Psf2Error::InvalidGlyphs),
        (5, 3, Psf2Error::InvalidGlyphs),
        (6, 0, Psf2Error::InvalidGlyphs),
        (7, 256, Psf2Error::InvalidGlyphs),
    ] {
        let mut file = baseline.clone();
        file[field * 4..][..4].copy_from_slice(&value.to_le_bytes());
        assert!(matches!(Psf2Font::parse(&file), Err(actual) if actual == error));
    }
    let mut excess = baseline;
    excess.push(0);
    assert!(matches!(
        Psf2Font::parse(&excess),
        Err(Psf2Error::InvalidGlyphs)
    ));
}

#[test]
fn requires_one_valid_unicode_record_per_glyph() {
    for table in [
        b"".as_slice(),
        b"\xff",
        b"\xff\xff\xff",
        b"a\xffb",
        b"\xc3\xff\xff",
        b"\xfe\xff\xff",
        b"\xfe\x80\xff\xff",
    ] {
        let file = font(HAS_UNICODE_TABLE, &[0; 8], table);
        assert!(matches!(
            Psf2Font::parse(&file),
            Err(Psf2Error::InvalidUnicodeTable)
        ));
    }
}
