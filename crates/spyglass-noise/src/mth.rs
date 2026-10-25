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
    fn lerp_endpoints() {
        assert_eq!(lerp(0.0, 3.0, 5.0), 3.0);
        assert_eq!(lerp(1.0, 3.0, 5.0), 5.0);
        assert_eq!(smoothstep(0.0), 0.0);
        assert_eq!(smoothstep(1.0), 1.0);
    }
}
