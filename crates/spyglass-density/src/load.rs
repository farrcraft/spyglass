//! Loading density functions and noise parameters from JSON, mirroring the game's codecs
//! (ADR 0010). Each type cites the codec it follows.

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{Map, Value};
use spyglass_data::bundle::{Bundle, BundleError};
use spyglass_data::codec::{
    At, LoadErrorKind, LoadResult, double, field, float, full_id, int, int_field, out_of_range,
};
use spyglass_noise::{NormalNoiseParameters, Normalization};

use crate::function::{
    Axis, Binary, BlendedNoise, DensityFunction, Df, DistanceMetric, Gradient, Points, Round,
    Spline, Tiling, Unary,
};

/// Registry paths, as the game's data packs lay them out.
pub(crate) const DENSITY_FUNCTION: &str = "worldgen/density_function";
pub(crate) const NOISE: &str = "worldgen/noise";

/// Where resources come from: a [`Bundle`], or anything else that maps registry entries to JSON.
pub trait Resources {
    /// The JSON of the entry `id` in `registry` (a registry path, e.g. `worldgen/noise`).
    fn resource(&self, registry: &str, id: &str) -> Result<Value, BundleError>;
}

impl Resources for Bundle {
    fn resource(&self, registry: &str, id: &str) -> Result<Value, BundleError> {
        Bundle::resource(self, registry, id)
    }
}

/// Resources keyed `<registry>/<full id>`, e.g. `worldgen/noise/minecraft:temperature`: for
/// tests and other hand-built data.
impl Resources for HashMap<String, Value> {
    fn resource(&self, registry: &str, id: &str) -> Result<Value, BundleError> {
        let key = format!("{registry}/{id}");
        self.get(&key).cloned().ok_or(BundleError::NotFound(key))
    }
}

/// Loads named density functions and noises, sharing each one however often it's referenced.
pub struct Loader<'r, R: Resources + ?Sized> {
    resources: &'r R,
    functions: HashMap<String, Df>,
    noises: HashMap<String, NormalNoiseParameters>,
    /// Named functions being loaded, outermost first, for cycle detection.
    loading: Vec<String>,
}

impl<'r, R: Resources + ?Sized> Loader<'r, R> {
    /// A loader reading from `resources`.
    pub fn new(resources: &'r R) -> Self {
        Loader {
            resources,
            functions: HashMap::new(),
            noises: HashMap::new(),
            loading: Vec::new(),
        }
    }

    /// The named density function `id`, loaded once and shared.
    pub fn function(&mut self, id: &str) -> LoadResult<Df> {
        let id = full_id(id);
        if let Some(f) = self.functions.get(&id) {
            return Ok(f.clone());
        }
        if let Some(start) = self.loading.iter().position(|l| *l == id) {
            let mut cycle = self.loading[start..].to_vec();
            cycle.push(id.clone());
            return Err(At::root(&id).error(LoadErrorKind::Cycle(cycle)));
        }
        let at = At::root(&id);
        let json = self
            .resources
            .resource(DENSITY_FUNCTION, &id)
            .map_err(|e| at.error(e.into()))?;
        self.loading.push(id.clone());
        let parsed = self.parse_function(&json, &at);
        self.loading.pop();
        let f = parsed?;
        self.functions.insert(id, f.clone());
        Ok(f)
    }

    /// The noise `id`'s parameters, loaded once.
    pub fn noise(&mut self, id: &str) -> LoadResult<NormalNoiseParameters> {
        let id = full_id(id);
        if let Some(p) = self.noises.get(&id) {
            return Ok(p.clone());
        }
        let at = At::root(&id);
        let json = self
            .resources
            .resource(NOISE, &id)
            .map_err(|e| at.error(e.into()))?;
        let p = parse_noise(&json, &at)?;
        self.noises.insert(id, p.clone());
        Ok(p)
    }

    /// A density function written inline in another resource, such as a noise router entry:
    /// a number, a reference or an object, as anywhere else. Errors name `resource` and `path`.
    pub fn inline(&mut self, resource: &str, path: &str, json: &Value) -> LoadResult<Df> {
        let at = At::new(resource, path);
        self.parse_function(json, &at)
    }

    /// Every noise loaded so far, by full id.
    pub fn noises(&self) -> &HashMap<String, NormalNoiseParameters> {
        &self.noises
    }

    /// A density function value: a number (a constant), a string (a reference) or an object
    /// with a `type` (`DensityFunction.CODEC`).
    fn parse_function(&mut self, json: &Value, at: &At<'_>) -> LoadResult<Df> {
        match json {
            Value::Number(_) => Ok(Arc::new(DensityFunction::Constant(noise_value(json, at)?))),
            Value::String(id) => self.function(id),
            Value::Object(obj) => self.parse_typed(obj, at),
            _ => Err(at.error(LoadErrorKind::Shape("a number, a reference or an object"))),
        }
    }

    fn parse_typed(&mut self, obj: &Map<String, Value>, at: &At<'_>) -> LoadResult<Df> {
        let kind = obj
            .get("type")
            .ok_or_else(|| at.error(LoadErrorKind::Missing("type")))?
            .as_str()
            .ok_or_else(|| at.child("type").error(LoadErrorKind::Shape("a type id")))?;
        let kind = full_id(kind);
        let arg = |this: &mut Self, name: &'static str| -> LoadResult<Df> {
            let value = field(obj, name, at)?;
            this.parse_function(value, &at.child(name))
        };
        // The types registered from an enum's values, each id one constant.
        if let Some(op) = binary_type(&kind) {
            // BinaryFunction: {"left", "right"}.
            let f = DensityFunction::Binary(op, arg(self, "left")?, arg(self, "right")?);
            return Ok(Arc::new(f));
        }
        if let Some(op) = unary_type(&kind) {
            // UnaryFunction: {"input"}.
            return Ok(Arc::new(DensityFunction::Unary(op, arg(self, "input")?)));
        }
        if let Some(kind) = round_type(&kind) {
            // RoundFunction: {"input", "multiple" (default the constant 1)}.
            let input = arg(self, "input")?;
            let multiple = match obj.get("multiple") {
                Some(v) => self.parse_function(v, &at.child("multiple"))?,
                None => Arc::new(DensityFunction::Constant(1.0)),
            };
            return Ok(Arc::new(DensityFunction::Round {
                kind,
                input,
                multiple,
            }));
        }
        let f = match kind.as_str() {
            // ConstantFunction.CODEC: {"value": NOISE_VALUE_CODEC}.
            "minecraft:constant" => DensityFunction::Constant(noise_value(
                field(obj, "value", at)?,
                &at.child("value"),
            )?),
            // NoiseFunction.CODEC.
            "minecraft:noise" => {
                let noise = self.noise_ref(obj, at)?;
                let xz_scale = double(field(obj, "xz_scale", at)?, &at.child("xz_scale"))?;
                let y_scale = double(field(obj, "y_scale", at)?, &at.child("y_scale"))?;
                let shift = |this: &mut Self, name: &'static str| -> LoadResult<Df> {
                    match obj.get(name) {
                        Some(v) => this.parse_function(v, &at.child(name)),
                        None => Ok(Arc::new(DensityFunction::Constant(0.0))),
                    }
                };
                DensityFunction::Noise {
                    noise,
                    xz_scale,
                    y_scale,
                    shift_x: shift(self, "shift_x")?,
                    shift_y: shift(self, "shift_y")?,
                    shift_z: shift(self, "shift_z")?,
                }
            }
            // ShiftNoiseFunction.*: {"noise": ...}.
            "minecraft:shift_a" => DensityFunction::ShiftA {
                noise: self.noise_ref(obj, at)?,
            },
            "minecraft:shift_b" => DensityFunction::ShiftB {
                noise: self.noise_ref(obj, at)?,
            },
            "minecraft:shift" => DensityFunction::Shift {
                noise: self.noise_ref(obj, at)?,
            },
            // CacheFunction and BlendDensityFunction: {"input"}.
            "minecraft:cache" => DensityFunction::Cache(arg(self, "input")?),
            "minecraft:blend_density" => DensityFunction::BlendDensity(arg(self, "input")?),
            // ClampFunction, validated: max must not be below min.
            "minecraft:clamp" => {
                let input = arg(self, "input")?;
                let min = noise_value(field(obj, "min", at)?, &at.child("min"))?;
                let max = noise_value(field(obj, "max", at)?, &at.child("max"))?;
                if max < min {
                    return Err(at.error(LoadErrorKind::Invalid("max is below min")));
                }
                DensityFunction::Clamp { input, min, max }
            }
            "minecraft:pow" => DensityFunction::Pow {
                base: arg(self, "base")?,
                exponent: arg(self, "exponent")?,
            },
            // LerpFunction.
            "minecraft:lerp" => DensityFunction::Lerp {
                alpha: arg(self, "alpha")?,
                first: arg(self, "first")?,
                second: arg(self, "second")?,
            },
            "minecraft:range_choice" => DensityFunction::RangeChoice {
                input: arg(self, "input")?,
                min_inclusive: noise_value(
                    field(obj, "min_inclusive", at)?,
                    &at.child("min_inclusive"),
                )?,
                max_exclusive: noise_value(
                    field(obj, "max_exclusive", at)?,
                    &at.child("max_exclusive"),
                )?,
                when_in_range: arg(self, "when_in_range")?,
                when_out_of_range: arg(self, "when_out_of_range")?,
            },
            "minecraft:interval_select" => self.parse_interval_select(obj, at)?,
            // SliceFunction: {"axis", "coordinate", "input"}.
            "minecraft:slice" => DensityFunction::Slice {
                axis: parse_axis(obj, at)?,
                coordinate: int(field(obj, "coordinate", at)?, &at.child("coordinate"))?,
                input: arg(self, "input")?,
            },
            // InterpolatedFunction: cell sizes are ExtraCodecs.POSITIVE_INT.
            "minecraft:interpolated" => DensityFunction::Interpolated {
                input: arg(self, "input")?,
                cell_size_xz: int_field(obj, "cell_size_xz", (1, i32::MAX), None, at)?,
                cell_size_y: int_field(obj, "cell_size_y", (1, i32::MAX), None, at)?,
            },
            // FindTopSurfaceFunction: lower_bound within twice DimensionType's y range.
            "minecraft:find_top_surface" => DensityFunction::FindTopSurface {
                density: arg(self, "density")?,
                upper_bound: arg(self, "upper_bound")?,
                lower_bound: int_field(
                    obj,
                    "lower_bound",
                    (2 * DIMENSION_MIN_Y, 2 * DIMENSION_MAX_Y),
                    None,
                    at,
                )?,
                cell_height: int_field(obj, "cell_height", (1, i32::MAX), None, at)?,
            },
            "minecraft:old_blended_noise" => {
                DensityFunction::OldBlendedNoise(parse_blended_noise(obj, at)?)
            }
            "minecraft:gradient" => DensityFunction::Gradient(parse_gradient(obj, at)?),
            // SplineFunction: {"spline": CubicSpline}.
            "minecraft:spline" => {
                let spline = self.parse_spline(field(obj, "spline", at)?, &at.child("spline"))?;
                DensityFunction::Spline(Arc::new(spline))
            }
            // SimpleDensityFunction: no fields.
            "minecraft:blend_alpha" => DensityFunction::BlendAlpha,
            "minecraft:blend_offset" => DensityFunction::BlendOffset,
            "minecraft:beardifier" => DensityFunction::Beardifier,
            // The End's: DistanceToPointFunction ({"point": Vec3i, "metric"}) and
            // EndIslandFunction (no fields).
            "minecraft:distance_to_point" => DensityFunction::DistanceToPoint {
                point: parse_vec3i(field(obj, "point", at)?, &at.child("point"))?,
                metric: parse_metric(obj, at)?,
            },
            "minecraft:end_outer_islands" => DensityFunction::EndOuterIslands,
            _ => {
                return Err(at.child("type").error(LoadErrorKind::Unsupported {
                    what: "density function type",
                    name: kind,
                }));
            }
        };
        Ok(Arc::new(f))
    }

    /// A `noise` field: a noise id, loaded so it can be bound later. The game can't use an
    /// inline noise here either (`RandomState` needs the key), so only ids are accepted.
    fn noise_ref(&mut self, obj: &Map<String, Value>, at: &At<'_>) -> LoadResult<String> {
        let at = at.child("noise");
        let id = field(obj, "noise", &at)?
            .as_str()
            .ok_or_else(|| at.error(LoadErrorKind::Shape("a noise id")))?;
        let id = full_id(id);
        self.noise(&id)?;
        Ok(id)
    }

    /// `CubicSpline.codec`: a number (a constant) or `{"coordinate", "points"}`, each point
    /// `{"location", "value", "derivative"}`.
    fn parse_spline(&mut self, json: &Value, at: &At<'_>) -> LoadResult<Spline> {
        match json {
            Value::Number(_) => Ok(Spline::Constant(float(json, at)?)),
            Value::Object(obj) => {
                let coordinate =
                    self.parse_function(field(obj, "coordinate", at)?, &at.child("coordinate"))?;
                let points_at = at.child("points");
                let points = field(obj, "points", at)?
                    .as_array()
                    .ok_or_else(|| points_at.error(LoadErrorKind::Shape("a list of points")))?;
                if points.is_empty() {
                    return Err(points_at.error(LoadErrorKind::Invalid("a spline needs points")));
                }
                let (mut locations, mut values, mut derivatives) =
                    (Vec::new(), Vec::new(), Vec::new());
                for (i, point) in points.iter().enumerate() {
                    let at = points_at.child(&format!("[{i}]"));
                    let point = point
                        .as_object()
                        .ok_or_else(|| at.error(LoadErrorKind::Shape("a point object")))?;
                    locations.push(float(
                        field(point, "location", &at)?,
                        &at.child("location"),
                    )?);
                    values
                        .push(self.parse_spline(field(point, "value", &at)?, &at.child("value"))?);
                    derivatives.push(float(
                        field(point, "derivative", &at)?,
                        &at.child("derivative"),
                    )?);
                }
                let points = Points::new(coordinate, locations, values, derivatives)
                    .expect("one location, value and derivative per point, and at least one");
                Ok(Spline::Multipoint(points))
            }
            _ => Err(at.error(LoadErrorKind::Shape("a number or a spline object"))),
        }
    }

    /// `IntervalSelectFunction.CODEC`: `{"input", "thresholds", "functions"}`, validated: one
    /// more function than thresholds (at least two), and thresholds in `Float.compare` order.
    fn parse_interval_select(
        &mut self,
        obj: &Map<String, Value>,
        at: &At<'_>,
    ) -> LoadResult<DensityFunction> {
        let input = self.parse_function(field(obj, "input", at)?, &at.child("input"))?;
        let thresholds_at = at.child("thresholds");
        let thresholds = field(obj, "thresholds", at)?
            .as_array()
            .ok_or_else(|| thresholds_at.error(LoadErrorKind::Shape("a list of numbers")))?
            .iter()
            .enumerate()
            .map(|(i, v)| noise_value(v, &thresholds_at.child(&format!("[{i}]"))))
            .collect::<LoadResult<Vec<f32>>>()?;
        let functions_at = at.child("functions");
        let list = field(obj, "functions", at)?
            .as_array()
            .ok_or_else(|| functions_at.error(LoadErrorKind::Shape("a list of functions")))?;
        if list.len() < 2 {
            return Err(functions_at.error(LoadErrorKind::Invalid("at least two functions")));
        }
        let functions = list
            .iter()
            .enumerate()
            .map(|(i, v)| self.parse_function(v, &functions_at.child(&format!("[{i}]"))))
            .collect::<LoadResult<Vec<Df>>>()?;
        if thresholds.len() != functions.len() - 1 {
            return Err(at.error(LoadErrorKind::Invalid(
                "there must be one fewer threshold than functions",
            )));
        }
        if !thresholds.is_sorted_by(|a, b| a.total_cmp(b).is_le()) {
            return Err(at.error(LoadErrorKind::Invalid(
                "threshold values must be ordered from smallest to largest",
            )));
        }
        Ok(DensityFunction::IntervalSelect {
            input,
            thresholds,
            functions,
        })
    }
}

/// `DimensionType.MIN_Y` and `MAX_Y`: the y range block positions can pack.
const DIMENSION_MIN_Y: i32 = -2032;
const DIMENSION_MAX_Y: i32 = 2031;

/// The ids `BinaryFunction.Type` registers.
fn binary_type(kind: &str) -> Option<Binary> {
    Some(match kind {
        "minecraft:add" => Binary::Add,
        "minecraft:sub" => Binary::Sub,
        "minecraft:mul" => Binary::Mul,
        "minecraft:div" => Binary::Div,
        "minecraft:min" => Binary::Min,
        "minecraft:max" => Binary::Max,
        _ => return None,
    })
}

/// The ids `UnaryFunction.Type` registers.
fn unary_type(kind: &str) -> Option<Unary> {
    Some(match kind {
        "minecraft:abs" => Unary::Abs,
        "minecraft:square" => Unary::Square,
        "minecraft:cube" => Unary::Cube,
        "minecraft:sqrt" => Unary::Sqrt,
        "minecraft:half_negative" => Unary::HalfNegative,
        "minecraft:quarter_negative" => Unary::QuarterNegative,
        "minecraft:reciprocal" => Unary::Reciprocal,
        "minecraft:negate" => Unary::Negate,
        "minecraft:squeeze" => Unary::Squeeze,
        "minecraft:log" => Unary::Log,
        "minecraft:sign" => Unary::Sign,
        _ => return None,
    })
}

/// The ids `RoundFunction.Type` registers.
fn round_type(kind: &str) -> Option<Round> {
    Some(match kind {
        "minecraft:floor" => Round::Floor,
        "minecraft:round" => Round::Round,
        "minecraft:ceil" => Round::Ceil,
        "minecraft:truncate" => Round::Truncate,
        _ => return None,
    })
}

/// An `axis` field: `Direction.Axis.CODEC`.
fn parse_axis(obj: &Map<String, Value>, at: &At<'_>) -> LoadResult<Axis> {
    match field(obj, "axis", at)?.as_str() {
        Some("x") => Ok(Axis::X),
        Some("y") => Ok(Axis::Y),
        Some("z") => Ok(Axis::Z),
        _ => Err(at
            .child("axis")
            .error(LoadErrorKind::Shape("\"x\", \"y\" or \"z\""))),
    }
}

/// `BlendedNoise.CODEC`: four scales in `[0.001, 1000]` and a smear multiplier in `[1, 8]`.
fn parse_blended_noise(obj: &Map<String, Value>, at: &At<'_>) -> LoadResult<BlendedNoise> {
    let ranged = |name: &'static str, min: f64, max: f64| -> LoadResult<f64> {
        let json = field(obj, name, at)?;
        let v = double(json, &at.child(name))?;
        if (min..=max).contains(&v) {
            Ok(v)
        } else {
            Err(at.child(name).error(out_of_range(json, min, max)))
        }
    };
    Ok(BlendedNoise {
        xz_scale: ranged("xz_scale", 0.001, 1000.0)?,
        y_scale: ranged("y_scale", 0.001, 1000.0)?,
        xz_factor: ranged("xz_factor", 0.001, 1000.0)?,
        y_factor: ranged("y_factor", 0.001, 1000.0)?,
        smear_scale_multiplier: ranged("smear_scale_multiplier", 1.0, 8.0)?,
    })
}

/// `Vec3i.CODEC`: a list of exactly three ints.
fn parse_vec3i(json: &Value, at: &At<'_>) -> LoadResult<[i32; 3]> {
    let list = json
        .as_array()
        .filter(|l| l.len() == 3)
        .ok_or_else(|| at.error(LoadErrorKind::Shape("a list of 3 ints")))?;
    Ok([
        int(&list[0], &at.child("[0]"))?,
        int(&list[1], &at.child("[1]"))?,
        int(&list[2], &at.child("[2]"))?,
    ])
}

/// A `metric` field: `DistanceMetric.CODEC`.
fn parse_metric(obj: &Map<String, Value>, at: &At<'_>) -> LoadResult<DistanceMetric> {
    match field(obj, "metric", at)?.as_str() {
        Some("euclidean") => Ok(DistanceMetric::Euclidean),
        Some("euclidean_squared") => Ok(DistanceMetric::EuclideanSquared),
        Some("manhattan") => Ok(DistanceMetric::Manhattan),
        Some("chebyshev") => Ok(DistanceMetric::Chebyshev),
        _ => Err(at.child("metric").error(LoadErrorKind::Shape(
            "\"euclidean\", \"euclidean_squared\", \"manhattan\" or \"chebyshev\"",
        ))),
    }
}

/// `DensityFunctions.NOISE_VALUE_CODEC`: `Codec.floatRange(-1000000, 1000000)`.
fn noise_value(json: &Value, at: &At<'_>) -> LoadResult<f32> {
    let v = float(json, at)?;
    if (-1_000_000.0..=1_000_000.0).contains(&v) {
        Ok(v)
    } else {
        Err(at.error(out_of_range(json, -1_000_000, 1_000_000)))
    }
}

/// `GradientFunction.CODEC`, which also rejects equal coordinates.
fn parse_gradient(obj: &Map<String, Value>, at: &At<'_>) -> LoadResult<Gradient> {
    let axis = parse_axis(obj, at)?;
    // TilingMode.CODEC, optional with clamp_to_edge as the default.
    let tiling = match obj.get("tiling").map(|t| t.as_str()) {
        None | Some(Some("clamp_to_edge")) => Tiling::ClampToEdge,
        Some(Some("repeat")) => Tiling::Repeat,
        Some(Some("mirrored_repeat")) => Tiling::MirroredRepeat,
        Some(_) => {
            return Err(at.child("tiling").error(LoadErrorKind::Shape(
                "\"clamp_to_edge\", \"repeat\" or \"mirrored_repeat\"",
            )));
        }
    };
    let gradient = Gradient {
        axis,
        tiling,
        from_coordinate: int(
            field(obj, "from_coordinate", at)?,
            &at.child("from_coordinate"),
        )?,
        to_coordinate: int(field(obj, "to_coordinate", at)?, &at.child("to_coordinate"))?,
        from_value: noise_value(field(obj, "from_value", at)?, &at.child("from_value"))?,
        to_value: noise_value(field(obj, "to_value", at)?, &at.child("to_value"))?,
    };
    if gradient.from_coordinate == gradient.to_coordinate {
        return Err(at.error(LoadErrorKind::Invalid(
            "from_coordinate cannot be equal to to_coordinate",
        )));
    }
    Ok(gradient)
}

/// `NormalNoise.Parameters.CODEC`, with its defaults and `validate`.
fn parse_noise(json: &Value, at: &At<'_>) -> LoadResult<NormalNoiseParameters> {
    let obj = json
        .as_object()
        .ok_or_else(|| at.error(LoadErrorKind::Shape("a noise object")))?;
    let ranged_int = |name: &'static str, min: i32, max: i32| -> LoadResult<Option<i32>> {
        obj.get(name)
            .map(|v| {
                let i = int(v, &at.child(name))?;
                if (min..=max).contains(&i) {
                    Ok(i)
                } else {
                    Err(at.child(name).error(out_of_range(v, min, max)))
                }
            })
            .transpose()
    };
    let base_octave = ranged_int("base_octave", -32, 32)?
        .ok_or_else(|| at.error(LoadErrorKind::Missing("base_octave")))?;
    let mut p = NormalNoiseParameters::new(base_octave);
    if let Some(n) = ranged_int("octave_count", 1, 32)? {
        p.octave_count = n;
    }
    if let Some(v) = obj.get("base_amplitude") {
        let a = double(v, &at.child("base_amplitude"))?;
        // Codec.doubleRange(9.999999747378752E-6, 1000000).
        if !(9.999_999_747_378_752e-6..=1_000_000.0).contains(&a) {
            return Err(at.child("base_amplitude").error(out_of_range(
                v,
                9.999_999_747_378_752e-6,
                1_000_000,
            )));
        }
        p.base_amplitude = a;
    }
    if let Some(v) = obj.get("normalize") {
        // Codec.either(BOOL, "legacy"): true is enabled, false disabled.
        p.normalize = match v {
            Value::Bool(true) => Normalization::Enabled,
            Value::Bool(false) => Normalization::Disabled,
            Value::String(s) if s == "legacy" => Normalization::Legacy,
            _ => {
                return Err(at
                    .child("normalize")
                    .error(LoadErrorKind::Shape("true, false or \"legacy\"")));
            }
        };
    }
    if let Some(v) = obj.get("amplitude_modifiers") {
        let list_at = at.child("amplitude_modifiers");
        let list = v
            .as_array()
            .ok_or_else(|| list_at.error(LoadErrorKind::Shape("a list of numbers")))?;
        if list.len() > 32 {
            return Err(list_at.error(LoadErrorKind::Invalid("at most 32 amplitude modifiers")));
        }
        p.amplitude_modifiers = list
            .iter()
            .enumerate()
            .map(|(i, m)| {
                let at = list_at.child(&format!("[{i}]"));
                let d = double(m, &at)?;
                // Codec.doubleRange(0, 1000000).
                if (0.0..=1_000_000.0).contains(&d) {
                    Ok(d)
                } else {
                    Err(at.error(out_of_range(m, 0, 1_000_000)))
                }
            })
            .collect::<LoadResult<_>>()?;
    }
    // Parameters.validate.
    if !p.amplitude_modifiers.is_empty() && p.amplitude_modifiers.len() != p.octave_count as usize {
        return Err(at.error(LoadErrorKind::Invalid(
            "amplitude_modifiers must be empty or have octave_count entries",
        )));
    }
    Ok(p)
}
