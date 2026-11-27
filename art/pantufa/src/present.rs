//! Turning a palette-indexed scene into framebuffer pixels.

use crate::canvas::Canvas;
use crate::palette::{bgra, idx::NIGHT};

const BYTES_PER_PIXEL: usize = 4;

/// A BGRA8888 destination: `pitch` is the distance in bytes between the
/// starts of two rows and may include padding (preserved untouched).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Target {
    pub width: usize,
    pub height: usize,
    pub pitch: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PresentError {
    /// Zero size, a pitch shorter than a row, or a buffer too short for the
    /// described rows.
    BadTarget,
    /// The scene does not fit even at scale 1.
    TooSmall,
}

/// Copies `scene` into `dst` scaled by the largest integer factor that fits,
/// centred, with the borders in the night colour. `quarters` is the
/// brightness (0..=4) for fades. Returns the scale used.
pub fn present_bgra(
    scene: &Canvas<'_>,
    quarters: u8,
    dst: &mut [u8],
    target: Target,
) -> Result<usize, PresentError> {
    let row_bytes = target
        .width
        .checked_mul(BYTES_PER_PIXEL)
        .ok_or(PresentError::BadTarget)?;
    if target.width == 0 || target.height == 0 || target.pitch < row_bytes {
        return Err(PresentError::BadTarget);
    }
    let needed = target
        .pitch
        .checked_mul(target.height - 1)
        .and_then(|n| n.checked_add(row_bytes))
        .ok_or(PresentError::BadTarget)?;
    if dst.len() < needed {
        return Err(PresentError::BadTarget);
    }
    let (sw, sh) = (scene.width(), scene.height());
    let scale = (target.width / sw).min(target.height / sh);
    if scale == 0 {
        return Err(PresentError::TooSmall);
    }
    let (ox, oy) = (
        (target.width - sw * scale) / 2,
        (target.height - sh * scale) / 2,
    );
    let mut lut = [[0u8; BYTES_PER_PIXEL]; 16];
    for (i, entry) in lut.iter_mut().enumerate() {
        *entry = bgra(i as u8, quarters);
    }
    let border = bgra(NIGHT, quarters);
    let colour = |index: u8| lut.get(index as usize).copied().unwrap_or(border);

    for y in 0..target.height {
        let row = &mut dst[y * target.pitch..y * target.pitch + row_bytes];
        let inside_y = y >= oy && y < oy + sh * scale;
        if !inside_y {
            for px in row.as_chunks_mut::<BYTES_PER_PIXEL>().0 {
                px.copy_from_slice(&border);
            }
            continue;
        }
        let src = &scene.pixels()[((y - oy) / scale) * sw..][..sw];
        let (left, rest) = row.split_at_mut(ox * BYTES_PER_PIXEL);
        let (middle, right) = rest.split_at_mut(sw * scale * BYTES_PER_PIXEL);
        for px in left
            .as_chunks_mut::<BYTES_PER_PIXEL>()
            .0
            .iter_mut()
            .chain(right.as_chunks_mut::<BYTES_PER_PIXEL>().0.iter_mut())
        {
            px.copy_from_slice(&border);
        }
        for (block, &index) in middle.chunks_exact_mut(scale * BYTES_PER_PIXEL).zip(src) {
            let c = colour(index);
            for px in block.as_chunks_mut::<BYTES_PER_PIXEL>().0 {
                px.copy_from_slice(&c);
            }
        }
    }
    Ok(scale)
}
