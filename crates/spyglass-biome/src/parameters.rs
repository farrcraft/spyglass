//! Climate parameters and targets (`Climate.Parameter`, `Climate.ParameterPoint`,
//! `Climate.TargetPoint`), and the parameter list from the data generator's report.

use serde_json::Value;
use spyglass_data::codec;
use spyglass_data::codec::{LoadError, LoadErrorKind};

/// `Climate.quantizeCoord`: `(long)(f * 10000.0f)`, an `f32` multiply, then truncation. Rust's
/// `as` saturates and maps NaN to 0, as Java's `(long)` does.
pub fn quantize(f: f32) -> i64 {
    (f * 10000.0) as i64
}

/// `Climate.Parameter`: a quantized `[min, max]` range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Parameter {
    /// The low end, inclusive.
    pub min: i64,
    /// The high end, inclusive.
    pub max: i64,
}

impl Parameter {
    /// The range as versions with the `climate_half_open` switch read it (ADR 0026): the data's
    /// `[min, max)`, which over integers is `[min, max - 1]`. Their `distance` is
    /// `(t - max) + 1` above the range, which is this range's `t - (max - 1)` in wrapping
    /// arithmetic, so fitness, the search's box distances and spawn fitness need no change. A
    /// single value becomes an empty range, never at distance 0, as in the game.
    pub fn half_open(self) -> Self {
        Parameter {
            min: self.min,
            max: self.max.wrapping_sub(1),
        }
    }

    /// `Climate.Parameter.distance(long)`: how far `target` is outside the range, 0 inside it.
    pub fn distance(self, target: i64) -> i64 {
        let above = target.wrapping_sub(self.max);
        let below = self.min.wrapping_sub(target);
        if above > 0 { above } else { below.max(0) }
    }

    /// `Climate.Parameter.CODEC` on data (not a report): a number, a `[min, max]` pair or a
    /// `{"min": .., "max": ..}` object (`ExtraCodecs.intervalCodec`). Each end is a float in
    /// `[-2, 2]` (`Codec.floatRange`), `min` mustn't be above `max` by `Float.compareTo`, and each
    /// is quantized with [`quantize`], truncating as the game does.
    pub fn from_codec(json: &Value) -> Result<Self, LoadErrorKind> {
        let float = |v: &Value| {
            let f = codec::f32_of(v).ok_or(LoadErrorKind::Shape(
                "a number, a [min, max] pair or a {min, max} object",
            ))?;
            // Float.compareTo, which total_cmp matches for every float JSON can spell.
            if f.total_cmp(&-2.0).is_lt() || f.total_cmp(&2.0).is_gt() {
                return Err(LoadErrorKind::OutOfRange {
                    value: v.to_string(),
                    min: "-2".into(),
                    max: "2".into(),
                });
            }
            Ok(f)
        };
        let (min, max) = match json {
            Value::Array(pair) => match pair.as_slice() {
                [min, max] => (float(min)?, float(max)?),
                _ => return Err(LoadErrorKind::Shape("a [min, max] pair")),
            },
            Value::Object(fields) => {
                let field = |name| fields.get(name).ok_or(LoadErrorKind::Missing(name));
                (float(field("min")?)?, float(field("max")?)?)
            }
            _ => {
                let f = float(json)?;
                (f, f)
            }
        };
        if min.total_cmp(&max).is_gt() {
            return Err(LoadErrorKind::Invalid("min is above max"));
        }
        Ok(Parameter {
            min: quantize(min),
            max: quantize(max),
        })
    }
}

/// A quantized climate: temperature, humidity, continentalness, erosion, depth, weirdness, in
/// `Climate.TargetPoint`'s order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TargetPoint(pub [i64; 6]);

impl TargetPoint {
    /// `Climate.target`: each value quantized.
    pub fn quantize(values: [f32; 6]) -> Self {
        TargetPoint(values.map(quantize))
    }
}

/// `Climate.ParameterPoint`: six ranges, in [`TargetPoint`]'s order, and an offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParameterPoint {
    /// The six climate ranges.
    pub parameters: [Parameter; 6],
    /// The quantized offset, added as if it were a seventh distance.
    pub offset: i64,
}

impl ParameterPoint {
    /// `Climate.ParameterPoint.CODEC` on data (not a report), as 26.2's `spawn_target` holds
    /// it: an object with the six parameters, each read by [`Parameter::from_codec`], and an
    /// `offset`, a float in `[0, 1]` (`Codec.floatRange`, by `Float.compareTo`) quantized with
    /// [`quantize`]. Other fields are ignored, as a record codec does. An error names the field
    /// it's in, or `None` for the point itself.
    pub fn from_codec(json: &Value) -> Result<Self, (Option<&'static str>, LoadErrorKind)> {
        let fields = json
            .as_object()
            .ok_or((None, LoadErrorKind::Shape("a parameter point")))?;
        let field =
            |name: &'static str| fields.get(name).ok_or((None, LoadErrorKind::Missing(name)));
        let mut parameters = [Parameter { min: 0, max: 0 }; 6];
        for (range, name) in parameters.iter_mut().zip(PARAMETERS) {
            *range = Parameter::from_codec(field(name)?).map_err(|kind| (Some(name), kind))?;
        }
        let at = |kind| (Some("offset"), kind);
        let value = field("offset")?;
        let offset = codec::f32_of(value).ok_or(at(LoadErrorKind::Shape("a number")))?;
        if offset.total_cmp(&0.0).is_lt() || offset.total_cmp(&1.0).is_gt() {
            return Err(at(LoadErrorKind::OutOfRange {
                value: value.to_string(),
                min: "0".into(),
                max: "1".into(),
            }));
        }
        Ok(ParameterPoint {
            parameters,
            offset: quantize(offset),
        })
    }

    /// `Climate.ParameterPoint.fitness`: the sum of the squared distances (`Mth.square`), plus
    /// the squared offset, in wrapping `long` arithmetic.
    pub fn fitness(&self, target: &TargetPoint) -> i64 {
        let square = |d: i64| d.wrapping_mul(d);
        self.parameters
            .iter()
            .zip(target.0)
            .fold(0i64, |sum, (p, t)| sum.wrapping_add(square(p.distance(t))))
            .wrapping_add(square(self.offset))
    }
}

/// A biome in a [`ParameterList`]: an index into its names, in order of first appearance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BiomeId(pub u16);

/// A biome source's biomes, by [`BiomeId`]: their ids, e.g. `minecraft:plains`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BiomeNames(Vec<String>);

impl BiomeNames {
    /// Names in [`BiomeId`] order.
    pub fn new(names: Vec<String>) -> Self {
        BiomeNames(names)
    }

    /// A biome's id. Panics if `biome` isn't one of these.
    pub fn name(&self, biome: BiomeId) -> &str {
        &self.0[usize::from(biome.0)]
    }

    /// Every biome id, by [`BiomeId`].
    pub fn names(&self) -> &[String] {
        &self.0
    }

    /// A biome's [`BiomeId`] by name, if it's one of these.
    pub fn id(&self, name: &str) -> Option<BiomeId> {
        self.0
            .iter()
            .position(|n| n == name)
            .map(|i| BiomeId(i as u16))
    }

    /// The [`BiomeId`] of `name`, added if it's new.
    fn intern(&mut self, name: &str) -> BiomeId {
        match self.id(name) {
            Some(id) => id,
            None => {
                self.0.push(name.to_string());
                BiomeId((self.0.len() - 1) as u16)
            }
        }
    }
}

/// `Climate.ParameterList`: parameter points with their biomes, in the game's order.
#[derive(Debug, Clone)]
pub struct ParameterList {
    points: Vec<ParameterPoint>,
    biomes: Vec<BiomeId>,
    names: BiomeNames,
}

/// The parameter names in a report entry, in [`TargetPoint`]'s order. The router's `vegetation`
/// is `humidity` here, and `ridges` is `weirdness`.
const PARAMETERS: [&str; 6] = [
    "temperature",
    "humidity",
    "continentalness",
    "erosion",
    "depth",
    "weirdness",
];

impl ParameterList {
    /// Reads a `biome_parameters` report: `{"biomes": [{"biome": id, "parameters": {...}}]}`,
    /// with each value written as `unquantizeCoord` of its long. See
    /// `docs/specs/biome-lookup.md`. `resource` names the report in errors.
    pub fn from_report(json: &Value, resource: &str) -> Result<Self, LoadError> {
        let error = |path: String, kind| LoadError::new(resource, path, kind);
        let entries = json
            .get("biomes")
            .ok_or_else(|| error(String::new(), LoadErrorKind::Missing("biomes")))?
            .as_array()
            .ok_or_else(|| error("biomes".into(), LoadErrorKind::Shape("a list of biomes")))?;
        if entries.is_empty() {
            return Err(error(
                "biomes".into(),
                LoadErrorKind::Invalid("a parameter list needs at least one point"),
            ));
        }
        let mut list = ParameterList {
            points: Vec::with_capacity(entries.len()),
            biomes: Vec::with_capacity(entries.len()),
            names: BiomeNames::default(),
        };
        for (i, entry) in entries.iter().enumerate() {
            let at = format!("biomes[{i}]");
            let biome = entry
                .get("biome")
                .and_then(Value::as_str)
                .ok_or_else(|| error(format!("{at}.biome"), LoadErrorKind::Shape("a biome id")))?;
            let parameters = entry
                .get("parameters")
                .ok_or_else(|| error(at.clone(), LoadErrorKind::Missing("parameters")))?;
            let field = |name: &'static str| {
                parameters
                    .get(name)
                    .ok_or_else(|| error(format!("{at}.parameters"), LoadErrorKind::Missing(name)))
            };
            let mut ranges = [Parameter { min: 0, max: 0 }; 6];
            for (range, name) in ranges.iter_mut().zip(PARAMETERS) {
                *range = parameter(field(name)?)
                    .map_err(|kind| error(format!("{at}.parameters.{name}"), kind))?;
            }
            // ParameterPoint.CODEC: offset is Codec.floatRange(0, 1), quantized.
            let offset = coordinate(field("offset")?, 0, 10_000)
                .map_err(|kind| error(format!("{at}.parameters.offset"), kind))?;
            if list.names.id(biome).is_none() && list.names.0.len() > usize::from(u16::MAX) {
                return Err(error(at, LoadErrorKind::Invalid("too many biomes")));
            }
            let BiomeId(id) = list.names.intern(biome);
            list.points.push(ParameterPoint {
                parameters: ranges,
                offset,
            });
            list.biomes.push(BiomeId(id));
        }
        Ok(list)
    }

    /// Every point's ranges made [half-open](Parameter::half_open), for versions with the
    /// `climate_half_open` switch.
    pub fn half_open(mut self) -> Self {
        for point in &mut self.points {
            point.parameters = point.parameters.map(Parameter::half_open);
        }
        self
    }

    /// The parameter points, in list order.
    pub fn points(&self) -> &[ParameterPoint] {
        &self.points
    }

    /// Each point's biome, in list order.
    pub fn biomes(&self) -> &[BiomeId] {
        &self.biomes
    }

    /// A biome's id, e.g. `minecraft:plains`.
    pub fn name(&self, biome: BiomeId) -> &str {
        self.names.name(biome)
    }

    /// Every biome id, by [`BiomeId`].
    pub fn names(&self) -> &[String] {
        self.names.names()
    }

    /// A biome's [`BiomeId`] by name, if it's in the list.
    pub fn id(&self, name: &str) -> Option<BiomeId> {
        self.names.id(name)
    }

    /// The biomes' names.
    pub fn biome_names(&self) -> &BiomeNames {
        &self.names
    }
}

/// A `Climate.Parameter`: a `[min, max]` pair, or one number for both. `Climate.Parameter.CODEC`
/// takes floats in `[-2, 2]` with `min <= max`.
fn parameter(json: &Value) -> Result<Parameter, LoadErrorKind> {
    match json {
        Value::Array(pair) => {
            let [min, max] = pair.as_slice() else {
                return Err(LoadErrorKind::Shape("a number or a [min, max] pair"));
            };
            let (min, max) = (
                coordinate(min, -20_000, 20_000)?,
                coordinate(max, -20_000, 20_000)?,
            );
            if min > max {
                return Err(LoadErrorKind::Invalid("min is above max"));
            }
            Ok(Parameter { min, max })
        }
        _ => {
            let v = coordinate(json, -20_000, 20_000)?;
            Ok(Parameter { min: v, max: v })
        }
    }
}

/// The quantized long behind a reported value: the value as `f32`, times 10000, rounded to the
/// nearest integer in `f64`. The game wrote `(float) q / 10000.0f`, and rounding inverts that
/// exactly for every `|q| <= 40000`, where the game's own truncating `quantizeCoord` doesn't.
/// `min` and `max` bound the long.
fn coordinate(json: &Value, min: i64, max: i64) -> Result<i64, LoadErrorKind> {
    let f = codec::f32_of(json).ok_or(LoadErrorKind::Shape("a number"))?;
    let scaled = f64::from(f) * 10000.0;
    if !(min as f64..=max as f64).contains(&scaled) {
        return Err(LoadErrorKind::OutOfRange {
            value: json.to_string(),
            min: (min as f64 / 10000.0).to_string(),
            max: (max as f64 / 10000.0).to_string(),
        });
    }
    Ok(scaled.round() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn number(text: &str) -> Value {
        serde_json::from_str(text).unwrap()
    }

    #[test]
    fn values_round_to_the_nearest_long() {
        // The game writes -16383 as (float) -16383 / 10000.0f, -1.6383, but -1.6383f * 10000.0f
        // truncates to -16382.
        assert_eq!(quantize(-1.6383), -16382);
        assert_eq!(
            coordinate(&number("-1.6383"), -20_000, 20_000).ok(),
            Some(-16383)
        );
        // Every long the codec range allows survives the report's round trip.
        for q in -20_000i64..=20_000 {
            let text = format!("{:?}", q as f32 / 10000.0);
            assert_eq!(
                coordinate(&number(&text), -20_000, 20_000).ok(),
                Some(q),
                "{text}"
            );
        }
        assert!(coordinate(&number("2.0001"), -20_000, 20_000).is_err());
        assert!(coordinate(&Value::from("0.5"), -20_000, 20_000).is_err());
    }

    #[test]
    fn quantize_truncates_like_java() {
        assert_eq!(quantize(-0.123_456), -1234);
        assert_eq!(quantize(0.999_99), 9999);
        assert_eq!(quantize(f32::NAN), 0);
        assert_eq!(quantize(f32::INFINITY), i64::MAX);
    }

    #[test]
    fn distance_and_fitness() {
        let p = Parameter { min: -100, max: 50 };
        assert_eq!(p.distance(-150), 50);
        assert_eq!(p.distance(-100), 0);
        assert_eq!(p.distance(0), 0);
        assert_eq!(p.distance(75), 25);
        let point = ParameterPoint {
            parameters: [p; 6],
            offset: 3,
        };
        let target = TargetPoint([-150, 0, 75, 50, 51, -101]);
        // 50² + 0 + 25² + 0 + 1² + 1² + 3².
        assert_eq!(point.fitness(&target), 2500 + 625 + 1 + 1 + 9);
    }

    /// 26.4's `Climate.Parameter.distance`, written as the game does: `(t - max) + 1` above.
    fn half_open_distance(p: Parameter, t: i64) -> i64 {
        let above = t.wrapping_sub(p.max).wrapping_add(1);
        let below = p.min.wrapping_sub(t);
        if above > 0 { above } else { below.max(0) }
    }

    #[test]
    fn half_open_ranges_are_the_games_distance() {
        let p = Parameter { min: -100, max: 50 };
        let h = p.half_open();
        assert_eq!((h.distance(49), h.distance(50), h.distance(75)), (0, 1, 26));
        assert_eq!((h.distance(-100), h.distance(-150)), (0, 50));
        // A single value is never at distance 0.
        let single = Parameter { min: 7, max: 7 }.half_open();
        assert_eq!(
            (single.distance(6), single.distance(7), single.distance(8)),
            (1, 1, 2)
        );
        for p in [
            p,
            Parameter { min: 7, max: 7 },
            Parameter {
                min: i64::MIN,
                max: i64::MAX,
            },
            Parameter {
                min: -20000,
                max: 20000,
            },
        ] {
            for t in [i64::MIN, -20001, -100, 0, 6, 7, 8, 50, 20000, i64::MAX] {
                assert_eq!(
                    p.half_open().distance(t),
                    half_open_distance(p, t),
                    "{p:?} at {t}"
                );
            }
        }
    }

    #[test]
    fn parameters_from_data_truncate() {
        let p = |text: &str| Parameter::from_codec(&number(text));
        assert_eq!(
            p("[-0.11, 1.0]").ok(),
            Some(Parameter {
                min: -1100,
                max: 10000
            })
        );
        assert_eq!(
            p("0.16").ok(),
            Some(Parameter {
                min: 1600,
                max: 1600
            })
        );
        assert_eq!(
            p(r#"{"min": -2, "max": 2, "extra": 0}"#).ok(),
            Some(Parameter {
                min: -20000,
                max: 20000
            })
        );
        // Unlike a report's values, data is quantized the game's way: -1.6383f * 10000.0f
        // truncates to -16382.
        assert_eq!(
            p("-1.6383").ok(),
            Some(Parameter {
                min: -16382,
                max: -16382
            })
        );
        assert!(matches!(p("2.0001"), Err(LoadErrorKind::OutOfRange { .. })));
        assert!(matches!(p("[0.5, -0.5]"), Err(LoadErrorKind::Invalid(_))));
        // Float.compareTo orders -0.0 below 0.0.
        assert!(p("[-0.0, 0.0]").is_ok());
        assert!(matches!(p("[0.0, -0.0]"), Err(LoadErrorKind::Invalid(_))));
        assert!(p("[0.5]").is_err());
        assert!(p(r#"{"min": 0.5}"#).is_err());
        assert!(p(r#""0.5""#).is_err());
    }

    #[test]
    fn reads_a_report() {
        let report = json!({"biomes": [
            {"biome": "minecraft:plains", "parameters": {
                "temperature": [-0.45, -0.15], "humidity": [-1.0, -0.35],
                "continentalness": [-0.11, 0.03], "erosion": [-0.7799, -0.375],
                "depth": 0.0, "weirdness": [-1.0, -0.9333], "offset": 0.0}},
            {"biome": "minecraft:desert", "parameters": {
                "temperature": 0.55, "humidity": [-1.0, 1.0], "continentalness": [-1.2, 1.0],
                "erosion": [-1.0, 1.0], "depth": 1.0, "weirdness": [-1.0, 1.0], "offset": 0.375}},
            {"biome": "minecraft:plains", "parameters": {
                "temperature": 0.0, "humidity": 0.0, "continentalness": 0.0,
                "erosion": 0.0, "depth": 0.0, "weirdness": 0.0, "offset": 1.0}}
        ]});
        let list = ParameterList::from_report(&report, "test").unwrap();
        assert_eq!(list.points().len(), 3);
        assert_eq!(list.names(), ["minecraft:plains", "minecraft:desert"]);
        assert_eq!(list.biomes(), [BiomeId(0), BiomeId(1), BiomeId(0)]);
        let p = list.points()[0].parameters;
        assert_eq!(
            p[0],
            Parameter {
                min: -4500,
                max: -1500
            }
        );
        assert_eq!(
            p[3],
            Parameter {
                min: -7799,
                max: -3750
            }
        );
        assert_eq!(
            p[5],
            Parameter {
                min: -10000,
                max: -9333
            }
        );
        assert_eq!(
            list.points()[1].parameters[0],
            Parameter {
                min: 5500,
                max: 5500
            }
        );
        assert_eq!(list.points()[1].offset, 3750);
        assert_eq!(list.points()[2].offset, 10000);
        assert_eq!(list.id("minecraft:desert"), Some(BiomeId(1)));
    }

    #[test]
    fn report_errors_name_the_path() {
        let bad = json!({"biomes": [{"biome": "minecraft:plains", "parameters": {
            "temperature": [0.5, -0.5], "humidity": 0, "continentalness": 0,
            "erosion": 0, "depth": 0, "weirdness": 0, "offset": 0}}]});
        let e = ParameterList::from_report(&bad, "r").unwrap_err();
        assert_eq!(e.path, "biomes[0].parameters.temperature");
        let missing = json!({"biomes": [{"biome": "minecraft:plains", "parameters": {}}]});
        let e = ParameterList::from_report(&missing, "r").unwrap_err();
        assert!(matches!(*e.kind, LoadErrorKind::Missing("temperature")));
        let offset = json!({"biomes": [{"biome": "minecraft:plains", "parameters": {
            "temperature": 0, "humidity": 0, "continentalness": 0,
            "erosion": 0, "depth": 0, "weirdness": 0, "offset": -0.5}}]});
        let e = ParameterList::from_report(&offset, "r").unwrap_err();
        assert_eq!(e.path, "biomes[0].parameters.offset");
        assert!(ParameterList::from_report(&json!({"biomes": []}), "r").is_err());
    }
}
