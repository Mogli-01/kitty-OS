//! Source-over compositing for straight-alpha BGRA8888 pixels.

use crate::{Canvas, PIXEL_BYTES, Point, Rect, Surface};

impl Canvas<'_> {
    /// Composites a clipped source rectangle over the destination using
    /// straight alpha. Source and destination coordinates stay aligned.
    /// Blends the stored channel values with integer rounding.
    pub fn blend(&mut self, source: Surface<'_>, rect: Rect, at: Point) {
        self.transfer_rows(source, rect, at, |target, source| {
            let (target, _) = target.as_chunks_mut::<PIXEL_BYTES>();
            let (source, _) = source.as_chunks::<PIXEL_BYTES>();
            for (target, &source) in target.iter_mut().zip(source) {
                source_over(target, source);
            }
        });
    }
}

fn source_over(target: &mut [u8; PIXEL_BYTES], source: [u8; PIXEL_BYTES]) {
    let alpha = u32::from(source[3]);
    if alpha == 0 {
        return;
    }
    if alpha == 255 || target[3] == 0 {
        *target = source;
        return;
    }

    // Keep the unrounded output alpha as a denominator for the straight
    // channels. The weights sum to at most 255 * 255, so all products fit.
    let source_weight = alpha * 255;
    let target_weight = u32::from(target[3]) * (255 - alpha);
    let total = source_weight + target_weight;
    for (target, source) in target[..3].iter_mut().zip(&source[..3]) {
        let channel = u32::from(*source) * source_weight + u32::from(*target) * target_weight;
        *target = ((channel + total / 2) / total) as u8;
    }
    target[3] = ((total + 127) / 255) as u8;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Color;

    #[test]
    fn preserves_transparent_source_and_copies_opaque_source() {
        let mut pixel = [10, 20, 30, 40];
        source_over(&mut pixel, [255, 255, 255, 0]);
        assert_eq!(pixel, [10, 20, 30, 40]);
        source_over(&mut pixel, [1, 2, 3, 255]);
        assert_eq!(pixel, [1, 2, 3, 255]);
        pixel = [255, 255, 255, 0];
        source_over(&mut pixel, [1, 2, 3, 1]);
        assert_eq!(pixel, [1, 2, 3, 1]);
    }

    #[test]
    fn weights_color_by_both_alphas_before_rounding() {
        let mut pixel = Color::rgb(0, 0, 100).to_bgra();
        source_over(&mut pixel, Color::rgba(200, 0, 0, 128).to_bgra());
        assert_eq!(pixel, Color::rgb(100, 0, 50).to_bgra());
        let mut pixel = Color::rgba(0, 0, 255, 128).to_bgra();
        source_over(&mut pixel, Color::rgba(255, 0, 0, 128).to_bgra());
        assert_eq!(pixel, Color::rgba(170, 0, 85, 192).to_bgra());
    }

    #[test]
    fn clips_source_and_destination_and_keeps_padding() {
        let source = [
            10, 20, 30, 128, 5, 6, 7, 255, 0xaa, 0xbb, 35, 45, 55, 255, 1, 2, 3, 0,
        ];
        let source = Surface::new(&source, 2, 2, 10).unwrap();
        let mut target = [0xcd; 22];
        let mut canvas = Canvas::new(&mut target, 2, 2, 13).unwrap();
        canvas.blend(
            source,
            Rect {
                origin: Point { x: 0, y: -1 },
                width: u32::MAX,
                height: u32::MAX,
            },
            Point { x: -1, y: 0 },
        );
        assert_eq!(&target[13..17], &[5, 6, 7, 255]);
        assert_eq!(&target[..13], &[0xcd; 13]);
        assert_eq!(&target[17..], &[0xcd; 5]);
    }
}
