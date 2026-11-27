use super::*;
use crate::{Color, DoubleBuffer};

const ZERO: Point = Point { x: 0, y: 0 };

fn rect(x: i32, y: i32, width: u32, height: u32) -> Rect {
    Rect {
        origin: Point { x, y },
        width,
        height,
    }
}

#[test]
fn clamps_relative_motion_and_marks_the_old_and_new_positions() {
    let sprite = Surface::new(&[0; 16], 2, 2, 8).unwrap();
    let hotspot = Point { x: 1, y: 1 };
    let mut cursor = Cursor::new(sprite, hotspot).unwrap();
    let mut damage = Damage::new(4, 4).unwrap();
    cursor.move_by(
        Point {
            x: i32::MAX,
            y: i32::MAX,
        },
        &mut damage,
    );
    assert_eq!(cursor.position(), Point { x: 3, y: 3 });
    assert!(damage.regions().contains(&rect(0, 0, 1, 1)));
    assert!(damage.regions().contains(&rect(2, 2, 2, 2)));
    damage.clear();
    cursor.move_by(Point { x: 42, y: 42 }, &mut damage);
    assert!(damage.regions().is_empty());
    cursor.move_by(
        Point {
            x: i32::MIN,
            y: i32::MIN,
        },
        &mut damage,
    );
    assert_eq!(cursor.position(), ZERO);
}

#[test]
fn rejects_hotspots_outside_the_image() {
    let sprite = Surface::new(&[0; 4], 1, 1, 4).unwrap();
    for hotspot in [
        Point { x: -1, y: 0 },
        Point { x: 0, y: 1 },
        Point { x: 1, y: 0 },
    ] {
        assert!(matches!(
            Cursor::new(sprite, hotspot),
            Err(Error::InvalidHotspot)
        ));
    }
}

#[test]
fn draws_only_inside_the_redrawn_region_and_keeps_sprite_alignment() {
    let pixels = [1, 2, 3, 255, 10, 20, 30, 255, 4, 5, 6, 255, 7, 8, 9, 255];
    let sprite = Surface::new(&pixels, 2, 2, 8).unwrap();
    let cursor = Cursor::new(sprite, Point { x: 1, y: 0 }).unwrap();
    let mut bytes = [0; 16];
    let mut canvas = Canvas::new(&mut bytes, 2, 2, 8).unwrap();
    cursor.draw_in(&mut canvas, rect(0, 0, 2, 1));
    let frame = canvas.as_surface();
    assert_eq!(frame.pixel(ZERO), Some(Color::rgb(30, 20, 10)));
    assert_eq!(
        frame.pixel(Point { x: 0, y: 1 }),
        Some(Color::rgba(0, 0, 0, 0))
    );
    assert_eq!(
        frame.pixel(Point { x: 1, y: 0 }),
        Some(Color::rgba(0, 0, 0, 0))
    );
}

#[test]
fn recomposes_motion_without_leaving_a_cursor_trail() {
    let sprite = Surface::new(&[0, 0, 200, 128], 1, 1, 4).unwrap();
    let mut cursor = Cursor::new(sprite, ZERO).unwrap();
    let mut damage = Damage::new(3, 1).unwrap();
    let mut front = [0; 12];
    let mut back = [0; 12];
    let background = Color::rgb(0, 0, 40);
    {
        let target = Canvas::new(&mut front, 3, 1, 12).unwrap();
        let drawing = Canvas::new(&mut back, 3, 1, 12).unwrap();
        let mut frame = DoubleBuffer::new(target, drawing).unwrap();
        damage.invalidate_all();
        for position in [ZERO, Point { x: 2, y: 0 }] {
            cursor.move_to(position, &mut damage);
            for &region in damage.regions() {
                frame.draw().fill_rect(region, background);
                cursor.draw_in(frame.draw(), region);
            }
            frame.present_damage(&mut damage).unwrap();
        }
    }
    assert_eq!(&front[0..4], &background.to_bgra());
    assert_eq!(&front[4..8], &background.to_bgra());
    assert_eq!(&front[8..12], &Color::rgb(100, 0, 20).to_bgra());
}
