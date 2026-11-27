use super::*;
use crate::{Canvas, Color, DoubleBuffer};

fn rect(x: i32, y: i32, width: u32, height: u32) -> Rect {
    Rect {
        origin: Point { x, y },
        width,
        height,
    }
}

#[test]
fn clips_to_the_frame_and_ignores_empty_regions() {
    assert!(matches!(Damage::new(0, 10), Err(Error::InvalidDimensions)));
    assert!(matches!(
        Damage::new(10, u32::MAX),
        Err(Error::InvalidDimensions)
    ));
    let mut damage = Damage::new(8, 6).unwrap();
    damage.record(rect(4, 4, 0, 1));
    damage.record(rect(i32::MAX, 0, u32::MAX, 1));
    damage.record(rect(i32::MIN, i32::MIN, 1, 1));
    assert!(damage.regions().is_empty());
    damage.record(rect(-2, 4, u32::MAX, u32::MAX));
    assert_eq!(damage.regions(), &[rect(0, 4, 8, 2)]);
}

#[test]
fn rechecks_overlaps_after_a_region_grows() {
    let mut damage = Damage::new(10, 10).unwrap();
    damage.record(rect(0, 0, 3, 2));
    damage.record(rect(2, 3, 2, 2));
    assert_eq!(damage.regions().len(), 2);
    damage.record(rect(3, 1, 2, 3));
    assert_eq!(damage.regions(), &[rect(0, 0, 5, 5)]);
    damage.record(rect(5, 0, 1, 1));
    assert_eq!(damage.regions().len(), 2);
}

#[test]
fn falls_back_to_the_full_frame_when_the_list_is_full() {
    let mut damage = Damage::new(40, 2).unwrap();
    for x in (0..32).step_by(2) {
        damage.record(rect(x, 0, 1, 1));
    }
    assert_eq!(damage.regions().len(), MAX_REGIONS);
    damage.record(rect(0, 0, 1, 1));
    assert_eq!(damage.regions().len(), MAX_REGIONS);
    damage.record(rect(32, 0, 1, 1));
    assert_eq!(damage.regions(), &[rect(0, 0, 40, 2)]);
    damage.record(rect(0, 1, 1, 1));
    assert_eq!(damage.regions(), &[rect(0, 0, 40, 2)]);
    damage.clear();
    damage.record(rect(2, 1, 1, 1));
    assert_eq!(damage.regions(), &[rect(2, 1, 1, 1)]);
}

#[test]
fn presents_marked_pixels_and_keeps_pending_damage_on_error() {
    let mut front = [0x55; 16];
    let mut back = [0; 16];
    let ink = Color::rgb(1, 2, 3);
    {
        let target = Canvas::new(&mut front, 2, 2, 8).unwrap();
        let drawing = Canvas::new(&mut back, 2, 2, 8).unwrap();
        let mut buffers = DoubleBuffer::new(target, drawing).unwrap();
        let mut wrong = Damage::new(3, 2).unwrap();
        wrong.invalidate_all();
        assert_eq!(
            buffers.present_damage(&mut wrong),
            Err(Error::MismatchedDimensions)
        );
        assert_eq!(wrong.regions(), &[rect(0, 0, 3, 2)]);
        let mut damage = Damage::new(2, 2).unwrap();
        buffers.draw().fill_rect(rect(0, 0, 2, 2), ink);
        damage.record(rect(1, 1, 1, 1));
        buffers.present_damage(&mut damage).unwrap();
        assert!(damage.regions().is_empty());
    }
    assert_eq!(&front[..12], &[0x55; 12]);
    assert_eq!(&front[12..], &ink.to_bgra());
}
