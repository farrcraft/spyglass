//! Value providers: the game's `IntProvider`, `FloatProvider` and `HeightProvider`, which data
//! uses wherever a number is drawn at random, as their codecs read them and as they sample
//! (`docs/specs/providers.md`).

use serde_json::{Map, Value};
use spyglass_rng::RandomSource;

use crate::anchor::{AnchorHeights, VerticalAnchor};
use crate::codec::{At, LoadErrorKind, LoadResult, field, float, full_id, int, object};

/// `IntProvider`.
#[derive(Debug, Clone, PartialEq)]
pub enum IntProvider {
    /// `constant`, or a bare integer.
    Constant(i32),
    /// `uniform`: `randomBetweenInclusive(min, max)`.
    Uniform(i32, i32),
    /// `biased_to_bottom`: `min + nextInt(nextInt(max − min + 1) + 1)`.
    BiasedToBottom(i32, i32),
    /// `very_biased_to_bottom`: `min + nextInt(nextInt(nextInt(max − min + 1) + 1) + 1)`.
    VeryBiasedToBottom(i32, i32),
    /// `clamped`: `source`, clamped to `[min, max]`.
    Clamped(Box<IntProvider>, i32, i32),
    /// `clamped_normal`: `mean + gaussian · deviation`, clamped in `f32`, then truncated.
    ClampedNormal {
        /// `mean`.
        mean: f32,
        /// `deviation`.
        deviation: f32,
        /// `min_inclusive`.
        min: i32,
        /// `max_inclusive`.
        max: i32,
    },
    /// `trapezoid`: `min`, `max` and `plateau`.
    Trapezoid(i32, i32, i32),
    /// `weighted_list`: one of the providers, by weight.
    WeightedList(Weighted<IntProvider>),
}

/// `FloatProvider`.
#[derive(Debug, Clone, PartialEq)]
pub enum FloatProvider {
    /// `constant`, or a bare number.
    Constant(f32),
    /// `uniform`: `min_inclusive + nextFloat · (max_exclusive − min_inclusive)`.
    Uniform(f32, f32),
    /// `clamped_normal`: `mean + gaussian · deviation`, clamped to `[min, max]`.
    ClampedNormal {
        /// `mean`.
        mean: f32,
        /// `deviation`.
        deviation: f32,
        /// `min`.
        min: f32,
        /// `max`.
        max: f32,
    },
    /// `trapezoid`: `min`, `max` and `plateau`.
    Trapezoid(f32, f32, f32),
}

/// `HeightProvider`.
#[derive(Debug, Clone, PartialEq)]
pub enum HeightProvider {
    /// `constant`, or a bare anchor.
    Constant(VerticalAnchor),
    /// `uniform`: `randomBetweenInclusive(min, max)`, or `min` if the range is empty.
    Uniform(VerticalAnchor, VerticalAnchor),
    /// `biased_to_bottom`, with `inner`.
    BiasedToBottom(VerticalAnchor, VerticalAnchor, i32),
    /// `very_biased_to_bottom`, with `inner`.
    VeryBiasedToBottom(VerticalAnchor, VerticalAnchor, i32),
    /// `trapezoid`, with `plateau`.
    Trapezoid(VerticalAnchor, VerticalAnchor, i32),
    /// `weighted_list`: one of the providers, by weight.
    WeightedList(Weighted<HeightProvider>),
}

/// `WeightedList`: entries with non-negative weights, at least one of them positive.
#[derive(Debug, Clone, PartialEq)]
pub struct Weighted<T> {
    entries: Vec<(T, i32)>,
    total: i32,
}

impl<T> Weighted<T> {
    /// `getRandomOrThrow`: `nextInt(totalWeight)`, then the entry whose cumulative weight passes
    /// it (both the game's flat and compact selectors pick the same).
    pub fn pick(&self, random: &mut impl RandomSource) -> &T {
        let mut r = random.next_int_bounded(self.total);
        for (value, weight) in &self.entries {
            r -= weight;
            if r < 0 {
                return value;
            }
        }
        unreachable!("the total weight is the entries' sum")
    }

    /// The entries, in order.
    pub fn entries(&self) -> impl Iterator<Item = &T> {
        self.entries.iter().map(|(v, _)| v)
    }
}

/// `Mth.randomBetweenInclusive`: `nextInt(max − min + 1) + min`.
fn between_inclusive(random: &mut impl RandomSource, min: i32, max: i32) -> i32 {
    random
        .next_int_bounded(max.wrapping_sub(min).wrapping_add(1))
        .wrapping_add(min)
}

/// `Mth.nextInt(random, min, max)`: `min` if `min ≥ max`, else [`between_inclusive`].
fn next_int(random: &mut impl RandomSource, min: i32, max: i32) -> i32 {
    if min >= max {
        min
    } else {
        between_inclusive(random, min, max)
    }
}

/// `Math.min(float, float)`: NaN if either is, and −0 below +0.
fn java_min(a: f32, b: f32) -> f32 {
    if a.is_nan() || b.is_nan() {
        f32::NAN
    } else if a == b && a == 0.0 {
        if a.is_sign_negative() { a } else { b }
    } else if a < b {
        a
    } else {
        b
    }
}

/// `Mth.clamp(float, float, float)`: `min` below it, else `Math.min(value, max)`.
fn clamp_f32(value: f32, min: f32, max: f32) -> f32 {
    // fcmpg/ifge: NaN goes to Math.min.
    if value < min {
        min
    } else {
        java_min(value, max)
    }
}

/// `Mth.normal(random, mean, deviation)`: `mean + (float) nextGaussian() · deviation`.
fn normal(random: &mut impl RandomSource, mean: f32, deviation: f32) -> f32 {
    mean + random.next_gaussian() as f32 * deviation
}

impl IntProvider {
    /// `sample(random)`.
    pub fn sample(&self, random: &mut impl RandomSource) -> i32 {
        match self {
            IntProvider::Constant(v) => *v,
            IntProvider::Uniform(min, max) => between_inclusive(random, *min, *max),
            IntProvider::BiasedToBottom(min, max) => {
                let inner = random.next_int_bounded(max.wrapping_sub(*min).wrapping_add(1));
                min.wrapping_add(random.next_int_bounded(inner.wrapping_add(1)))
            }
            IntProvider::VeryBiasedToBottom(min, max) => {
                let a = random.next_int_bounded(max.wrapping_sub(*min).wrapping_add(1));
                let b = random.next_int_bounded(a.wrapping_add(1));
                min.wrapping_add(random.next_int_bounded(b.wrapping_add(1)))
            }
            IntProvider::Clamped(source, min, max) => source.sample(random).max(*min).min(*max),
            IntProvider::ClampedNormal {
                mean,
                deviation,
                min,
                max,
            } => clamp_f32(normal(random, *mean, *deviation), *min as f32, *max as f32) as i32,
            IntProvider::Trapezoid(min, max, plateau) => {
                if *plateau == 0 && *max == min.wrapping_neg() {
                    let bound = max.wrapping_add(1);
                    let a = random.next_int_bounded(bound);
                    return a.wrapping_sub(random.next_int_bounded(bound));
                }
                let range = max.wrapping_sub(*min);
                if *plateau == range {
                    return between_inclusive(random, *min, *max);
                }
                let side = range.wrapping_sub(*plateau) / 2;
                let rest = range.wrapping_sub(side);
                let a = between_inclusive(random, 0, rest);
                min.wrapping_add(a)
                    .wrapping_add(between_inclusive(random, 0, side))
            }
            IntProvider::WeightedList(list) => list.pick(random).sample(random),
        }
    }

    /// `getMinValue()`.
    pub fn min(&self) -> i32 {
        match self {
            IntProvider::Constant(v) => *v,
            IntProvider::Uniform(min, _)
            | IntProvider::BiasedToBottom(min, _)
            | IntProvider::VeryBiasedToBottom(min, _)
            | IntProvider::ClampedNormal { min, .. }
            | IntProvider::Trapezoid(min, _, _) => *min,
            IntProvider::Clamped(source, min, _) => (*min).max(source.min()),
            IntProvider::WeightedList(list) => list.entries().map(Self::min).min().unwrap_or(0),
        }
    }

    /// `getMaxValue()`.
    pub fn max(&self) -> i32 {
        match self {
            IntProvider::Constant(v) => *v,
            IntProvider::Uniform(_, max)
            | IntProvider::BiasedToBottom(_, max)
            | IntProvider::VeryBiasedToBottom(_, max)
            | IntProvider::ClampedNormal { max, .. }
            | IntProvider::Trapezoid(_, max, _) => *max,
            IntProvider::Clamped(source, _, max) => (*max).min(source.max()),
            IntProvider::WeightedList(list) => list.entries().map(Self::max).max().unwrap_or(0),
        }
    }

    /// `IntProviders.CODEC`: a bare integer, or a provider by `type`.
    pub fn parse(json: &Value, at: &At<'_>) -> LoadResult<IntProvider> {
        if !json.is_object() {
            return Ok(IntProvider::Constant(int(json, at)?));
        }
        let o = object(json, at)?;
        let int_field = |name: &'static str| int(field(o, name, at)?, &at.child(name));
        let range = || -> LoadResult<(i32, i32)> {
            let (min, max) = (int_field("min_inclusive")?, int_field("max_inclusive")?);
            if max < min {
                return Err(at.error(LoadErrorKind::Invalid("max must be at least min")));
            }
            Ok((min, max))
        };
        Ok(match type_of(o, at)?.as_str() {
            "minecraft:constant" => IntProvider::Constant(int_field("value")?),
            "minecraft:uniform" => {
                let (min, max) = range()?;
                IntProvider::Uniform(min, max)
            }
            "minecraft:biased_to_bottom" => {
                let (min, max) = range()?;
                IntProvider::BiasedToBottom(min, max)
            }
            "minecraft:very_biased_to_bottom" => {
                let (min, max) = range()?;
                IntProvider::VeryBiasedToBottom(min, max)
            }
            "minecraft:clamped" => {
                let source = IntProvider::parse(field(o, "source", at)?, &at.child("source"))?;
                let (min, max) = range()?;
                IntProvider::Clamped(Box::new(source), min, max)
            }
            "minecraft:clamped_normal" => {
                let float_field = |name: &'static str| float(field(o, name, at)?, &at.child(name));
                let (mean, deviation) = (float_field("mean")?, float_field("deviation")?);
                let (min, max) = range()?;
                IntProvider::ClampedNormal {
                    mean,
                    deviation,
                    min,
                    max,
                }
            }
            "minecraft:trapezoid" => {
                let (min, max) = (int_field("min")?, int_field("max")?);
                let plateau = int_field("plateau")?;
                if max < min {
                    return Err(at.error(LoadErrorKind::Invalid("max must be at least min")));
                }
                if plateau > max.wrapping_sub(min) {
                    return Err(
                        at.error(LoadErrorKind::Invalid("plateau can be at most the range"))
                    );
                }
                IntProvider::Trapezoid(min, max, plateau)
            }
            "minecraft:weighted_list" => {
                IntProvider::WeightedList(weighted(o, at, IntProvider::parse)?)
            }
            kind => return Err(unsupported(at, "int provider type", kind)),
        })
    }
}

impl FloatProvider {
    /// `sample(random)`.
    pub fn sample(&self, random: &mut impl RandomSource) -> f32 {
        match self {
            FloatProvider::Constant(v) => *v,
            FloatProvider::Uniform(min, max) => random.next_float() * (max - min) + min,
            FloatProvider::ClampedNormal {
                mean,
                deviation,
                min,
                max,
            } => clamp_f32(normal(random, *mean, *deviation), *min, *max),
            FloatProvider::Trapezoid(min, max, plateau) => {
                let range = max - min;
                let side = (range - plateau) / 2.0;
                let rest = range - side;
                let a = random.next_float() * rest;
                min + a + random.next_float() * side
            }
        }
    }

    /// `min()`.
    pub fn min(&self) -> f32 {
        match self {
            FloatProvider::Constant(v) => *v,
            FloatProvider::Uniform(min, _)
            | FloatProvider::ClampedNormal { min, .. }
            | FloatProvider::Trapezoid(min, _, _) => *min,
        }
    }

    /// `max()`: for `uniform`, the exclusive maximum.
    pub fn max(&self) -> f32 {
        match self {
            FloatProvider::Constant(v) => *v,
            FloatProvider::Uniform(_, max)
            | FloatProvider::ClampedNormal { max, .. }
            | FloatProvider::Trapezoid(_, max, _) => *max,
        }
    }

    /// `FloatProviders.CODEC`: a bare number, or a provider by `type`.
    pub fn parse(json: &Value, at: &At<'_>) -> LoadResult<FloatProvider> {
        if !json.is_object() {
            return Ok(FloatProvider::Constant(float(json, at)?));
        }
        let o = object(json, at)?;
        let float_field = |name: &'static str| float(field(o, name, at)?, &at.child(name));
        Ok(match type_of(o, at)?.as_str() {
            "minecraft:constant" => FloatProvider::Constant(float_field("value")?),
            "minecraft:uniform" => {
                let (min, max) = (float_field("min_inclusive")?, float_field("max_exclusive")?);
                // fcmpg/ifgt: NaN passes.
                if max <= min {
                    return Err(at.error(LoadErrorKind::Invalid(
                        "max_exclusive must be greater than min_inclusive",
                    )));
                }
                FloatProvider::Uniform(min, max)
            }
            "minecraft:clamped_normal" => {
                let (mean, deviation) = (float_field("mean")?, float_field("deviation")?);
                let (min, max) = (float_field("min")?, float_field("max")?);
                if max < min {
                    return Err(at.error(LoadErrorKind::Invalid("max must be at least min")));
                }
                FloatProvider::ClampedNormal {
                    mean,
                    deviation,
                    min,
                    max,
                }
            }
            "minecraft:trapezoid" => {
                let (min, max) = (float_field("min")?, float_field("max")?);
                let plateau = float_field("plateau")?;
                if max < min {
                    return Err(at.error(LoadErrorKind::Invalid("max must be at least min")));
                }
                if plateau > max - min {
                    return Err(
                        at.error(LoadErrorKind::Invalid("plateau can be at most the range"))
                    );
                }
                FloatProvider::Trapezoid(min, max, plateau)
            }
            kind => return Err(unsupported(at, "float provider type", kind)),
        })
    }

    /// `FloatProviders.codec(min, max)`: [`parse`](Self::parse), then the provider's range must
    /// be within `[min, max]` (`max` unbounded if `None`).
    pub fn parse_in(
        json: &Value,
        min: f32,
        max: Option<f32>,
        at: &At<'_>,
    ) -> LoadResult<FloatProvider> {
        let p = Self::parse(json, at)?;
        // fcmpg/ifge, fcmpl/ifle: NaN passes both.
        if p.min() < min || max.is_some_and(|max| p.max() > max) {
            return Err(at.error(LoadErrorKind::OutOfRange {
                value: format!("[{}, {}]", p.min(), p.max()),
                min: min.to_string(),
                max: max.map_or("∞".to_string(), |m| m.to_string()),
            }));
        }
        Ok(p)
    }
}

impl HeightProvider {
    /// `sample(random, context)`, with the context's anchors.
    pub fn sample(&self, random: &mut impl RandomSource, anchors: AnchorHeights) -> i32 {
        let resolve = |a: &VerticalAnchor| a.resolve(anchors);
        match self {
            HeightProvider::Constant(a) => resolve(a),
            HeightProvider::Uniform(lo, hi) => {
                let (lo, hi) = (resolve(lo), resolve(hi));
                if lo > hi {
                    lo
                } else {
                    between_inclusive(random, lo, hi)
                }
            }
            HeightProvider::BiasedToBottom(lo, hi, inner) => {
                let (lo, hi) = (resolve(lo), resolve(hi));
                let span = hi.wrapping_sub(lo).wrapping_sub(*inner).wrapping_add(1);
                if span <= 0 {
                    return lo;
                }
                let a = random.next_int_bounded(span);
                random
                    .next_int_bounded(a.wrapping_add(*inner))
                    .wrapping_add(lo)
            }
            HeightProvider::VeryBiasedToBottom(lo, hi, inner) => {
                let (lo, hi) = (resolve(lo), resolve(hi));
                if hi.wrapping_sub(lo).wrapping_sub(*inner).wrapping_add(1) <= 0 {
                    return lo;
                }
                let a = next_int(random, lo.wrapping_add(*inner), hi);
                let b = next_int(random, lo, a.wrapping_sub(1));
                next_int(random, lo, b.wrapping_sub(1).wrapping_add(*inner))
            }
            HeightProvider::Trapezoid(lo, hi, plateau) => {
                let (lo, hi) = (resolve(lo), resolve(hi));
                if lo > hi {
                    return lo;
                }
                let range = hi.wrapping_sub(lo);
                if *plateau >= range {
                    return between_inclusive(random, lo, hi);
                }
                let side = range.wrapping_sub(*plateau) / 2;
                let rest = range.wrapping_sub(side);
                let a = between_inclusive(random, 0, rest);
                lo.wrapping_add(a)
                    .wrapping_add(between_inclusive(random, 0, side))
            }
            HeightProvider::WeightedList(list) => list.pick(random).sample(random, anchors),
        }
    }

    /// `HeightProvider.CODEC`: a bare anchor, or a provider by `type`.
    pub fn parse(json: &Value, at: &At<'_>) -> LoadResult<HeightProvider> {
        let o = object(json, at)?;
        if !o.contains_key("type") {
            return Ok(HeightProvider::Constant(VerticalAnchor::parse(json, at)?));
        }
        let anchor =
            |name: &'static str| VerticalAnchor::parse(field(o, name, at)?, &at.child(name));
        let optional = |name: &'static str, default: i32| match o.get(name) {
            None => Ok(default),
            Some(v) => int(v, &at.child(name)),
        };
        Ok(match type_of(o, at)?.as_str() {
            "minecraft:constant" => HeightProvider::Constant(anchor("value")?),
            "minecraft:uniform" => {
                HeightProvider::Uniform(anchor("min_inclusive")?, anchor("max_inclusive")?)
            }
            "minecraft:biased_to_bottom" | "minecraft:very_biased_to_bottom" => {
                let (lo, hi) = (anchor("min_inclusive")?, anchor("max_inclusive")?);
                let inner = optional("inner", 1)?;
                if inner < 1 {
                    return Err(at.child("inner").error(LoadErrorKind::OutOfRange {
                        value: inner.to_string(),
                        min: "1".to_string(),
                        max: i32::MAX.to_string(),
                    }));
                }
                if type_of(o, at)? == "minecraft:biased_to_bottom" {
                    HeightProvider::BiasedToBottom(lo, hi, inner)
                } else {
                    HeightProvider::VeryBiasedToBottom(lo, hi, inner)
                }
            }
            "minecraft:trapezoid" => HeightProvider::Trapezoid(
                anchor("min_inclusive")?,
                anchor("max_inclusive")?,
                optional("plateau", 0)?,
            ),
            "minecraft:weighted_list" => {
                HeightProvider::WeightedList(weighted(o, at, HeightProvider::parse)?)
            }
            kind => return Err(unsupported(at, "height provider type", kind)),
        })
    }
}

/// A map's `type`, as a full id.
fn type_of(o: &Map<String, Value>, at: &At<'_>) -> LoadResult<String> {
    field(o, "type", at)?
        .as_str()
        .map(full_id)
        .ok_or_else(|| at.child("type").error(LoadErrorKind::Shape("a type id")))
}

fn unsupported(at: &At<'_>, what: &'static str, kind: &str) -> crate::codec::LoadError {
    at.child("type").error(LoadErrorKind::Unsupported {
        what,
        name: kind.to_string(),
    })
}

/// `WeightedList.nonEmptyCodec`: `distribution`, a list of `{data, weight}` with weights at
/// least 0, at least one of them positive.
fn weighted<T>(
    o: &Map<String, Value>,
    at: &At<'_>,
    parse: impl Fn(&Value, &At<'_>) -> LoadResult<T>,
) -> LoadResult<Weighted<T>> {
    let at = at.child("distribution");
    let list = o
        .get("distribution")
        .ok_or_else(|| at.error(LoadErrorKind::Missing("distribution")))?
        .as_array()
        .ok_or_else(|| at.error(LoadErrorKind::Shape("a list")))?;
    let mut entries = Vec::with_capacity(list.len());
    let mut total: i32 = 0;
    for (i, entry) in list.iter().enumerate() {
        let at = at.child(&i.to_string());
        let e = object(entry, &at)?;
        let value = parse(field(e, "data", &at)?, &at.child("data"))?;
        let weight = int(field(e, "weight", &at)?, &at.child("weight"))?;
        if weight < 0 {
            return Err(at.child("weight").error(LoadErrorKind::OutOfRange {
                value: weight.to_string(),
                min: "0".to_string(),
                max: i32::MAX.to_string(),
            }));
        }
        total = total
            .checked_add(weight)
            .ok_or_else(|| at.error(LoadErrorKind::Invalid("total weight overflows")))?;
        entries.push((value, weight));
    }
    if total == 0 {
        return Err(at.error(LoadErrorKind::Invalid(
            "a weighted list needs an entry with non-zero weight",
        )));
    }
    Ok(Weighted { entries, total })
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use spyglass_rng::LegacyRandomSource;

    use super::*;

    const ANCHORS: AnchorHeights = AnchorHeights {
        min_y: -64,
        height: 384,
    };

    fn at() -> At<'static> {
        At::root("test")
    }

    #[test]
    fn bare_values_are_constants() {
        assert_eq!(
            IntProvider::parse(&json!(4), &at()).unwrap(),
            IntProvider::Constant(4)
        );
        assert_eq!(
            FloatProvider::parse(&json!(-0.7), &at()).unwrap(),
            FloatProvider::Constant(-0.7)
        );
        assert_eq!(
            HeightProvider::parse(&json!({"absolute": 3}), &at()).unwrap(),
            HeightProvider::Constant(VerticalAnchor::Absolute(3))
        );
    }

    #[test]
    fn samples_stay_in_range() {
        let ints = [
            json!({"type": "minecraft:uniform", "min_inclusive": 2, "max_inclusive": 9}),
            json!({"type": "minecraft:biased_to_bottom", "min_inclusive": 2, "max_inclusive": 9}),
            json!({"type": "minecraft:very_biased_to_bottom", "min_inclusive": 2, "max_inclusive": 9}),
            json!({"type": "minecraft:clamped", "min_inclusive": 2, "max_inclusive": 9,
                "source": {"type": "minecraft:uniform", "min_inclusive": -20, "max_inclusive": 20}}),
            json!({"type": "minecraft:clamped_normal", "mean": 5.0, "deviation": 9.0,
                "min_inclusive": 2, "max_inclusive": 9}),
            json!({"type": "minecraft:trapezoid", "min": 2, "max": 9, "plateau": 3}),
            json!({"type": "minecraft:weighted_list", "distribution": [
                {"data": 2, "weight": 1}, {"data": 9, "weight": 3}]}),
        ];
        let mut random = LegacyRandomSource::new(7);
        for json in ints {
            let p = IntProvider::parse(&json, &at()).unwrap();
            assert_eq!((p.min(), p.max()), (2, 9), "{json}");
            for _ in 0..200 {
                assert!((2..=9).contains(&p.sample(&mut random)), "{json}");
            }
        }
        let floats = [
            json!({"type": "minecraft:uniform", "min_inclusive": 0.5, "max_exclusive": 1.5}),
            json!({"type": "minecraft:clamped_normal", "mean": 1.0, "deviation": 3.0,
                "min": 0.5, "max": 1.5}),
            json!({"type": "minecraft:trapezoid", "min": 0.5, "max": 1.5, "plateau": 0.25}),
        ];
        for json in floats {
            let p = FloatProvider::parse(&json, &at()).unwrap();
            for _ in 0..200 {
                assert!((0.5..=1.5).contains(&p.sample(&mut random)), "{json}");
            }
        }
        let heights = [
            json!({"type": "minecraft:uniform", "min_inclusive": {"above_bottom": 8},
                "max_inclusive": {"absolute": 180}}),
            json!({"type": "minecraft:biased_to_bottom", "min_inclusive": {"above_bottom": 8},
                "max_inclusive": {"absolute": 180}, "inner": 8}),
            json!({"type": "minecraft:very_biased_to_bottom", "min_inclusive": {"above_bottom": 8},
                "max_inclusive": {"absolute": 180}}),
            json!({"type": "minecraft:trapezoid", "min_inclusive": {"above_bottom": 8},
                "max_inclusive": {"absolute": 180}, "plateau": 20}),
        ];
        for json in heights {
            let p = HeightProvider::parse(&json, &at()).unwrap();
            for _ in 0..200 {
                assert!(
                    (-56..=180).contains(&p.sample(&mut random, ANCHORS)),
                    "{json}"
                );
            }
        }
    }

    #[test]
    fn the_codecs_reject_what_the_game_rejects() {
        let error = |json: Value| FloatProvider::parse(&json, &at()).unwrap_err().to_string();
        assert_eq!(
            error(json!({"type": "minecraft:uniform", "min_inclusive": 1.0, "max_exclusive": 1.0})),
            "test: max_exclusive must be greater than min_inclusive"
        );
        let below = FloatProvider::parse_in(&json!(-0.5), 0.0, None, &at()).unwrap_err();
        assert_eq!(below.to_string(), "test: [-0.5, -0.5] is outside 0..=∞");
        let int_error = |json: Value| IntProvider::parse(&json, &at()).unwrap_err().to_string();
        assert_eq!(
            int_error(json!({"type": "minecraft:uniform", "min_inclusive": 3, "max_inclusive": 2})),
            "test: max must be at least min"
        );
        assert_eq!(
            int_error(json!({"type": "minecraft:weighted_list", "distribution": [
                {"data": 2, "weight": 0}]})),
            "test: distribution: a weighted list needs an entry with non-zero weight"
        );
        assert_eq!(
            int_error(json!({"type": "test:nope"})),
            r#"test: type: unsupported int provider type "test:nope""#
        );
    }
}
