//! Loading 26.2's density functions, as its codecs read them (`levelgen.DensityFunctions`):
//! `argument`, `argument1` and `argument2` fields, `f64` constants, and 26.2's own type ids.
//! Noises load as for every version (`crate::load::parse_noise`).

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{Map, Value};
use spyglass_data::codec::{
    At, LoadErrorKind, LoadResult, double, field, float, full_id, int_field, out_of_range,
};
use spyglass_noise::NormalNoiseParameters;

use super::function::{DensityFunction, Df, Mapped, Marker, Spline, TwoArgument};
use crate::load::{
    DENSITY_FUNCTION, DIMENSION_MAX_Y, DIMENSION_MIN_Y, NOISE, Resources, parse_blended_noise,
    parse_noise,
};

/// Loads 26.2's named density functions and noises, sharing each one however often it's
/// referenced.
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

    /// The named density function `id`, loaded once and shared: its value, not a
    /// [`DensityFunction::Reference`] to it.
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

    /// A density function written inline in another resource, such as a noise router entry.
    /// Errors name `resource` and `path`.
    pub fn inline(&mut self, resource: &str, path: &str, json: &Value) -> LoadResult<Df> {
        let at = At::new(resource, path);
        self.parse_function(json, &at)
    }

    /// Every noise loaded so far, by full id.
    pub fn noises(&self) -> &HashMap<String, NormalNoiseParameters> {
        &self.noises
    }

    /// `DensityFunction.CODEC`: a number (a constant), a string (a reference) or an object with
    /// a `type`.
    fn parse_function(&mut self, json: &Value, at: &At<'_>) -> LoadResult<Df> {
        match json {
            Value::Number(_) => Ok(Arc::new(DensityFunction::Constant(noise_value(json, at)?))),
            Value::String(id) => {
                let function = self.function(id)?;
                Ok(Arc::new(DensityFunction::Reference {
                    id: full_id(id),
                    function,
                }))
            }
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
        let value = |name: &'static str| noise_value(field(obj, name, at)?, &at.child(name));
        if let Some(op) = two_argument_type(&kind) {
            let f =
                DensityFunction::TwoArgument(op, arg(self, "argument1")?, arg(self, "argument2")?);
            return Ok(Arc::new(f));
        }
        if let Some(op) = mapped_type(&kind) {
            return Ok(Arc::new(DensityFunction::Mapped(
                op,
                arg(self, "argument")?,
            )));
        }
        if let Some(marker) = marker_type(&kind) {
            return Ok(Arc::new(DensityFunction::Marker(
                marker,
                arg(self, "argument")?,
            )));
        }
        let f = match kind.as_str() {
            "minecraft:constant" => DensityFunction::Constant(value("argument")?),
            // Noise.CODEC: the scales are plain Codec.DOUBLE, both required.
            "minecraft:noise" => DensityFunction::Noise {
                noise: self.noise_ref(obj, "noise", at)?,
                xz_scale: double(field(obj, "xz_scale", at)?, &at.child("xz_scale"))?,
                y_scale: double(field(obj, "y_scale", at)?, &at.child("y_scale"))?,
            },
            "minecraft:shifted_noise" => DensityFunction::ShiftedNoise {
                shift_x: arg(self, "shift_x")?,
                shift_y: arg(self, "shift_y")?,
                shift_z: arg(self, "shift_z")?,
                xz_scale: double(field(obj, "xz_scale", at)?, &at.child("xz_scale"))?,
                y_scale: double(field(obj, "y_scale", at)?, &at.child("y_scale"))?,
                noise: self.noise_ref(obj, "noise", at)?,
            },
            // ShiftA, ShiftB, Shift: the noise is the single `argument`.
            "minecraft:shift_a" => DensityFunction::ShiftA {
                noise: self.noise_ref(obj, "argument", at)?,
            },
            "minecraft:shift_b" => DensityFunction::ShiftB {
                noise: self.noise_ref(obj, "argument", at)?,
            },
            "minecraft:shift" => DensityFunction::Shift {
                noise: self.noise_ref(obj, "argument", at)?,
            },
            // YClampedGradient: from_y and to_y within twice DimensionType's y range.
            "minecraft:y_clamped_gradient" => DensityFunction::YClampedGradient {
                from_y: int_field(obj, "from_y", Y_RANGE, None, at)?,
                to_y: int_field(obj, "to_y", Y_RANGE, None, at)?,
                from_value: value("from_value")?,
                to_value: value("to_value")?,
            },
            // Clamp: {input, min, max}, noise values, unvalidated.
            "minecraft:clamp" => DensityFunction::Clamp {
                input: arg(self, "input")?,
                min: value("min")?,
                max: value("max")?,
            },
            "minecraft:range_choice" => DensityFunction::RangeChoice {
                input: arg(self, "input")?,
                min_inclusive: value("min_inclusive")?,
                max_exclusive: value("max_exclusive")?,
                when_in_range: arg(self, "when_in_range")?,
                when_out_of_range: arg(self, "when_out_of_range")?,
            },
            "minecraft:interval_select" => self.parse_interval_select(obj, at)?,
            "minecraft:spline" => {
                let spline = self.parse_spline(field(obj, "spline", at)?, &at.child("spline"))?;
                DensityFunction::Spline(Arc::new(spline))
            }
            "minecraft:find_top_surface" => DensityFunction::FindTopSurface {
                density: arg(self, "density")?,
                upper_bound: arg(self, "upper_bound")?,
                lower_bound: int_field(obj, "lower_bound", Y_RANGE, None, at)?,
                cell_height: int_field(obj, "cell_height", (1, i32::MAX), None, at)?,
            },
            "minecraft:old_blended_noise" => {
                DensityFunction::OldBlendedNoise(parse_blended_noise(obj, at)?)
            }
            // Unit codecs.
            "minecraft:end_islands" => DensityFunction::EndIslands,
            "minecraft:blend_alpha" => DensityFunction::BlendAlpha,
            "minecraft:blend_offset" => DensityFunction::BlendOffset,
            "minecraft:beardifier" => DensityFunction::Beardifier,
            _ => {
                return Err(at.child("type").error(LoadErrorKind::Unsupported {
                    what: "26.2 density function type",
                    name: kind,
                }));
            }
        };
        Ok(Arc::new(f))
    }

    /// A noise id field, loaded so it can be bound later.
    fn noise_ref(
        &mut self,
        obj: &Map<String, Value>,
        name: &'static str,
        at: &At<'_>,
    ) -> LoadResult<String> {
        let at = at.child(name);
        let id = field(obj, name, &at)?
            .as_str()
            .ok_or_else(|| at.error(LoadErrorKind::Shape("a noise id")))?;
        let id = full_id(id);
        self.noise(&id)?;
        Ok(id)
    }

    /// `CubicSpline.codec`, as 26.3's: a number or `{"coordinate", "points"}`.
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

    /// `IntervalSelect`: `{"input", "thresholds", "functions"}`, one more function than
    /// thresholds, thresholds ascending (`Comparators.isInOrder`, so equal neighbours pass).
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
            .collect::<LoadResult<Vec<f64>>>()?;
        let functions_at = at.child("functions");
        let functions = field(obj, "functions", at)?
            .as_array()
            .ok_or_else(|| functions_at.error(LoadErrorKind::Shape("a list of functions")))?
            .iter()
            .enumerate()
            .map(|(i, v)| self.parse_function(v, &functions_at.child(&format!("[{i}]"))))
            .collect::<LoadResult<Vec<Df>>>()?;
        if thresholds.len() + 1 != functions.len() {
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

/// `intRange(DimensionType.MIN_Y * 2, DimensionType.MAX_Y * 2)`.
const Y_RANGE: (i32, i32) = (2 * DIMENSION_MIN_Y, 2 * DIMENSION_MAX_Y);

/// The ids `TwoArgumentSimpleFunction.Type` registers.
fn two_argument_type(kind: &str) -> Option<TwoArgument> {
    Some(match kind {
        "minecraft:add" => TwoArgument::Add,
        "minecraft:mul" => TwoArgument::Mul,
        "minecraft:min" => TwoArgument::Min,
        "minecraft:max" => TwoArgument::Max,
        _ => return None,
    })
}

/// The ids `Mapped.Type` registers.
fn mapped_type(kind: &str) -> Option<Mapped> {
    Some(match kind {
        "minecraft:abs" => Mapped::Abs,
        "minecraft:square" => Mapped::Square,
        "minecraft:cube" => Mapped::Cube,
        "minecraft:half_negative" => Mapped::HalfNegative,
        "minecraft:quarter_negative" => Mapped::QuarterNegative,
        "minecraft:invert" => Mapped::Invert,
        "minecraft:squeeze" => Mapped::Squeeze,
        _ => return None,
    })
}

/// The ids `Marker.Type` registers.
fn marker_type(kind: &str) -> Option<Marker> {
    Some(match kind {
        "minecraft:interpolated" => Marker::Interpolated,
        "minecraft:flat_cache" => Marker::FlatCache,
        "minecraft:cache_2d" => Marker::Cache2D,
        "minecraft:cache_once" => Marker::CacheOnce,
        "minecraft:cache_all_in_cell" => Marker::CacheAllInCell,
        "minecraft:blend_density" => Marker::BlendDensity,
        _ => return None,
    })
}

/// `DensityFunctions.NOISE_VALUE_CODEC` in 26.2: `Codec.doubleRange(-1000000, 1000000)`.
fn noise_value(json: &Value, at: &At<'_>) -> LoadResult<f64> {
    let v = double(json, at)?;
    if (-1_000_000.0..=1_000_000.0).contains(&v) {
        Ok(v)
    } else {
        Err(at.error(out_of_range(json, -1_000_000, 1_000_000)))
    }
}
