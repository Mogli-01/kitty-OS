use super::*;

const CLEAR: Color = Color::rgba(0, 0, 0, 0);
const PINK: Color = Color::rgb(222, 111, 136);

fn point(x: i32, y: i32) -> Point {
    Point { x, y }
}

fn rect(x: i32, y: i32, width: u32, height: u32) -> Rect {
    Rect {
        origin: point(x, y),
        width,
        height,
    }
}

#[test]
fn validates_dimensions_pitch_and_visible_length() {
    assert!(matches!(
        Surface::new(&[], 0, 1, 4),
        Err(Error::InvalidDimensions)
    ));
    assert!(matches!(
        Surface::new(&[], 1, u32::MAX, 4),
        Err(Error::InvalidDimensions)
    ));
    assert!(matches!(
        Surface::new(&[0; 16], 2, 1, 7),
        Err(Error::InvalidPitch)
    ));
    assert!(matches!(
        Surface::new(&[], 2, 2, usize::MAX),
        Err(Error::SizeOverflow)
    ));
    assert!(matches!(
        Canvas::new(&mut [0; 12], 2, 2, 9),
        Err(Error::BufferTooShort)
    ));
    let mut bytes = [0; 17];
    let mut canvas = Canvas::new(&mut bytes, 2, 2, 9).unwrap();
    // The last row can end at its final visible pixel, without a pad byte.
    assert!(canvas.put_pixel(point(1, 1), PINK));
    assert_eq!(&bytes[13..], &PINK.to_bgra());
}

#[test]
fn writes_bgra_bytes_and_ignores_out_of_bounds_points() {
    let mut bytes = [0; 14];
    let mut canvas = Canvas::new(&mut bytes, 2, 1, 10).unwrap();
    let color = Color::rgba(10, 20, 30, 40);
    assert!(canvas.put_pixel(point(1, 0), color));
    for p in [
        point(-1, 0),
        point(2, 0),
        point(0, 1),
        point(i32::MAX, i32::MIN),
    ] {
        assert!(!canvas.put_pixel(p, PINK));
        assert_eq!(canvas.as_surface().pixel(p), None);
    }
    assert_eq!(canvas.as_surface().pixel(point(1, 0)), Some(color));
    assert_eq!(canvas.as_surface().width(), 2);
    assert_eq!(canvas.as_surface().height(), 1);
    assert_eq!(bytes, [0, 0, 0, 0, 30, 20, 10, 40, 0, 0, 0, 0, 0, 0]);
}

#[test]
fn fills_clipped_rectangles_without_touching_padding() {
    let mut bytes = [0xa5; 40];
    let mut canvas = Canvas::new(&mut bytes, 3, 2, 16).unwrap();
    canvas.fill_rect(rect(-1, 1, 3, u32::MAX), PINK);
    for no_op in [rect(0, 0, 0, 1), rect(4, 0, 1, 1), rect(i32::MIN, -5, 1, 5)] {
        canvas.fill_rect(no_op, CLEAR);
    }
    assert_eq!(&bytes[..16], &[0xa5; 16]);
    assert_eq!(&bytes[16..20], &PINK.to_bgra());
    assert_eq!(&bytes[20..24], &PINK.to_bgra());
    assert_eq!(&bytes[24..], &[0xa5; 16]);
}

#[test]
fn blit_clips_both_views_and_preserves_relative_alignment() {
    let mut source = [0xcc; 60];
    let mut draw_source = Canvas::new(&mut source, 4, 3, 20).unwrap();
    for y in 0..3 {
        for x in 0..4 {
            draw_source.put_pixel(point(x, y), Color::rgba((y * 4 + x + 1) as u8, 1, 2, 3));
        }
    }
    let source = draw_source.as_surface();

    let mut bytes = [0; 96];
    let mut target = Canvas::new(&mut bytes, 5, 4, 24).unwrap();
    target.blit(source, rect(-2, 0, 8, 4), point(-1, 1));
    let image = target.as_surface();
    for y in 0..4 {
        for x in 0..5 {
            let expected = if y > 0 && x > 0 {
                source.pixel(point(x - 1, y - 1))
            } else {
                Some(CLEAR)
            };
            assert_eq!(image.pixel(point(x, y)), expected);
        }
    }
    target.blit(source, rect(1, 1, u32::MAX, u32::MAX), point(-2, -1));
    assert_eq!(
        target.as_surface().pixel(point(0, 0)),
        source.pixel(point(3, 2))
    );
    assert_eq!(target.as_surface().pixel(point(1, 0)), Some(CLEAR));
    for row in bytes.as_chunks::<24>().0 {
        assert_eq!(&row[20..24], &[0; 4]);
    }
}

#[test]
fn line_rounding_is_consistent_when_clipped_or_reversed() {
    let mut bytes = [0; 4 * 4 * 4];
    let mut reversed = [0; 4 * 4 * 4];
    let mut canvas = Canvas::new(&mut bytes, 4, 4, 16).unwrap();
    let mut other = Canvas::new(&mut reversed, 4, 4, 16).unwrap();
    canvas.line(point(-2, -1), point(3, 2), PINK);
    other.line(point(3, 2), point(-2, -1), PINK);
    let image = canvas.as_surface();
    let lit = [point(0, 0), point(1, 1), point(2, 1), point(3, 2)];
    for y in 0..4 {
        for x in 0..4 {
            let p = point(x, y);
            let expected = if lit.contains(&p) { PINK } else { CLEAR };
            assert_eq!(image.pixel(p), Some(expected));
            assert_eq!(other.as_surface().pixel(p), Some(expected));
        }
    }
}

#[test]
fn clips_lines_with_extreme_coordinates_to_visible_positions() {
    let mut bytes = [0; 4 * 4 * 4];
    let mut canvas = Canvas::new(&mut bytes, 4, 4, 16).unwrap();
    canvas.line(point(i32::MIN, i32::MIN), point(i32::MAX, i32::MAX), PINK);
    canvas.line(point(-1, i32::MIN), point(-1, i32::MAX), PINK); // entirely out of view
    let green = Color::rgb(1, 240, 80);
    canvas.line(point(3, i32::MAX), point(3, i32::MIN), green);
    canvas.line(point(1, 3), point(1, 3), green);
    let image = canvas.as_surface();
    for y in 0..4 {
        for x in 0..4 {
            let expected = if x == 3 || (x == 1 && y == 3) {
                green
            } else if x == y {
                PINK
            } else {
                CLEAR
            };
            assert_eq!(image.pixel(point(x, y)), Some(expected));
        }
    }
}
