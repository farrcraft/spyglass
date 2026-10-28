//! `net.minecraft.util.Interval`: the range of values a density function can take, in `f32`.
//!
//! The game computes ranges only to choose samplers (`min` and `max` compile by them), so they
//! must be computed as the game does, bound for bound. Each function cites the method it ports.
//! Where the game throws, these return [`IntervalError`].

use crate::java;

/// A closed range `[min, max]`, or [`Interval::NAI`] ("not an interval").
#[derive(Debug, Clone, Copy)]
pub struct Interval {
    min: f32,
    max: f32,
}

/// Where the game throws `IllegalArgumentException` or `IllegalStateException`, which would stop
/// it compiling the function.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("{}", self.reason())]
pub enum IntervalError {
    /// `Interval.of` with NaN bounds or `max < min`.
    Bounds,
    /// `Interval.mapMonotonic` produced NaN.
    MonotonicNaN,
}

impl IntervalError {
    /// The message, as a static string.
    pub fn reason(self) -> &'static str {
        match self {
            IntervalError::Bounds => "a density function's range has NaN or reversed bounds",
            IntervalError::MonotonicNaN => "a density function's range maps to NaN",
        }
    }
}

type IntervalResult = Result<Interval, IntervalError>;

impl PartialEq for Interval {
    /// `Interval.equals`: NaI is equal only to itself; bounds compare with `==`.
    fn eq(&self, other: &Self) -> bool {
        (self.is_nai() && other.is_nai()) || (self.min == other.min && self.max == other.max)
    }
}

#[allow(
    clippy::should_implement_trait,
    reason = "the game's static Interval.add and friends; they can fail, so they aren't operators"
)]
impl Interval {
    /// `NaI`.
    pub const NAI: Interval = Interval {
        min: f32::NAN,
        max: f32::NAN,
    };
    /// `INFINITE`.
    pub const INFINITE: Interval = Interval {
        min: f32::NEG_INFINITY,
        max: f32::INFINITY,
    };

    /// `of(min, max)`.
    pub fn of(min: f32, max: f32) -> IntervalResult {
        // The game tests max < min first, then NaN; both throw.
        if min.is_nan() || max.is_nan() || max < min {
            return Err(IntervalError::Bounds);
        }
        Ok(Interval { min, max })
    }

    /// `ofExact(value)`, for a value that isn't NaN.
    pub fn exact(value: f32) -> IntervalResult {
        Interval::of(value, value)
    }

    /// `ofSymmetric(bound)`: `[−bound, bound]`.
    pub fn symmetric(bound: f32) -> IntervalResult {
        Interval::of(-bound, bound)
    }

    /// `min()`; NaN for NaI.
    pub fn min(self) -> f32 {
        self.min
    }

    /// `max()`; NaN for NaI.
    pub fn max(self) -> f32 {
        self.max
    }

    /// `isNaI()`. Only NaI has NaN bounds, since [`Interval::of`] refuses them.
    pub fn is_nai(self) -> bool {
        self.min.is_nan()
    }

    /// `contains(value)`.
    pub fn contains(self, value: f32) -> bool {
        value >= self.min && value <= self.max
    }

    /// `encapsulating(List)`: the smallest interval holding every non-NaI one, or NaI if all are.
    pub fn encapsulating(intervals: impl IntoIterator<Item = Interval>) -> IntervalResult {
        let (mut min, mut max) = (f32::INFINITY, f32::NEG_INFINITY);
        for i in intervals {
            if !i.is_nai() {
                min = java::min(i.min, min);
                max = java::max(i.max, max);
            }
        }
        if max < min {
            return Ok(Interval::NAI);
        }
        Interval::of(min, max)
    }

    /// `encapsulating(float, float)`: both values, ignoring a NaN one.
    pub fn encapsulating_values(a: f32, b: f32) -> IntervalResult {
        match (a.is_nan(), b.is_nan()) {
            (true, true) => Ok(Interval::NAI),
            (true, false) => Interval::exact(b),
            (false, true) => Interval::exact(a),
            (false, false) => Interval::of(java::min(a, b), java::max(a, b)),
        }
    }

    /// `encapsulating(Interval, float)`: `self` widened to hold `value`, ignoring NaN.
    fn encapsulating_with(self, value: f32) -> IntervalResult {
        if value.is_nan() {
            Ok(self)
        } else if self.is_nai() {
            Interval::exact(value)
        } else {
            Interval::of(java::min(self.min, value), java::max(self.max, value))
        }
    }

    /// `add(a, b)`.
    pub fn add(a: Interval, b: Interval) -> IntervalResult {
        let (min, max) = (a.min + b.min, a.max + b.max);
        if min.is_nan() || max.is_nan() {
            return Ok(Interval::NAI);
        }
        Interval::of(min, max)
    }

    /// `sub(a, b)`.
    pub fn sub(a: Interval, b: Interval) -> IntervalResult {
        let (min, max) = (a.min - b.max, a.max - b.min);
        if min.is_nan() || max.is_nan() {
            return Ok(Interval::NAI);
        }
        Interval::of(min, max)
    }

    /// `mul(a, b)`, with `0 · anything = 0` at the bounds.
    pub fn mul(a: Interval, b: Interval) -> IntervalResult {
        if a.is_nai() || b.is_nai() {
            return Ok(Interval::NAI);
        }
        let p = mul_bound(a.min, b.min);
        let q = mul_bound(a.min, b.max);
        let r = mul_bound(a.max, b.min);
        let s = mul_bound(a.max, b.max);
        Interval::of(
            java::min(java::min(p, q), java::min(r, s)),
            java::max(java::max(p, q), java::max(r, s)),
        )
    }

    /// `reciprocal(i)`.
    pub fn reciprocal(i: Interval) -> IntervalResult {
        if i.is_nai() || (i.min == 0.0 && i.max == 0.0) {
            return Ok(Interval::NAI);
        }
        if !i.contains(0.0) {
            Interval::of(1.0 / i.max, 1.0 / i.min)
        } else if i.max == 0.0 {
            Interval::of(f32::NEG_INFINITY, 1.0 / i.min)
        } else if i.min == 0.0 {
            Interval::of(1.0 / i.max, f32::INFINITY)
        } else {
            Ok(Interval::INFINITE)
        }
    }

    /// `div(a, b)`: `a` times the reciprocal of `b`.
    pub fn div(a: Interval, b: Interval) -> IntervalResult {
        Interval::mul(a, Interval::reciprocal(b)?)
    }

    /// `min(a, b)`.
    pub fn min_of(a: Interval, b: Interval) -> IntervalResult {
        if a.is_nai() || b.is_nai() {
            return Ok(Interval::NAI);
        }
        Interval::of(java::min(a.min, b.min), java::min(a.max, b.max))
    }

    /// `max(a, b)`.
    pub fn max_of(a: Interval, b: Interval) -> IntervalResult {
        if a.is_nai() || b.is_nai() {
            return Ok(Interval::NAI);
        }
        Interval::of(java::max(a.min, b.min), java::max(a.max, b.max))
    }

    /// `clamp(i, min, max)`, for `min ≤ max` (the clamp codec guarantees it).
    pub fn clamp(i: Interval, min: f32, max: f32) -> IntervalResult {
        if min > max {
            return Err(IntervalError::Bounds);
        }
        if i.is_nai() {
            Ok(Interval::NAI)
        } else if i.min >= max {
            Interval::of(max, max)
        } else if i.max <= min {
            Interval::of(min, min)
        } else {
            Interval::of(java::max(i.min, min), java::min(i.max, max))
        }
    }

    /// `abs(i)`.
    pub fn abs(i: Interval) -> IntervalResult {
        if i.is_nai() {
            return Ok(Interval::NAI);
        }
        let top = java::max(i.min.abs(), i.max.abs());
        if i.contains(0.0) {
            Interval::of(0.0, top)
        } else {
            Interval::of(java::min(i.min.abs(), i.max.abs()), top)
        }
    }

    /// `square(i)`.
    pub fn square(i: Interval) -> IntervalResult {
        if i.is_nai() {
            return Ok(Interval::NAI);
        }
        let (a, b) = (i.min * i.min, i.max * i.max);
        let top = java::max(a, b);
        if i.contains(0.0) {
            Interval::of(0.0, top)
        } else {
            Interval::of(java::min(a, b), top)
        }
    }

    /// `mapMonotonic(i, f)`, for `f` monotonic.
    pub fn map_monotonic(i: Interval, f: impl Fn(f32) -> f32) -> IntervalResult {
        if i.is_nai() {
            return Ok(Interval::NAI);
        }
        let (a, b) = (f(i.min), f(i.max));
        if a.is_nan() || b.is_nan() {
            return Err(IntervalError::MonotonicNaN);
        }
        Interval::of(java::min(a, b), java::max(a, b))
    }

    /// `log(i)`: NaI if every value is negative, else `ln` of the non-negative part.
    pub fn log(i: Interval) -> IntervalResult {
        if i.max < 0.0 {
            return Ok(Interval::NAI);
        }
        let positive = Interval::max_of(i, Interval::exact(0.0)?)?;
        Interval::map_monotonic(positive, |v| f64::from(v).ln() as f32)
    }

    /// `sign(i)`.
    pub fn sign(i: Interval) -> IntervalResult {
        if i.is_nai() {
            return Ok(Interval::NAI);
        }
        if i.min == i.max {
            return Interval::exact(java::signum(i.min));
        }
        if i.contains(0.0) {
            if i.min == 0.0 {
                Interval::of(0.0, 1.0)
            } else if i.max == 0.0 {
                Interval::of(-1.0, 0.0)
            } else {
                Interval::of(-1.0, 1.0)
            }
        } else {
            Interval::exact(if i.min > 0.0 { 1.0 } else { -1.0 })
        }
    }

    /// `lerp(alpha, first, second)`: every combination of the bounds of `first` and `second`.
    pub fn lerp(alpha: Interval, first: Interval, second: Interval) -> IntervalResult {
        if alpha.is_nai() || first.is_nai() || second.is_nai() {
            return Ok(Interval::NAI);
        }
        Interval::encapsulating([
            Interval::lerp_values(alpha, first.min, second.min)?,
            Interval::lerp_values(alpha, first.max, second.min)?,
            Interval::lerp_values(alpha, first.min, second.max)?,
            Interval::lerp_values(alpha, first.max, second.max)?,
        ])
    }

    /// `lerp(alpha, first, second)` with fixed endpoints.
    fn lerp_values(alpha: Interval, first: f32, second: f32) -> IntervalResult {
        if alpha.is_nai() || first.is_nan() || second.is_nan() {
            return Ok(Interval::NAI);
        }
        if first.is_finite() && second.is_finite() {
            // lerpFiniteBounds: first + mulBound(t, second − first) at each bound of alpha.
            let bound = |t: f32| first + mul_bound(t, second - first);
            return Interval::encapsulating_values(bound(alpha.min), bound(alpha.max));
        }
        // lerpInfiniteBounds.
        if first == second {
            return Interval::exact(first);
        }
        let (a, b) = (
            lerp_infinite_bound(alpha.min, first, second),
            lerp_infinite_bound(alpha.max, first, second),
        );
        if a.is_nan() || b.is_nan() {
            return Ok(Interval::NAI);
        }
        Interval::encapsulating_values(a, b)
    }

    /// `pow(base, exponent)`.
    pub fn pow(base: Interval, exponent: Interval) -> IntervalResult {
        if base.is_nai() || exponent.is_nai() {
            return Ok(Interval::NAI);
        }
        if base.min == base.max {
            return pow_base(base.min, exponent);
        }
        let mut result = Interval::encapsulating([
            pow_base(base.min, exponent)?,
            pow_base(base.max, exponent)?,
        ])?;
        if base.contains(0.0) {
            if base.max > 0.0 {
                result = Interval::encapsulating([result, pow_base(0.0, exponent)?])?;
            }
            if base.min < 0.0 {
                result = Interval::encapsulating([result, pow_base(-0.0, exponent)?])?;
            }
        }
        Ok(result)
    }
}

/// `mulBound(a, b)`: `0` if either is zero (so `0 · ∞` is `0`), else `a · b`.
fn mul_bound(a: f32, b: f32) -> f32 {
    if a == 0.0 || b == 0.0 { 0.0 } else { a * b }
}

/// `lerpInfiniteBound(t, first, second)`.
fn lerp_infinite_bound(t: f32, first: f32, second: f32) -> f32 {
    let a = mul_bound(1.0 - t, first);
    let b = mul_bound(t, second);
    if a.is_infinite() && b.is_infinite() {
        if t <= 0.0 {
            return if second > first {
                f32::NEG_INFINITY
            } else {
                f32::INFINITY
            };
        }
        if t >= 1.0 {
            return if second > first {
                f32::INFINITY
            } else {
                f32::NEG_INFINITY
            };
        }
        return f32::NAN;
    }
    a + b
}

/// `Math.pow` on `float`s widened to `double`, narrowed back.
fn powf(base: f32, exponent: f32) -> f32 {
    f64::from(base).powf(f64::from(exponent)) as f32
}

/// `pow(float, Interval)`: a fixed base.
fn pow_base(base: f32, exponent: Interval) -> IntervalResult {
    if base.is_nan() || exponent.is_nai() {
        return Ok(Interval::NAI);
    }
    if exponent.min == exponent.max {
        let v = powf(base, exponent.min);
        return if v.is_nan() {
            Ok(Interval::NAI)
        } else {
            Interval::exact(v)
        };
    }
    if base == 0.0 {
        return Interval::mul(
            pow_zero_base(exponent)?,
            Interval::exact(1.0_f32.copysign(base))?,
        );
    }
    if base == 1.0 {
        return Interval::exact(1.0);
    }
    if base > 0.0 {
        // powPositiveBase.
        if exponent.min.is_finite() && exponent.max.is_finite() {
            return Interval::encapsulating_values(
                powf(base, exponent.min),
                powf(base, exponent.max),
            );
        }
        return pow_infinite_exponent(base, exponent);
    }
    pow_negative_base(base, exponent)
}

/// `powZeroBase(exponent)`.
fn pow_zero_base(exponent: Interval) -> IntervalResult {
    if exponent.contains(0.0) {
        if exponent.max == 0.0 {
            Interval::of(1.0, f32::INFINITY)
        } else if exponent.min == 0.0 {
            Interval::of(0.0, 1.0)
        } else {
            Interval::of(0.0, f32::INFINITY)
        }
    } else if exponent.max < 0.0 {
        Interval::exact(f32::INFINITY)
    } else {
        Interval::exact(0.0)
    }
}

/// `powInfiniteExponent(base, exponent)`, for a positive base other than 1.
fn pow_infinite_exponent(base: f32, exponent: Interval) -> IntervalResult {
    if exponent.min.is_infinite() && exponent.max.is_infinite() {
        return Interval::of(0.0, f32::INFINITY);
    }
    if exponent.min.is_infinite() {
        return if base < 1.0 {
            Interval::of(powf(base, exponent.max), f32::INFINITY)
        } else {
            Interval::of(0.0, powf(base, exponent.max))
        };
    }
    if base < 1.0 {
        Interval::of(0.0, powf(base, exponent.min))
    } else {
        Interval::of(powf(base, exponent.min), f32::INFINITY)
    }
}

/// `powNegativeBase(base, exponent)`: only integer exponents give real values, so the bounds
/// are the integers at each end and their neighbours (for the other parity).
fn pow_negative_base(base: f32, exponent: Interval) -> IntervalResult {
    let lo = f64::from(exponent.min).ceil() as f32;
    let hi = f64::from(exponent.max).floor() as f32;
    if hi < lo {
        return Ok(Interval::NAI);
    }
    let (at_lo, at_hi) = (powf(base, lo), powf(base, hi));
    let mut result = Interval::encapsulating_values(at_lo, at_hi)?;
    if lo.is_infinite() {
        result = result.encapsulating_with(-at_lo)?;
    } else if lo + 1.0 < hi {
        result = result.encapsulating_with(powf(base, lo + 1.0))?;
    }
    if hi.is_infinite() {
        result = result.encapsulating_with(-at_hi)?;
    } else if hi - 1.0 > lo {
        result = result.encapsulating_with(powf(base, hi - 1.0))?;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn i(min: f32, max: f32) -> Interval {
        Interval::of(min, max).unwrap()
    }

    #[test]
    fn of_refuses_nan_and_reversed_bounds() {
        assert_eq!(Interval::of(2.0, 1.0), Err(IntervalError::Bounds));
        assert_eq!(Interval::of(f32::NAN, 1.0), Err(IntervalError::Bounds));
        assert!(Interval::NAI.is_nai());
        assert_eq!(Interval::NAI, Interval::NAI);
        assert_ne!(Interval::NAI, i(0.0, 0.0));
    }

    #[test]
    fn arithmetic() {
        assert_eq!(
            Interval::add(i(-1.0, 2.0), i(3.0, 4.0)).unwrap(),
            i(2.0, 6.0)
        );
        assert_eq!(
            Interval::sub(i(-1.0, 2.0), i(3.0, 4.0)).unwrap(),
            i(-5.0, -1.0)
        );
        assert_eq!(
            Interval::mul(i(-1.0, 2.0), i(-3.0, 4.0)).unwrap(),
            i(-6.0, 8.0)
        );
        // ∞ + (−∞) is NaN at the upper bound: NaI.
        let inf = Interval::INFINITE;
        let minus_inf = i(f32::NEG_INFINITY, f32::NEG_INFINITY);
        assert!(Interval::add(inf, minus_inf).unwrap().is_nai());
        assert_eq!(Interval::add(inf, inf).unwrap(), inf);
        // mulBound: 0 · ∞ is 0, not NaN.
        assert_eq!(Interval::mul(i(0.0, 1.0), inf).unwrap(), inf);
        assert_eq!(Interval::mul(i(0.0, 0.0), inf).unwrap(), i(0.0, 0.0));
    }

    #[test]
    fn reciprocal_and_div() {
        assert_eq!(Interval::reciprocal(i(2.0, 4.0)).unwrap(), i(0.25, 0.5));
        assert_eq!(
            Interval::reciprocal(i(-2.0, 0.0)).unwrap(),
            i(f32::NEG_INFINITY, -0.5)
        );
        assert_eq!(
            Interval::reciprocal(i(0.0, 4.0)).unwrap(),
            i(0.25, f32::INFINITY)
        );
        assert_eq!(
            Interval::reciprocal(i(-1.0, 1.0)).unwrap(),
            Interval::INFINITE
        );
        assert!(Interval::reciprocal(i(0.0, 0.0)).unwrap().is_nai());
        assert_eq!(
            Interval::div(i(1.0, 2.0), i(2.0, 4.0)).unwrap(),
            i(0.25, 1.0)
        );
    }

    #[test]
    fn unary_ranges() {
        assert_eq!(Interval::abs(i(-3.0, 2.0)).unwrap(), i(0.0, 3.0));
        assert_eq!(Interval::abs(i(-3.0, -2.0)).unwrap(), i(2.0, 3.0));
        assert_eq!(Interval::square(i(-3.0, 2.0)).unwrap(), i(0.0, 9.0));
        assert_eq!(Interval::square(i(2.0, 3.0)).unwrap(), i(4.0, 9.0));
        assert_eq!(
            Interval::clamp(i(-3.0, 2.0), -1.0, 1.0).unwrap(),
            i(-1.0, 1.0)
        );
        assert_eq!(
            Interval::clamp(i(3.0, 4.0), -1.0, 1.0).unwrap(),
            i(1.0, 1.0)
        );
        assert_eq!(
            Interval::clamp(i(-4.0, -3.0), -1.0, 1.0).unwrap(),
            i(-1.0, -1.0)
        );
        assert_eq!(Interval::sign(i(-3.0, 0.0)).unwrap(), i(-1.0, 0.0));
        assert_eq!(Interval::sign(i(2.0, 3.0)).unwrap(), i(1.0, 1.0));
        assert!(Interval::log(i(-3.0, -1.0)).unwrap().is_nai());
        assert_eq!(
            Interval::log(i(-3.0, 1.0)).unwrap(),
            i(f32::NEG_INFINITY, 0.0)
        );
        assert_eq!(
            Interval::map_monotonic(i(-2.0, 3.0), |v| -v).unwrap(),
            i(-3.0, 2.0)
        );
    }

    #[test]
    fn min_max_and_encapsulating() {
        assert_eq!(
            Interval::min_of(i(0.0, 5.0), i(1.0, 2.0)).unwrap(),
            i(0.0, 2.0)
        );
        assert_eq!(
            Interval::max_of(i(0.0, 5.0), i(1.0, 2.0)).unwrap(),
            i(1.0, 5.0)
        );
        assert_eq!(
            Interval::encapsulating([i(0.0, 1.0), Interval::NAI, i(-2.0, -1.0)]).unwrap(),
            i(-2.0, 1.0)
        );
        assert!(Interval::encapsulating([Interval::NAI]).unwrap().is_nai());
        assert_eq!(
            Interval::encapsulating_values(f32::NAN, 3.0).unwrap(),
            i(3.0, 3.0)
        );
    }

    #[test]
    fn lerp_ranges() {
        // alpha in [0, 1] between [0, 1] and [10, 20]: anywhere from 0 to 20.
        assert_eq!(
            Interval::lerp(i(0.0, 1.0), i(0.0, 1.0), i(10.0, 20.0)).unwrap(),
            i(0.0, 20.0)
        );
        // Extrapolating alpha widens it.
        assert_eq!(
            Interval::lerp(i(-1.0, 2.0), i(0.0, 0.0), i(1.0, 1.0)).unwrap(),
            i(-1.0, 2.0)
        );
        // Infinite endpoints: alpha 0 gives first, alpha 1 second.
        let r = Interval::lerp(
            i(0.0, 1.0),
            i(f32::NEG_INFINITY, f32::NEG_INFINITY),
            i(5.0, 5.0),
        )
        .unwrap();
        assert_eq!(r, i(f32::NEG_INFINITY, 5.0));
    }

    #[test]
    fn pow_ranges() {
        assert_eq!(
            Interval::pow(i(2.0, 2.0), i(1.0, 3.0)).unwrap(),
            i(2.0, 8.0)
        );
        assert_eq!(
            Interval::pow(i(0.5, 0.5), i(1.0, 3.0)).unwrap(),
            i(0.125, 0.5)
        );
        // A base range through zero adds 0^e.
        assert_eq!(
            Interval::pow(i(-1.0, 2.0), i(2.0, 2.0)).unwrap(),
            i(0.0, 4.0)
        );
        // Negative bases: only integer exponents, both parities.
        assert_eq!(
            Interval::pow(i(-2.0, -2.0), i(1.0, 3.0)).unwrap(),
            i(-8.0, 4.0)
        );
        assert!(Interval::pow(i(-2.0, -2.0), i(1.2, 1.8)).unwrap().is_nai());
        // Zero base: 0^[1, 2] is 0, 0^[-1, 1] reaches infinity.
        assert_eq!(
            Interval::pow(i(0.0, 0.0), i(1.0, 2.0)).unwrap(),
            i(0.0, 0.0)
        );
        assert_eq!(
            Interval::pow(i(0.0, 0.0), i(-1.0, 1.0)).unwrap(),
            i(0.0, f32::INFINITY)
        );
        // Infinite exponents with a base below 1 approach 0.
        assert_eq!(
            Interval::pow(i(0.5, 0.5), i(1.0, f32::INFINITY)).unwrap(),
            i(0.0, 0.5)
        );
    }
}
