use crate::{Canvas, Damage, Error, Point, Rect};

/// An exclusive pair of canvases for drawing and presenting in separate
/// buffers. The two views may have different pitches.
pub struct DoubleBuffer<'front, 'back> {
    front: Canvas<'front>,
    back: Canvas<'back>,
}

impl<'front, 'back> DoubleBuffer<'front, 'back> {
    /// Both buffers must have the same visible dimensions. Ownership of the
    /// views prevents the buffers from overlapping.
    pub fn new(front: Canvas<'front>, back: Canvas<'back>) -> Result<Self, Error> {
        if front.layout.width != back.layout.width || front.layout.height != back.layout.height {
            return Err(Error::MismatchedDimensions);
        }
        Ok(Self { front, back })
    }

    pub fn draw(&mut self) -> &mut Canvas<'back> {
        &mut self.back
    }

    /// Copies the full visible frame from the drawing buffer.
    pub fn present(&mut self) {
        self.present_region(Rect {
            origin: Point { x: 0, y: 0 },
            width: self.back.layout.width,
            height: self.back.layout.height,
        });
    }

    /// Copies the visible part of a changed region at the same screen
    /// coordinates, preserving the front buffer outside it.
    pub fn present_region(&mut self, rect: Rect) {
        self.front.blit(self.back.as_surface(), rect, rect.origin);
    }

    /// Presents the recorded regions and clears them after a successful copy.
    /// Draw the entire contents of each region in the back buffer first.
    pub fn present_damage(&mut self, damage: &mut Damage) -> Result<(), Error> {
        if damage.dimensions() != (self.back.layout.width, self.back.layout.height) {
            return Err(Error::MismatchedDimensions);
        }
        for &region in damage.regions() {
            self.present_region(region);
        }
        damage.clear();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Color;

    const DRAWN: Color = Color::rgb(10, 20, 30);
    const BLACK: Color = Color::rgba(0, 0, 0, 0);
    const ORIGIN: Point = Point { x: 0, y: 0 };

    #[test]
    fn rejects_different_visible_dimensions() {
        let mut front = [0; 16];
        let mut back = [0; 16];
        let front = Canvas::new(&mut front, 2, 2, 8).unwrap();
        let back = Canvas::new(&mut back, 1, 2, 8).unwrap();
        assert!(matches!(
            DoubleBuffer::new(front, back),
            Err(Error::MismatchedDimensions)
        ));
    }

    #[test]
    fn drawing_changes_the_front_only_when_presented() {
        let mut front = [0; 16];
        let mut back = [0; 16];
        let front = Canvas::new(&mut front, 2, 2, 8).unwrap();
        let back = Canvas::new(&mut back, 2, 2, 8).unwrap();
        let mut frame = DoubleBuffer::new(front, back).unwrap();
        frame.draw().put_pixel(ORIGIN, DRAWN);
        assert_eq!(frame.front.as_surface().pixel(ORIGIN), Some(BLACK));
        frame.present();
        assert_eq!(frame.front.as_surface().pixel(ORIGIN), Some(DRAWN));
        frame.draw().put_pixel(ORIGIN, BLACK);
        assert_eq!(frame.front.as_surface().pixel(ORIGIN), Some(DRAWN));
        frame.present();
        assert_eq!(frame.front.as_surface().pixel(ORIGIN), Some(BLACK));
    }

    #[test]
    fn presents_only_a_clipped_region_and_preserves_padding() {
        let mut front = [0xcc; 26];
        let mut back = [0x55; 18];
        let target = Canvas::new(&mut front, 2, 2, 13).unwrap();
        let draw = Canvas::new(&mut back, 2, 2, 9).unwrap();
        let mut frame = DoubleBuffer::new(target, draw).unwrap();
        frame.draw().fill_rect(
            Rect {
                origin: ORIGIN,
                width: 2,
                height: 2,
            },
            DRAWN,
        );
        frame.present_region(Rect {
            origin: Point { x: -2, y: 1 },
            width: 3,
            height: u32::MAX,
        });
        assert_eq!(&front[..13], &[0xcc; 13]);
        assert_eq!(&front[13..17], &DRAWN.to_bgra());
        assert_eq!(&front[17..], &[0xcc; 9]);
        assert_eq!(back[8], 0x55);
        assert_eq!(back[17], 0x55);
    }
}
