//! The few float helpers the art needs. `core` has no `floor`, `sqrt` or
//! `sin` for `f32`, and the bare-metal target uses soft-float, so these stay
//! small and only run on sprite-sized data.

use core::f32::consts::{FRAC_PI_2, PI, TAU};

/// Coordinates are clamped to ±2^30 before converting, so `floor`, `ceil` and
/// `round` can never reach `i32::MIN`/`MAX` and the callers' arithmetic on
/// the result cannot overflow. NaN becomes 0. Far beyond any canvas anyway.
const LIMIT: f32 = (1 << 30) as f32;

pub(crate) fn floor(x: f32) -> i32 {
    if x.is_nan() {
        return 0;
    }
    let x = x.clamp(-LIMIT, LIMIT);
    let i = x as i32;
    if (i as f32) > x { i - 1 } else { i }
}

pub(crate) fn ceil(x: f32) -> i32 {
    -floor(-x)
}

/// Rounds half up, like JavaScript's `Math.round` (the prototype's rule).
pub(crate) fn round(x: f32) -> i32 {
    floor(x + 0.5)
}

pub(crate) fn abs(x: f32) -> f32 {
    if x < 0.0 { -x } else { x }
}

pub(crate) fn lerp(a: f32, b: f32, u: f32) -> f32 {
    a + (b - a) * u
}

/// Square root by Newton's method from a bit-level first guess. Exact enough
/// for distances in pixels; returns 0 for non-positive input.
pub(crate) fn sqrt(x: f32) -> f32 {
    if x <= 0.0 || x.is_nan() {
        return 0.0;
    }
    let mut g = f32::from_bits((x.to_bits() >> 1) + 0x1FBD_1DF5);
    for _ in 0..3 {
        g = 0.5 * (g + x / g);
    }
    g
}

/// Sine with an error below 0.001: parabolic approximation plus one
/// correction step, after reducing the angle to `[-π, π]`.
pub(crate) fn sin(x: f32) -> f32 {
    if x.is_nan() {
        return 0.0;
    }
    let mut x = x % TAU;
    if x > PI {
        x -= TAU;
    } else if x < -PI {
        x += TAU;
    }
    let y = (4.0 / PI) * x - (4.0 / (PI * PI)) * x * abs(x);
    0.225 * (y * abs(y) - y) + y
}

pub(crate) fn sin_cos(angle: f32) -> (f32, f32) {
    (sin(angle), sin(angle + FRAC_PI_2))
}
