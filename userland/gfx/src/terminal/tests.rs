use core::fmt::Write;

use super::*;

#[test]
fn validates_dimensions_and_only_initializes_visible_cells() {
    let mut storage = ['x'; 5];
    assert!(matches!(
        TextGrid::new(&mut storage, 0, 1),
        Err(Error::InvalidDimensions)
    ));
    assert!(matches!(
        TextGrid::new(&mut storage, 2, 3),
        Err(Error::BufferTooShort)
    ));
    assert!(TextGrid::new(&mut storage, u32::MAX, u32::MAX).is_err());
    assert_eq!(storage, ['x'; 5]);
    {
        let mut grid = TextGrid::new(&mut storage, 2, 2).unwrap();
        assert_eq!(grid.cell(2, 0), None);
        assert_eq!(grid.cell(0, 2), None);
        grid.write_str("text\na").unwrap();
        grid.clear();
        assert_eq!(grid.cursor(), (0, 0));
        assert_eq!(grid.cell(1, 1), Some(' '));
    }
    assert_eq!(storage, [' ', ' ', ' ', ' ', 'x']);
}

#[test]
fn wraps_text_across_writes_and_scrolls_one_row_at_a_time() {
    let mut storage = [' '; 6];
    let mut grid = TextGrid::new(&mut storage, 3, 2).unwrap();
    grid.write_str("aé🦀").unwrap();
    assert_eq!(grid.cursor(), (2, 0));
    assert_eq!(grid.cell(1, 0), Some('é'));
    assert_eq!(grid.cell(2, 0), Some('🦀'));
    grid.write_char('x').unwrap();
    assert_eq!(grid.cursor(), (1, 1));
    assert_eq!(grid.cell(0, 1), Some('x'));
    grid.write_str("yz\nk").unwrap();
    assert_eq!(grid.cursor(), (1, 1));
    assert_eq!(grid.cell(0, 0), Some('x'));
    assert_eq!(grid.cell(2, 0), Some('z'));
    assert_eq!(grid.cell(0, 1), Some('k'));
    assert_eq!(grid.cell(1, 1), Some(' '));
    grid.write_str("\nq").unwrap();
    assert_eq!(grid.cell(0, 0), Some('k'));
    assert_eq!(grid.cell(0, 1), Some('q'));
    assert_eq!(grid.cell(2, 1), Some(' '));
}

#[test]
fn handles_controls_on_a_single_row() {
    let mut storage = [' '; 10];
    let mut grid = TextGrid::new(&mut storage, 10, 1).unwrap();
    grid.write_str("abc\rX\u{8}Y\tZ\u{1b}\u{85}").unwrap();
    assert_eq!(grid.cursor(), (9, 0));
    assert_eq!(grid.cell(0, 0), Some('Y'));
    assert_eq!(grid.cell(1, 0), Some('b'));
    assert_eq!(grid.cell(8, 0), Some('Z'));
    grid.write_str("W\u{8}Q").unwrap();
    assert_eq!(grid.cursor(), (9, 0));
    assert_eq!(grid.cell(9, 0), Some('Q'));
    grid.write_str("\n!").unwrap();
    assert_eq!(grid.cursor(), (1, 0));
    assert_eq!(grid.cell(0, 0), Some('!'));
    assert_eq!(grid.cell(9, 0), Some(' '));
}

#[test]
fn renders_scrolled_psf2_cells_and_fallback_with_clipping() {
    // Two 2 × 2 glyphs. `é` uses off-diagonal pixels and `?` uses the
    // other diagonal. Each entry has its own Unicode alias.
    let font = Psf2Font::parse(&[
        0x72, 0xb5, 0x4a, 0x86, 0, 0, 0, 0, 32, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0, 2, 0, 0, 0, 2, 0,
        0, 0, 2, 0, 0, 0, 0x40, 0x80, 0x80, 0x40, 0xc3, 0xa9, 0xff, b'?', 0xff,
    ])
    .unwrap();

    let mut storage = [' '; 4];
    let mut grid = TextGrid::new(&mut storage, 2, 2).unwrap();
    grid.write_str("q\né🦀\n").unwrap();
    let foreground = Color::rgb(10, 20, 30);
    let background = Color::rgb(40, 50, 60);
    let mut pixels = [0xa5; 5 * 17];
    let mut canvas = Canvas::new(&mut pixels, 4, 5, 17).unwrap();
    grid.draw_psf2(
        &mut canvas,
        Point { x: -1, y: 0 },
        font,
        foreground,
        background,
    );
    let surface = canvas.as_surface();
    for y in 0..5 {
        for x in 0..4 {
            let expected = match (x, y) {
                (0 | 1, 0) | (2, 1) => foreground,
                (0..=2, 0..=3) => background,
                _ => Color::rgba(0xa5, 0xa5, 0xa5, 0xa5),
            };
            assert_eq!(surface.pixel(Point { x, y }), Some(expected));
        }
    }
    for row in pixels.as_chunks::<17>().0 {
        assert_eq!(row[16], 0xa5);
    }

    let original = pixels;
    let mut canvas = Canvas::new(&mut pixels, 4, 5, 17).unwrap();
    for x in [i32::MIN, i32::MAX] {
        grid.draw_psf2(&mut canvas, Point { x, y: 0 }, font, foreground, background);
    }
    assert_eq!(pixels, original);
}
