//! Java library functions whose edge cases differ from Rust's.

/// `Math.min(float, float)`: NaN if either is NaN, and `-0.0` below `+0.0` (Rust's `f32::min`
/// returns the other operand for NaN).
pub(crate) fn min(a: f32, b: f32) -> f32 {
    if a.is_nan() {
        a
    } else if a == 0.0 && b == 0.0 && b.is_sign_negative() {
        b
    } else if a <= b {
        a
    } else {
        b
    }
}

/// `Math.max(float, float)`: NaN if either is NaN, and `+0.0` above `-0.0`.
pub(crate) fn max(a: f32, b: f32) -> f32 {
    if a.is_nan() {
        a
    } else if a == 0.0 && b == 0.0 && a.is_sign_negative() {
        b
    } else if a >= b {
        a
    } else {
        b
    }
}

/// `Math.min(double, double)`: NaN if either is NaN, and `-0.0` below `+0.0`.
pub(crate) fn min_f64(a: f64, b: f64) -> f64 {
    if a.is_nan() {
        a
    } else if a == 0.0 && b == 0.0 && b.is_sign_negative() {
        b
    } else if a <= b {
        a
    } else {
        b
    }
}

/// `Math.max(double, double)`: NaN if either is NaN, and `+0.0` above `-0.0`.
pub(crate) fn max_f64(a: f64, b: f64) -> f64 {
    if a.is_nan() {
        a
    } else if a == 0.0 && b == 0.0 && a.is_sign_negative() {
        b
    } else if a >= b {
        a
    } else {
        b
    }
}

/// `Mth.clamp(double, double, double)`: `min` if `value < min`, else `Math.min(value, max)`,
/// so a NaN value stays NaN and `−0` clamped below by `+0` stays `−0`.
pub(crate) fn mth_clamp_f64(value: f64, min: f64, max: f64) -> f64 {
    if value < min {
        min
    } else {
        min_f64(value, max)
    }
}

/// `Math.clamp(float, float, float)`: `Math.min(max, Math.max(value, min))`, so `−0` clamped
/// below by `+0` is `+0`, and NaN stays NaN. (The game's bounds are ordered and never NaN, which
/// `Math.clamp` would throw on.)
pub(crate) fn clamp(value: f32, min: f32, max: f32) -> f32 {
    self::min(max, self::max(value, min))
}

/// `Math.floorDiv(int, int)`: rounds toward negative infinity, wrapping `MIN_VALUE / -1`.
pub(crate) fn floor_div(x: i32, y: i32) -> i32 {
    let q = x.wrapping_div(y);
    if (x ^ y) < 0 && q.wrapping_mul(y) != x {
        q - 1
    } else {
        q
    }
}

/// `Math.floorMod(int, int)`: `x − floorDiv(x, y)·y`, with the sign of `y`.
pub(crate) fn floor_mod(x: i32, y: i32) -> i32 {
    x.wrapping_sub(floor_div(x, y).wrapping_mul(y))
}

/// `Math.round(float)`: the nearest int, halves rounded up, saturating, NaN as 0.
pub(crate) fn round(x: f32) -> i32 {
    // x + 0.5 is exact in f64 wherever x has a fraction, and floor then rounds halves up.
    (f64::from(x) + 0.5).floor() as i32
}

/// `Math.signum(float)`: ±1, or the argument itself for zeros and NaN.
pub(crate) fn signum(x: f32) -> f32 {
    if x == 0.0 || x.is_nan() {
        x
    } else {
        1.0_f32.copysign(x)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn min_and_max_propagate_nan_and_order_zeros() {
        assert!(min(f32::NAN, 1.0).is_nan());
        assert!(min(1.0, f32::NAN).is_nan());
        assert!(max(1.0, f32::NAN).is_nan());
        assert!(min(0.0, -0.0).is_sign_negative());
        assert!(min(-0.0, 0.0).is_sign_negative());
        assert!(max(-0.0, 0.0).is_sign_positive());
        assert!(max(0.0, -0.0).is_sign_positive());
        assert_eq!(min(2.0, -3.0), -3.0);
        assert_eq!(max(2.0, -3.0), 2.0);
        assert!(min_f64(f64::NAN, 1.0).is_nan());
        assert!(min_f64(1.0, f64::NAN).is_nan());
        assert!(max_f64(1.0, f64::NAN).is_nan());
        assert!(min_f64(0.0, -0.0).is_sign_negative());
        assert!(max_f64(-0.0, 0.0).is_sign_positive());
        assert_eq!(
            mth_clamp_f64(-0.0, 0.0, 1.0).to_bits(),
            (-0.0_f64).to_bits()
        );
        assert!(mth_clamp_f64(f64::NAN, -1.0, 1.0).is_nan());
    }

    #[test]
    fn math_clamp_lifts_negative_zero() {
        // Math.min(max, Math.max(−0, +0)) is +0, where Mth.clamp's `value < min` test keeps −0.
        assert_eq!(clamp(-0.0, 0.0, 1.0).to_bits(), 0);
        assert_eq!(clamp(-0.0, -1.0, 1.0).to_bits(), (-0.0_f32).to_bits());
        assert!(clamp(f32::NAN, 0.0, 1.0).is_nan());
        assert_eq!(clamp(5.0, 0.0, 1.0), 1.0);
        assert_eq!(clamp(-5.0, 0.0, 1.0), 0.0);
    }

    #[test]
    fn floor_division_follows_the_divisor() {
        assert_eq!(floor_div(7, 2), 3);
        assert_eq!(floor_div(-7, 2), -4);
        assert_eq!(floor_div(7, -2), -4);
        assert_eq!(floor_div(-7, -2), 3);
        assert_eq!(floor_div(i32::MIN, -1), i32::MIN);
        assert_eq!(floor_mod(-7, 2), 1);
        assert_eq!(floor_mod(7, -2), -1);
        assert_eq!(floor_mod(-8, 4), 0);
    }

    #[test]
    fn round_rounds_halves_up() {
        assert_eq!(round(2.5), 3);
        assert_eq!(round(-2.5), -2);
        assert_eq!(round(0.499_999_97), 0);
        assert_eq!(round(-0.5), 0);
        assert_eq!(round(f32::NAN), 0);
        assert_eq!(round(1e20), i32::MAX);
        assert_eq!(round(-1e20), i32::MIN);
    }

    #[test]
    fn signum_keeps_zeros_and_nan() {
        assert_eq!(signum(-3.0), -1.0);
        assert_eq!(signum(0.25), 1.0);
        assert!(signum(-0.0).is_sign_negative());
        assert!(signum(f32::NAN).is_nan());
    }
}
