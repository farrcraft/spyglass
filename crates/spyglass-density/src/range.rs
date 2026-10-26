//! `DensityFunction.range()` for each type, as the game computes it. Ranges only choose how `min`
//! and `max` compile, but that choice changes results in corner cases, so they follow the
//! bytecode bound for bound (see `docs/specs/density-functions.md`).

use spyglass_data::codec::LoadError;
use spyglass_noise::mth::exp2i;

use crate::function::{Binary, DensityFunction, Df, Spline, Unary};
use crate::interval::Interval;
use crate::java;
use crate::sampler::{Binder, LIMIT_FACTOR, leaky_relu, round_to_integer, squeeze};

/// The range of `function`, its inputs' through `binder` (which remembers them).
pub(crate) fn of(binder: &mut Binder<'_>, function: &Df) -> Result<Interval, LoadError> {
    Ok(match &**function {
        DensityFunction::Constant(v) => Interval::exact(*v)?,
        // NoiseFunction: the normal noise's range.
        DensityFunction::Noise { noise, .. } => {
            Interval::symmetric(binder.normal_noise(noise)?.range_bound())?
        }
        // ShiftNoiseFunction: the noise's range times VALUE_FACTOR.
        DensityFunction::ShiftA { noise }
        | DensityFunction::ShiftB { noise }
        | DensityFunction::Shift { noise } => Interval::mul(
            Interval::symmetric(binder.normal_noise(noise)?.range_bound())?,
            Interval::exact(4.0)?,
        )?,
        DensityFunction::Binary(op, left, right) => {
            let (a, b) = (binder.range(left)?, binder.range(right)?);
            match op {
                Binary::Add => Interval::add(a, b)?,
                Binary::Sub => Interval::sub(a, b)?,
                Binary::Mul => Interval::mul(a, b)?,
                Binary::Div => Interval::div(a, b)?,
                Binary::Min => Interval::min_of(a, b)?,
                Binary::Max => Interval::max_of(a, b)?,
            }
        }
        DensityFunction::Unary(op, input) => {
            let r = binder.range(input)?;
            match op {
                Unary::Abs => Interval::abs(r)?,
                Unary::Square => Interval::square(r)?,
                Unary::Cube => Interval::map_monotonic(r, |v| v * v * v)?,
                Unary::Sqrt => Interval::pow(r, Interval::exact(0.5)?)?,
                Unary::HalfNegative => Interval::map_monotonic(r, |v| leaky_relu(0.5, v))?,
                Unary::QuarterNegative => Interval::map_monotonic(r, |v| leaky_relu(0.25, v))?,
                Unary::Reciprocal => Interval::reciprocal(r)?,
                Unary::Negate => Interval::sub(Interval::exact(0.0)?, r)?,
                Unary::Squeeze => Interval::map_monotonic(r, squeeze)?,
                Unary::Log => Interval::log(r)?,
                Unary::Sign => Interval::sign(r)?,
            }
        }
        // RoundFunction: round(input / multiple) · multiple, the rounding mapped monotonically.
        DensityFunction::Round {
            kind,
            input,
            multiple,
        } => {
            let m = binder.range(multiple)?;
            let quotient = Interval::div(binder.range(input)?, m)?;
            let rounded = Interval::map_monotonic(quotient, |v| round_to_integer(v, *kind))?;
            Interval::mul(rounded, m)?
        }
        DensityFunction::Clamp { input, min, max } => {
            Interval::clamp(binder.range(input)?, *min, *max)?
        }
        DensityFunction::Pow { base, exponent } => {
            Interval::pow(binder.range(base)?, binder.range(exponent)?)?
        }
        DensityFunction::Cache(input)
        | DensityFunction::BlendDensity(input)
        | DensityFunction::Slice { input, .. }
        | DensityFunction::Interpolated { input, .. } => binder.range(input)?,
        DensityFunction::Lerp {
            alpha,
            first,
            second,
        } => Interval::lerp(
            binder.range(alpha)?,
            binder.range(first)?,
            binder.range(second)?,
        )?,
        DensityFunction::RangeChoice {
            when_in_range,
            when_out_of_range,
            ..
        } => Interval::encapsulating([
            binder.range(when_in_range)?,
            binder.range(when_out_of_range)?,
        ])?,
        DensityFunction::IntervalSelect { functions, .. } => {
            let ranges = functions
                .iter()
                .map(|f| binder.range(f))
                .collect::<Result<Vec<_>, _>>()?;
            Interval::encapsulating(ranges)?
        }
        // FindTopSurfaceFunction: from the lower bound to the upper bound's maximum, if higher.
        DensityFunction::FindTopSurface {
            upper_bound,
            lower_bound,
            ..
        } => {
            let lower = *lower_bound as f32;
            Interval::of(lower, java::max(lower, binder.range(upper_bound)?.max()))?
        }
        // BlendedNoise.range: computeFbmRange(-15, yMultiplier · smear, LIMIT_FACTOR).
        DensityFunction::OldBlendedNoise(p) => fbm_range(
            -15,
            684.412 * p.y_scale * p.smear_scale_multiplier,
            LIMIT_FACTOR,
        )?,
        DensityFunction::Gradient(g) => Interval::encapsulating_values(g.from_value, g.to_value)?,
        DensityFunction::Spline(spline) => spline_range(binder, spline)?,
        // SimpleDensityFunction: blend_alpha [0, 1], blend_offset and beardifier unbounded.
        DensityFunction::BlendAlpha => Interval::of(0.0, 1.0)?,
        DensityFunction::BlendOffset | DensityFunction::Beardifier => Interval::INFINITE,
        DensityFunction::DistanceToPoint { .. } => Interval::of(0.0, f32::INFINITY)?,
        DensityFunction::EndOuterIslands => Interval::of(-0.84375, 0.5625)?,
    })
}

/// `BlendedNoise.computeFbmRange`: the layers' ranges summed, each a smeared Perlin noise's
/// (`±(|smear| + 2)`) times its amplitude, as `createFbm` lays them out.
fn fbm_range(first_octave: i32, smear: f64, factor: f64) -> Result<Interval, LoadError> {
    let n = 1 - first_octave;
    let mut frequency = 1.0;
    let mut amplitude = factor / (exp2i(n) - 1.0);
    let mut range = Interval::exact(0.0)?;
    for _ in 0..n {
        let layer = Interval::symmetric(((smear * frequency).abs() + 2.0) as f32)?;
        let layer = Interval::mul(layer, Interval::exact(amplitude as f32)?)?;
        range = Interval::add(range, layer)?;
        frequency /= 2.0;
        amplitude *= 2.0;
    }
    Ok(range)
}

/// `CubicSpline.range()`.
fn spline_range(binder: &mut Binder<'_>, spline: &Spline) -> Result<Interval, LoadError> {
    let points = match spline {
        Spline::Constant(v) => return Ok(Interval::exact(*v)?),
        Spline::Multipoint(points) => points,
    };
    let (locations, derivatives) = (points.locations(), points.derivatives());
    let last = locations.len() - 1;
    let coordinate = binder.range(points.coordinate())?;
    if coordinate.is_nai() {
        return Ok(coordinate);
    }
    let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
    // linearExtend, as sampling does beyond the end points.
    let extend = |c: f32, value: f32, i: usize| {
        let d = derivatives[i];
        if d == 0.0 {
            value
        } else {
            value + d * (c - locations[i])
        }
    };
    let ranges = points
        .values()
        .iter()
        .map(|v| spline_range(binder, v))
        .collect::<Result<Vec<_>, _>>()?;
    // The game takes the end points' ranges before the list of all of them; the values are the
    // same either way.
    if coordinate.min() < locations[0] {
        let (a, b) = (
            extend(coordinate.min(), ranges[0].min(), 0),
            extend(coordinate.min(), ranges[0].max(), 0),
        );
        lo = java::min(lo, java::min(a, b));
        hi = java::max(hi, java::max(a, b));
    }
    if coordinate.max() > locations[last] {
        let (a, b) = (
            extend(coordinate.max(), ranges[last].min(), last),
            extend(coordinate.max(), ranges[last].max(), last),
        );
        lo = java::min(lo, java::min(a, b));
        hi = java::max(hi, java::max(a, b));
    }
    for r in &ranges {
        lo = java::min(lo, r.min());
        hi = java::max(hi, r.max());
    }
    // Between points, the cubic's overshoot is bounded by a quarter of its tangent terms.
    for i in 0..last {
        let dx = locations[i + 1] - locations[i];
        let (r0, r1) = (ranges[i], ranges[i + 1]);
        let (d0, d1) = (derivatives[i], derivatives[i + 1]);
        if d0 == 0.0 && d1 == 0.0 {
            continue;
        }
        let (p, q) = (d0 * dx, d1 * dx);
        let low = java::min(r0.min(), r1.min());
        let high = java::max(r0.max(), r1.max());
        let a = p - r1.max() + r0.min();
        let b = p - r1.min() + r0.max();
        let c = -q + r1.min() - r0.max();
        let d = -q + r1.max() - r0.min();
        lo = java::min(lo, low + 0.25 * java::min(a, c));
        hi = java::max(hi, high + 0.25 * java::max(b, d));
    }
    Ok(Interval::of(lo, hi)?)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use super::*;
    use crate::function::{Axis, Gradient, Points, Tiling};

    fn i(min: f32, max: f32) -> Interval {
        Interval::of(min, max).unwrap()
    }

    /// A spline over x from `x0` to `x1` (values `x0..x1` along x), with points at locations 0
    /// and 1, values 0 and 1, and both derivatives `d`.
    fn spline(x0: f32, x1: f32, d: f32) -> Df {
        let coordinate = Arc::new(DensityFunction::Gradient(Gradient {
            axis: Axis::X,
            tiling: Tiling::ClampToEdge,
            from_coordinate: 0,
            from_value: x0,
            to_coordinate: 1,
            to_value: x1,
        }));
        let points = Points::new(
            coordinate,
            vec![0.0, 1.0],
            vec![Spline::Constant(0.0), Spline::Constant(1.0)],
            vec![d, d],
        )
        .unwrap();
        Arc::new(DensityFunction::Spline(Arc::new(Spline::Multipoint(
            points,
        ))))
    }

    fn range_of(f: &Df) -> Interval {
        let noises = HashMap::new();
        of(&mut Binder::new(&noises, 0), f).unwrap()
    }

    #[test]
    fn spline_ranges() {
        // Flat ends: just the values.
        assert_eq!(range_of(&spline(0.0, 1.0, 0.0)), i(0.0, 1.0));
        // Derivatives 4 over a step of 1: p = q = 4, so the bound widens by a quarter of
        // (4 − 1 + 0) on each side.
        assert_eq!(range_of(&spline(0.0, 1.0, 4.0)), i(-0.75, 1.75));
        // A coordinate reaching −1 and 2 extends the end values linearly: 0 + 4·(−1 − 0) and
        // 1 + 4·(2 − 1).
        assert_eq!(range_of(&spline(-1.0, 2.0, 4.0)), i(-4.0, 5.0));
    }

    #[test]
    fn fbm_ranges_sum_the_layers() {
        // One layer at amplitude factor: ±(|3| + 2) · 2.
        assert_eq!(fbm_range(0, 3.0, 2.0).unwrap(), i(-10.0, 10.0));
        // Two: amplitudes 3/3 and 6/3 at smears 4 and 2, so ±(6 · 1 + 4 · 2).
        assert_eq!(fbm_range(-1, 4.0, 3.0).unwrap(), i(-14.0, 14.0));
    }
}
