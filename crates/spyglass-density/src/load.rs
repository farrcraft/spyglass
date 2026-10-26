//! Loading density functions and noise parameters from JSON, mirroring the game's codecs
//! (ADR 0010). Each type cites the codec it follows.

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{Map, Value};
use spyglass_data::bundle::{Bundle, BundleError};
use spyglass_data::codec::{
    At, LoadErrorKind, LoadResult, double, field, float, full_id, int, out_of_range,
};
use spyglass_noise::{NormalNoiseParameters, Normalization};

use crate::function::{Axis, DensityFunction, Df, Gradient, Spline};

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
            // BinaryFunction: {"left", "right"}.
            "minecraft:add" => DensityFunction::Add(arg(self, "left")?, arg(self, "right")?),
            "minecraft:mul" => DensityFunction::Mul(arg(self, "left")?, arg(self, "right")?),
            // UnaryFunction and CacheFunction: {"input"}.
            "minecraft:abs" => DensityFunction::Abs(arg(self, "input")?),
            "minecraft:cache" => DensityFunction::Cache(arg(self, "input")?),
            // LerpFunction.
            "minecraft:lerp" => DensityFunction::Lerp {
                alpha: arg(self, "alpha")?,
                first: arg(self, "first")?,
                second: arg(self, "second")?,
            },
            "minecraft:gradient" => DensityFunction::Gradient(parse_gradient(obj, at)?),
            // SplineFunction: {"spline": CubicSpline}.
            "minecraft:spline" => {
                let spline = self.parse_spline(field(obj, "spline", at)?, &at.child("spline"))?;
                DensityFunction::Spline(Arc::new(spline))
            }
            // SimpleDensityFunction: no fields.
            "minecraft:blend_alpha" => DensityFunction::BlendAlpha,
            "minecraft:blend_offset" => DensityFunction::BlendOffset,
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
                Ok(Spline::Multipoint {
                    coordinate,
                    locations,
                    values,
                    derivatives,
                })
            }
            _ => Err(at.error(LoadErrorKind::Shape("a number or a spline object"))),
        }
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
    let axis_at = at.child("axis");
    let axis = match field(obj, "axis", at)?.as_str() {
        Some("x") => Axis::X,
        Some("y") => Axis::Y,
        Some("z") => Axis::Z,
        _ => return Err(axis_at.error(LoadErrorKind::Shape("\"x\", \"y\" or \"z\""))),
    };
    if let Some(tiling) = obj.get("tiling") {
        let name = tiling.as_str().unwrap_or_default();
        if name != "clamp_to_edge" {
            return Err(at.child("tiling").error(LoadErrorKind::Unsupported {
                what: "gradient tiling",
                name: tiling.to_string(),
            }));
        }
    }
    let gradient = Gradient {
        axis,
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
