//! `log` and `pow`, correctly rounded.
//!
//! `ln x` splits `x = m·2^e` with `m` in [√½, √2), so `ln x = e·ln 2 + 2·atanh z` with
//! `z = (m − 1)/(m + 1)`, at most 0.172, whose odd series converges fast. `pow` is
//! `exp(y·ln x)` with both in double-double arithmetic, `exp` reduced by ln 2 to a short Taylor
//! series and scaled by a power of two; Java's special cases come first.

use std::sync::OnceLock;

use crate::dd::{Dd, two_prod, two_sum};
use crate::trig::{inverse_factorials, rounded};

/// ln 2 in three parts (from the QD library's constant).
const LN_2: [f64; 3] = [
    std::f64::consts::LN_2,
    2.319_046_813_846_299_6e-17,
    5.707_708_438_416_212e-34,
];

/// `pow`'s error grows with |y·ln x|: about 2^-94.5 of the result at the extremes, where
/// |y·ln x| is near 745. Its rounding test allows for that.
const POW_BOUND: f64 = 3.231_174_267_785_264_4e-27; // 2^-88

/// Terms of the atanh series: the last is below 2^-110 of the result for |z| ≤ 0.172.
const ATANH_TERMS: usize = 23;

/// Terms of the exp series: the last is below 2^-115 for |s| ≤ ln 2 / 2.
const EXP_TERMS: usize = 25;

/// `1/(2n + 1)` for the atanh series, as double-doubles.
fn odd_reciprocals() -> &'static [Dd; ATANH_TERMS] {
    static TABLE: OnceLock<[Dd; ATANH_TERMS]> = OnceLock::new();
    TABLE.get_or_init(|| std::array::from_fn(|n| Dd::new(1.0).div(Dd::new((2 * n + 1) as f64))))
}

/// `e·ln 2` for an integer `e`.
fn ln_2_times(e: f64) -> Dd {
    two_prod(e, LN_2[0])
        .add(two_prod(e, LN_2[1]))
        .add(Dd::new(e * LN_2[2]))
}

/// `ln x` for positive finite `x`, as a double-double.
fn ln(x: f64) -> Dd {
    let (x, scaled) = if x < f64::MIN_POSITIVE {
        (x * 18_014_398_509_481_984.0, 54) // 2^54, to normalize a subnormal
    } else {
        (x, 0)
    };
    let bits = x.to_bits();
    let mut e = ((bits >> 52) & 0x7ff) as i32 - 1023 - scaled;
    let mut m = f64::from_bits((bits & ((1 << 52) - 1)) | (1023 << 52));
    if m > std::f64::consts::SQRT_2 {
        m /= 2.0;
        e += 1;
    }
    // m − 1 is exact for m in [√½, √2]; m + 1 may not be, so it's kept as a sum.
    let z = Dd::new(m - 1.0).div(two_sum(m, 1.0));
    let z2 = z.mul(z);
    let c = odd_reciprocals();
    let mut acc = c[ATANH_TERMS - 1];
    for n in (0..ATANH_TERMS - 1).rev() {
        acc = acc.mul(z2).add(c[n]);
    }
    ln_2_times(f64::from(e)).add(acc.mul(z).mul_f(2.0))
}

/// `exp t` as a double-double near 1 and a power of two: `exp t = value · 2^k`.
fn exp(t: Dd) -> (Dd, i32) {
    let k = (t.hi / LN_2[0]).round();
    let s = t.sub(ln_2_times(k));
    let f = inverse_factorials();
    let mut acc = f[EXP_TERMS - 1];
    for n in (0..EXP_TERMS - 1).rev() {
        acc = acc.mul(s).add(f[n]);
    }
    (acc, k as i32)
}

/// `2^k` for a normal power of two.
fn power_of_two(k: i32) -> f64 {
    f64::from_bits(((k + 1023) as u64) << 52)
}

/// `Math.log`, and whether the rounding test settled it.
pub(crate) fn log(x: f64) -> (f64, bool) {
    if x.is_nan() || x < 0.0 {
        return (f64::NAN, true);
    }
    if x == 0.0 {
        return (f64::NEG_INFINITY, true);
    }
    if x == f64::INFINITY {
        return (x, true);
    }
    if x == 1.0 {
        return (0.0, true);
    }
    rounded(ln(x))
}

/// Whether `y` is an odd integer.
fn odd(y: f64) -> bool {
    y == y.trunc() && y.abs() < 9_007_199_254_740_992.0 && (y % 2.0).abs() == 1.0
}

/// `Math.pow`, with Java's special cases, and whether the rounding test settled it.
pub(crate) fn pow(x: f64, y: f64) -> (f64, bool) {
    if y == 0.0 {
        return (1.0, true);
    }
    if y == 1.0 {
        return (x, true);
    }
    if x.is_nan() || y.is_nan() {
        return (f64::NAN, true);
    }
    let ax = x.abs();
    if y.is_infinite() {
        return if ax == 1.0 {
            (f64::NAN, true)
        } else if (ax > 1.0) == (y > 0.0) {
            (f64::INFINITY, true)
        } else {
            (0.0, true)
        };
    }
    if x == 0.0 || x.is_infinite() {
        // A zero result for a zero base raised to a positive power, or an infinite base to a
        // negative one; an infinite result otherwise. A negative base keeps its sign only for
        // odd integer powers.
        let magnitude = if (x == 0.0) == (y > 0.0) {
            0.0
        } else {
            f64::INFINITY
        };
        let negative = x.is_sign_negative() && odd(y);
        return (if negative { -magnitude } else { magnitude }, true);
    }
    if x < 0.0 && y != y.trunc() {
        return (f64::NAN, true);
    }
    let (value, settled) = pow_positive(ax, y);
    (if x < 0.0 && odd(y) { -value } else { value }, settled)
}

/// `x^y` for positive finite `x` and finite non-zero `y`.
fn pow_positive(x: f64, y: f64) -> (f64, bool) {
    if x == 1.0 {
        return (1.0, true);
    }
    let t = ln(x).mul_f(y);
    // Past ln(MAX) with room to spare, or below half the least subnormal.
    if t.hi > 710.0 {
        return (f64::INFINITY, true);
    }
    if t.hi < -745.2 {
        return (0.0, true);
    }
    let (value, k) = exp(t);
    let (near_one, settled) = match value.round(POW_BOUND) {
        Some(v) => (v, true),
        None => (value.hi, false),
    };
    if k > 1023 {
        // Only up to 2^1024 · 1.42: scaled in two steps, so the last rounds as IEEE overflow does.
        return (near_one * power_of_two(k - 1) * 2.0, settled);
    }
    if k < -1021 {
        // A subnormal result is rounded twice here, once near 1 and once when scaled: off by
        // an ulp at most, and only below 2^-1021, which nothing in worldgen reaches.
        return (near_one * power_of_two(k + 600) * power_of_two(-600), false);
    }
    (near_one * power_of_two(k), settled)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn javas_special_cases() {
        assert_eq!(pow(f64::NAN, 0.0), (1.0, true));
        assert!(pow(f64::NAN, 1.0).0.is_nan());
        assert!(pow(-2.0, 0.5).0.is_nan());
        assert!(pow(1.0, f64::INFINITY).0.is_nan());
        assert_eq!(pow(0.5, f64::INFINITY).0, 0.0);
        assert_eq!(pow(2.0, f64::INFINITY).0, f64::INFINITY);
        assert_eq!(pow(-0.0, 3.0).0.to_bits(), (-0.0f64).to_bits());
        assert_eq!(pow(-0.0, -3.0).0, f64::NEG_INFINITY);
        assert_eq!(pow(-0.0, 2.0).0.to_bits(), 0.0f64.to_bits());
        assert_eq!(pow(f64::NEG_INFINITY, 3.0).0, f64::NEG_INFINITY);
        assert_eq!(pow(f64::NEG_INFINITY, -2.0).0.to_bits(), 0.0f64.to_bits());
        assert_eq!(pow(-2.0, 3.0), (-8.0, true));
        assert_eq!(pow(2.0, 2000.0).0, f64::INFINITY);
        assert_eq!(pow(2.0, -2000.0).0, 0.0);
        assert!(log(-1.0).0.is_nan());
        assert_eq!(log(0.0).0, f64::NEG_INFINITY);
        assert_eq!(log(1.0), (0.0, true));
    }

    #[test]
    fn exact_powers_stay_exact() {
        for n in -1021..=1023 {
            assert_eq!(pow(2.0, f64::from(n)).0, power_of_two(n), "2^{n}");
        }
        assert_eq!(pow(4.0, 0.5).0, 2.0);
        assert_eq!(pow(10.0, 3.0).0, 1000.0);
    }
}
