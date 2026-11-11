//! `sin` and `cos`, correctly rounded for |x| < 2^20.
//!
//! The argument is reduced by π/2, held as three doubles (about 160 bits), so that
//! `x = k·π/2 + r` with |r| ≤ π/4 and `r` carried as a double-double; then `sin r` or `cos r`,
//! chosen and signed by `k mod 4`, is summed as a Taylor series in double-double arithmetic.
//! The result is accurate to about 2^-100 of its value, and rounded once.

use std::sync::OnceLock;

use crate::dd::{Dd, two_prod};

/// π/2 in three parts, each the double nearest what's left (from the QD library's constant).
const PI_2: [f64; 3] = [
    std::f64::consts::FRAC_PI_2,
    6.123_233_995_736_766e-17,
    -1.497_384_904_859_169_8e-33,
];

/// 2/π, to pick `k`; its error only moves `r` a little past π/4, which the series covers.
const FRAC_2_PI: f64 = std::f64::consts::FRAC_2_PI;

/// Arguments from here on are left to the `libm` crate (fdlibm's algorithm): the three-part π/2
/// stops being enough. No worldgen call comes near.
const LIMIT: f64 = 1_048_576.0; // 2^20

/// Below this, `sin x` rounds to `x`: x³/6 is under half an ulp of `x`.
const SIN_TINY: f64 = 1.0 / 67_108_864.0; // 2^-26

/// Below this, `cos x` rounds to 1: x²/2 is under half the gap below 1 (2^-54). From about
/// 2^-26.5 it isn't, and `cos x` rounds to the double below 1.
const COS_TINY: f64 = 1.0 / 134_217_728.0; // 2^-27

/// The relative error bound the rounding test allows for: generous for a series accurate to
/// about 2^-100.
pub(crate) const BOUND: f64 = 2.524_354_896_707_238e-29; // 2^-95

/// Terms of each series: the last is below 2^-110 of the result for |r| ≤ π/4 (plus slack).
const TERMS: usize = 15;

/// `1/n!` for n up to `2·TERMS + 1`, as double-doubles.
pub(crate) fn inverse_factorials() -> &'static [Dd; 2 * TERMS + 2] {
    static TABLE: OnceLock<[Dd; 2 * TERMS + 2]> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut table = [Dd::new(1.0); 2 * TERMS + 2];
        for n in 1..table.len() {
            table[n] = table[n - 1].div(Dd::new(n as f64));
        }
        table
    })
}

/// `x` as `k·π/2 + r`: `r` and `k mod 4`.
fn reduce(x: f64) -> (Dd, u32) {
    if x.abs() <= std::f64::consts::FRAC_PI_4 {
        return (Dd::new(x), 0);
    }
    let k = (x * FRAC_2_PI).round();
    let r = Dd::new(x)
        .sub(two_prod(k, PI_2[0]))
        .sub(two_prod(k, PI_2[1]))
        .sub(Dd::new(k * PI_2[2]));
    // k is below 2^21 in magnitude, so this is exact.
    (r, (k as i64).rem_euclid(4) as u32)
}

/// `sin r = r − r³/3! + r⁵/5! − ...`, by Horner's rule in `r²`.
fn sin_series(r: Dd) -> Dd {
    let f = inverse_factorials();
    let r2 = r.mul(r);
    let term = |n: usize| {
        let c = f[2 * n + 1];
        if n.is_multiple_of(2) { c } else { c.neg() }
    };
    let mut acc = term(TERMS - 1);
    for n in (0..TERMS - 1).rev() {
        acc = acc.mul(r2).add(term(n));
    }
    acc.mul(r)
}

/// `cos r = 1 − r²/2! + r⁴/4! − ...`.
fn cos_series(r: Dd) -> Dd {
    let f = inverse_factorials();
    let r2 = r.mul(r);
    let term = |n: usize| {
        let c = f[2 * n];
        if n.is_multiple_of(2) { c } else { c.neg() }
    };
    let mut acc = term(TERMS - 1);
    for n in (0..TERMS - 1).rev() {
        acc = acc.mul(r2).add(term(n));
    }
    acc
}

/// `sin x`, and whether the rounding test settled it.
pub(crate) fn sin(x: f64) -> (f64, bool) {
    if !x.is_finite() {
        return (f64::NAN, true);
    }
    if x.abs() < SIN_TINY {
        return (x, true);
    }
    if x.abs() >= LIMIT {
        return (libm::sin(x), false);
    }
    let (r, quadrant) = reduce(x);
    let value = match quadrant {
        0 => sin_series(r),
        1 => cos_series(r),
        2 => sin_series(r).neg(),
        _ => cos_series(r).neg(),
    };
    rounded(value)
}

/// `cos x`, and whether the rounding test settled it.
pub(crate) fn cos(x: f64) -> (f64, bool) {
    if !x.is_finite() {
        return (f64::NAN, true);
    }
    if x.abs() < COS_TINY {
        return (1.0, true);
    }
    if x.abs() >= LIMIT {
        return (libm::cos(x), false);
    }
    let (r, quadrant) = reduce(x);
    let value = match quadrant {
        0 => cos_series(r),
        1 => sin_series(r).neg(),
        2 => cos_series(r).neg(),
        _ => sin_series(r),
    };
    rounded(value)
}

/// The correctly rounded double, or, where the test leaves it open, the nearest to `value`.
pub(crate) fn rounded(value: Dd) -> (f64, bool) {
    match value.round(BOUND) {
        Some(v) => (v, true),
        None => (value.hi, false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiny_arguments_round_as_the_series_would() {
        let below_one = f64::from_bits(1.0f64.to_bits() - 1);
        assert_eq!(cos(COS_TINY / 2.0).0, 1.0);
        assert_eq!(cos(COS_TINY).0, 1.0);
        // x²/2 = 2^-53: past the midpoint below 1.
        assert_eq!(cos(SIN_TINY).0, below_one);
        assert_eq!(sin(SIN_TINY / 2.0).0, SIN_TINY / 2.0);
        assert_eq!(sin(-0.0).0.to_bits(), (-0.0f64).to_bits());
    }
}
