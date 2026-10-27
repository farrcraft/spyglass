//! The `Mth` helpers noise uses, with the game's `float` operation order.

/// `Mth.floor(double)`: `(int) Math.floor(d)`, saturating, with NaN as 0 (as Rust's `as` does).
pub fn floor(d: f64) -> i32 {
    d.floor() as i32
}

/// `Mth.smoothstep(float)`: `t³(t(6t − 15) + 10)`.
pub fn smoothstep(t: f32) -> f32 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// `Mth.lerp(float, float, float)`: `a + t(b − a)`.
pub fn lerp(t: f32, a: f32, b: f32) -> f32 {
    a + t * (b - a)
}

/// `Mth.lerp2(float, ...)`.
pub fn lerp2(tx: f32, ty: f32, x0y0: f32, x1y0: f32, x0y1: f32, x1y1: f32) -> f32 {
    lerp(ty, lerp(tx, x0y0, x1y0), lerp(tx, x0y1, x1y1))
}

/// `Mth.lerp3(float, ...)`: corners in the game's order, x fastest, then y, then z.
#[allow(clippy::too_many_arguments, reason = "mirrors Mth.lerp3's signature")]
pub fn lerp3(
    tx: f32,
    ty: f32,
    tz: f32,
    v000: f32,
    v100: f32,
    v010: f32,
    v110: f32,
    v001: f32,
    v101: f32,
    v011: f32,
    v111: f32,
) -> f32 {
    lerp(
        tz,
        lerp2(tx, ty, v000, v100, v010, v110),
        lerp2(tx, ty, v001, v101, v011, v111),
    )
}

/// `Mth.lfloor(double)`: `(long) Math.floor(d)`, saturating, with NaN as 0 (as Rust's `as` does).
pub fn lfloor(d: f64) -> i64 {
    d.floor() as i64
}

/// `Mth.smoothstep(double)`, the `double` overload 26.2's noise uses:
/// `((t·t)·t) · ((t·((t·6) − 15)) + 10)`.
pub fn smoothstep_f64(t: f64) -> f64 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// `Mth.lerp(double, double, double)`: `a + t(b − a)`.
pub fn lerp_f64(t: f64, a: f64, b: f64) -> f64 {
    a + t * (b - a)
}

/// `Mth.lerp2(double, ...)`.
pub fn lerp2_f64(tx: f64, ty: f64, x0y0: f64, x1y0: f64, x0y1: f64, x1y1: f64) -> f64 {
    lerp_f64(ty, lerp_f64(tx, x0y0, x1y0), lerp_f64(tx, x0y1, x1y1))
}

/// `Mth.lerp3(double, ...)`: corners in the game's order, x fastest, then y, then z.
#[allow(clippy::too_many_arguments, reason = "mirrors Mth.lerp3's signature")]
pub fn lerp3_f64(
    tx: f64,
    ty: f64,
    tz: f64,
    v000: f64,
    v100: f64,
    v010: f64,
    v110: f64,
    v001: f64,
    v101: f64,
    v011: f64,
    v111: f64,
) -> f64 {
    lerp_f64(
        tz,
        lerp2_f64(tx, ty, v000, v100, v010, v110),
        lerp2_f64(tx, ty, v001, v101, v011, v111),
    )
}

/// `Mth.clampedLerp(double, double, double)`: `a` if `t < 0`, `b` if `t > 1`, else
/// [`lerp_f64`] (NaN takes the lerp).
pub fn clamped_lerp(t: f64, a: f64, b: f64) -> f64 {
    if t < 0.0 {
        a
    } else if t > 1.0 {
        b
    } else {
        lerp_f64(t, a, b)
    }
}

/// `Math.pow(2.0, n)` for any `int` `n`: exact wherever `2^n` is a `f64` (subnormals included),
/// `+∞` above, and `+0.0` below. Unlike [`exp2i`] it never panics, for 26.2's noise parameters,
/// which have no range checks. At `n = −1075`, exactly halfway to the smallest subnormal, it
/// rounds to even (0); Java's `pow` only promises 1 ulp there, and no data comes near.
pub fn pow2(n: i32) -> f64 {
    match n {
        1024.. => f64::INFINITY,
        -1022..=1023 => f64::from_bits(((n + 1023) as u64) << 52),
        -1074..=-1023 => f64::from_bits(1_u64 << (n + 1074)),
        _ => 0.0,
    }
}

/// `2^n` exactly, for the integer powers the game computes with `Math.pow(2.0, n)` and
/// `Math.pow(0.5, n)`. Built from the exponent bits, so no `pow` implementation is involved.
///
/// # Panics
///
/// If `2^n` isn't a normal `f64` (`n` outside `-1022..=1023`).
pub fn exp2i(n: i32) -> f64 {
    assert!((-1022..=1023).contains(&n), "2^{n} isn't a normal f64");
    f64::from_bits(((n + 1023) as u64) << 52)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floor_saturates_like_java() {
        assert_eq!(floor(-0.5), -1);
        assert_eq!(floor(1e20), i32::MAX);
        assert_eq!(floor(-1e20), i32::MIN);
        assert_eq!(floor(f64::NAN), 0);
    }

    #[test]
    fn exp2i_is_exact() {
        assert_eq!(exp2i(0), 1.0);
        assert_eq!(exp2i(-10), 1.0 / 1024.0);
        assert_eq!(exp2i(31), 2_147_483_648.0);
    }

    #[test]
    fn lfloor_saturates_like_java() {
        assert_eq!(lfloor(-0.5), -1);
        assert_eq!(lfloor(1e300), i64::MAX);
        assert_eq!(lfloor(f64::NEG_INFINITY), i64::MIN);
        assert_eq!(lfloor(f64::NAN), 0);
    }

    #[test]
    fn pow2_covers_every_int() {
        for n in -1022..=1023 {
            assert_eq!(pow2(n), exp2i(n));
        }
        assert_eq!(pow2(-1074), f64::from_bits(1));
        assert_eq!(pow2(-1023), f64::MIN_POSITIVE / 2.0);
        assert_eq!(pow2(-1075), 0.0);
        assert_eq!(pow2(i32::MIN), 0.0);
        assert_eq!(pow2(1024), f64::INFINITY);
        assert_eq!(pow2(i32::MAX), f64::INFINITY);
    }

    #[test]
    fn f64_helpers() {
        assert_eq!(smoothstep_f64(0.5), 0.5);
        assert_eq!(smoothstep_f64(1.0), 1.0);
        // 0.25³ · (0.25 · (1.5 − 15) + 10) = 0.015625 · 6.625.
        assert_eq!(smoothstep_f64(0.25), 0.103_515_625);
        assert_eq!(lerp_f64(0.25, 2.0, 6.0), 3.0);
        assert_eq!(clamped_lerp(-0.1, 2.0, 6.0), 2.0);
        assert_eq!(clamped_lerp(1.5, 2.0, 6.0), 6.0);
        assert_eq!(clamped_lerp(0.5, 2.0, 6.0), 4.0);
        assert!(clamped_lerp(f64::NAN, 2.0, 6.0).is_nan());
        // x fastest, then y, then z: only v101 is 1, at (1, 0, 1).
        let v = lerp3_f64(0.5, 0.25, 0.75, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0);
        assert_eq!(v, 0.5 * 0.75 * 0.75);
    }

    #[test]
    fn lerp_endpoints() {
        assert_eq!(lerp(0.0, 3.0, 5.0), 3.0);
        assert_eq!(lerp(1.0, 3.0, 5.0), 5.0);
        assert_eq!(smoothstep(0.0), 0.0);
        assert_eq!(smoothstep(1.0), 1.0);
    }
}
