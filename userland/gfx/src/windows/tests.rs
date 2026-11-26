use super::*;
use crate::{Canvas, Color, DoubleBuffer, Surface};

fn rect(x: i32, y: i32, width: u32, height: u32) -> Rect {
    Rect {
        origin: Point { x, y },
        width,
        height,
    }
}

fn assert_hit(stack: &WindowStack, x: i32, y: i32, id: u32, local_x: i32, local_y: i32) {
    assert_eq!(
        stack.hit_test(Point { x, y }),
        Some(WindowHit {
            id,
            local: Point {
                x: local_x,
                y: local_y
            },
        }),
    );
}

#[test]
fn intersections_handle_extreme_edges() {
    let a = rect(i32::MIN, i32::MIN, u32::MAX, u32::MAX);
    let b = rect(0, 0, u32::MAX, u32::MAX);
    let overlap = Some(rect(0, 0, i32::MAX as u32, i32::MAX as u32));
    assert_eq!(a.intersection(b), overlap);
    assert_eq!(b.intersection(a), overlap);
    assert_eq!(rect(-2, -2, 3, 3).intersection(rect(1, 0, 1, 1)), None);
    assert_eq!(a.intersection(rect(0, 0, 0, 5)), None);
}

#[test]
fn hit_testing_prefers_the_top_window_and_reports_local_coordinates() {
    let mut stack = WindowStack::new(8, 7).unwrap();
    stack.insert(5, rect(-2, -2, 7, 5)).unwrap();
    stack.insert(3, rect(1, 1, 3, 3)).unwrap();
    stack
        .insert(1, rect(7, 6, i32::MAX as u32, i32::MAX as u32))
        .unwrap();
    stack
        .insert(
            2,
            rect(i32::MIN, i32::MIN, i32::MAX as u32, i32::MAX as u32),
        )
        .unwrap();
    assert_hit(&stack, 0, 0, 5, 2, 2);
    assert_hit(&stack, 1, 1, 3, 0, 0);
    assert_hit(&stack, 4, 2, 5, 6, 4);
    assert_hit(&stack, 7, 6, 1, 0, 0);
    for (x, y) in [(-1, 1), (1, -1), (8, 6), (7, 7), (4, 3)] {
        assert_eq!(stack.hit_test(Point { x, y }), None);
    }
}

#[test]
fn failures_leave_the_stack_and_damage_unchanged() {
    assert!(matches!(
        WindowStack::new(0, 2),
        Err(Error::InvalidDimensions)
    ));
    let mut stack = WindowStack::new(10, 10).unwrap();
    for bounds in [rect(0, 0, 0, 1), rect(0, 0, 1, u32::MAX)] {
        assert_eq!(stack.insert(0, bounds), Err(Error::InvalidDimensions));
    }
    for id in 0..MAX_WINDOWS as u32 {
        stack.insert(id, rect(0, 0, 1, 1)).unwrap();
    }
    stack.damage_mut().clear();
    assert_eq!(
        stack.insert(0, rect(5, 5, 1, 1)),
        Err(Error::DuplicateWindow)
    );
    assert_eq!(
        stack.insert(u32::MAX, rect(0, 0, 1, 1)),
        Err(Error::WindowStackFull)
    );
    assert_eq!(
        stack.move_to(u32::MAX, Point { x: 8, y: 8 }),
        Err(Error::UnknownWindow)
    );
    assert_eq!(stack.raise(u32::MAX), Err(Error::UnknownWindow));
    assert_eq!(stack.remove(u32::MAX), Err(Error::UnknownWindow));
    assert_eq!(stack.windows().len(), MAX_WINDOWS);
    assert_hit(&stack, 0, 0, MAX_WINDOWS as u32 - 1, 0, 0);
    assert!(stack.damage().regions().is_empty());
    stack.remove(0).unwrap();
    stack.insert(u32::MAX, rect(0, 0, 1, 1)).unwrap();
    assert_hit(&stack, 0, 0, u32::MAX, 0, 0);
}

#[test]
fn moving_and_removing_marks_only_clipped_window_bounds() {
    let mut stack = WindowStack::new(8, 7).unwrap();
    stack.insert(0, rect(-2, 1, 4, 3)).unwrap();
    assert_eq!(stack.damage().regions(), &[rect(0, 1, 2, 3)]);
    stack.damage_mut().clear();
    stack.move_to(0, Point { x: 6, y: 3 }).unwrap();
    let regions = stack.damage().regions();
    assert_eq!(regions.len(), 2);
    assert!(regions.contains(&rect(0, 1, 2, 3)));
    assert!(regions.contains(&rect(6, 3, 2, 3)));
    stack.damage_mut().clear();
    stack.move_to(0, Point { x: 6, y: 3 }).unwrap();
    assert!(stack.damage().regions().is_empty());
    stack.remove(0).unwrap();
    assert_eq!(stack.damage().regions(), &[rect(6, 3, 2, 3)]);
    stack.damage_mut().clear();
    stack.insert(1, rect(i32::MAX, i32::MIN, 4, 3)).unwrap();
    stack
        .move_to(
            1,
            Point {
                x: i32::MIN,
                y: i32::MAX,
            },
        )
        .unwrap();
    stack.remove(1).unwrap();
    assert!(stack.damage().regions().is_empty());
}

#[test]
fn raising_marks_only_intersections_and_preserves_other_window_order() {
    let mut stack = WindowStack::new(10, 10).unwrap();
    stack.insert(1, rect(-2, 0, 6, 4)).unwrap();
    stack.insert(2, rect(-1, 0, 3, 2)).unwrap();
    stack.insert(3, rect(4, 0, 2, 2)).unwrap();
    stack.insert(4, rect(3, 3, 2, 2)).unwrap();
    stack.damage_mut().clear();
    stack.raise(1).unwrap();
    assert!(
        stack
            .windows()
            .iter()
            .map(|window| window.id)
            .eq([2, 3, 4, 1])
    );
    let regions = stack.damage().regions();
    assert_eq!(regions.len(), 2);
    assert!(regions.contains(&rect(0, 0, 2, 2)));
    assert!(regions.contains(&rect(3, 3, 1, 1)));
    assert_hit(&stack, 0, 1, 1, 2, 1);
    assert_hit(&stack, 4, 0, 3, 0, 0);
    stack.damage_mut().clear();
    stack.raise(1).unwrap();
    assert!(stack.damage().regions().is_empty());
}

const BACKGROUND: Color = Color::rgb(10, 20, 30);
const LOWER: Color = Color::rgb(40, 50, 60);
const UPPER: Color = Color::rgb(70, 80, 90);

fn recompose(stack: &mut WindowStack, front: &mut [u8], back: &mut [u8]) {
    let front = Canvas::new(front, 5, 1, 20).unwrap();
    let back = Canvas::new(back, 5, 1, 20).unwrap();
    let mut frame = DoubleBuffer::new(front, back).unwrap();
    for &change in stack.damage().regions() {
        frame.draw().fill_rect(change, BACKGROUND);
        for window in stack.windows() {
            if let Some(visible) = window.bounds.intersection(change) {
                let color = if window.id == 1 { LOWER } else { UPPER };
                frame.draw().fill_rect(visible, color);
            }
        }
    }
    frame.present_damage(stack.damage_mut()).unwrap();
}

#[test]
fn partial_recomposition_follows_stack_order_without_leaving_old_pixels() {
    let mut stack = WindowStack::new(5, 1).unwrap();
    let mut front = [0; 20];
    let mut back = [0; 20];
    stack.damage_mut().invalidate_all();
    stack.insert(1, rect(0, 0, 3, 1)).unwrap();
    stack.insert(2, rect(1, 0, 2, 1)).unwrap();
    recompose(&mut stack, &mut front, &mut back);
    let surface = Surface::new(&front, 5, 1, 20).unwrap();
    assert_eq!(surface.pixel(Point { x: 1, y: 0 }), Some(UPPER));
    stack.raise(1).unwrap();
    recompose(&mut stack, &mut front, &mut back);
    let surface = Surface::new(&front, 5, 1, 20).unwrap();
    assert_eq!(surface.pixel(Point { x: 1, y: 0 }), Some(LOWER));
    stack.move_to(1, Point { x: 4, y: 0 }).unwrap();
    recompose(&mut stack, &mut front, &mut back);
    let surface = Surface::new(&front, 5, 1, 20).unwrap();
    assert_eq!(surface.pixel(Point { x: 0, y: 0 }), Some(BACKGROUND));
    assert_eq!(surface.pixel(Point { x: 1, y: 0 }), Some(UPPER));
    assert_eq!(surface.pixel(Point { x: 4, y: 0 }), Some(LOWER));
    stack.remove(1).unwrap();
    recompose(&mut stack, &mut front, &mut back);
    let surface = Surface::new(&front, 5, 1, 20).unwrap();
    assert_eq!(surface.pixel(Point { x: 4, y: 0 }), Some(BACKGROUND));
    assert!(stack.damage().regions().is_empty());
}
