//! 26.2's density functions as loaded: the 1.21 family's types (`levelgen.DensityFunctions`),
//! seed-independent. See `docs/specs/density-functions.md#262-types`.

use std::sync::Arc;

pub use crate::function::BlendedNoise;

/// A shared node. A named function referenced from several places is one node behind each
/// [`DensityFunction::Reference`].
pub type Df = Arc<DensityFunction>;

/// A 26.2 density function. Types follow the game's codec names.
#[derive(Debug, Clone, PartialEq)]
pub enum DensityFunction {
    /// `constant`, or a bare number: `DensityFunctions.Constant`, an `f64`.
    Constant(f64),
    /// A reference to a named function (`DensityFunctions.HolderHolder`). It's transparent, but
    /// not a `Constant` even when the function is one, until something strips references
    /// (the `NoiseChunk` and the climate sampler do).
    Reference {
        /// The function's full id.
        id: String,
        /// The function.
        function: Df,
    },
    /// `add`, `mul`, `min` and `max`: `TwoArgumentSimpleFunction`, with `argument1` and
    /// `argument2`.
    TwoArgument(TwoArgument, Df, Df),
    /// `abs`, `square`, `cube`, `half_negative`, `quarter_negative`, `invert` and `squeeze`:
    /// `DensityFunctions.Mapped`.
    Mapped(Mapped, Df),
    /// `clamp`: `Mth.clamp`, in `f64`.
    Clamp {
        /// `input`.
        input: Df,
        /// `min`.
        min: f64,
        /// `max`.
        max: f64,
    },
    /// `noise`: no shifts.
    Noise {
        /// The noise's id.
        noise: String,
        /// `xz_scale`.
        xz_scale: f64,
        /// `y_scale`.
        y_scale: f64,
    },
    /// `shifted_noise`.
    ShiftedNoise {
        /// The noise's id.
        noise: String,
        /// `xz_scale`.
        xz_scale: f64,
        /// `y_scale`.
        y_scale: f64,
        /// `shift_x`.
        shift_x: Df,
        /// `shift_y`.
        shift_y: Df,
        /// `shift_z`.
        shift_z: Df,
    },
    /// `shift_a`: the noise at `(x/4, 0, z/4)`, times 4.
    ShiftA {
        /// The noise's id (`argument`).
        noise: String,
    },
    /// `shift_b`: the noise at `(z/4, x/4, 0)`, times 4.
    ShiftB {
        /// The noise's id (`argument`).
        noise: String,
    },
    /// `shift`: the noise at `(x/4, y/4, z/4)`, times 4.
    Shift {
        /// The noise's id (`argument`).
        noise: String,
    },
    /// `y_clamped_gradient`: `Mth.clampedMap(y, from_y, to_y, from_value, to_value)`.
    YClampedGradient {
        /// `from_y`.
        from_y: i32,
        /// `to_y`.
        to_y: i32,
        /// `from_value`.
        from_value: f64,
        /// `to_value`.
        to_value: f64,
    },
    /// `range_choice`.
    RangeChoice {
        /// `input`.
        input: Df,
        /// `min_inclusive`.
        min_inclusive: f64,
        /// `max_exclusive`.
        max_exclusive: f64,
        /// `when_in_range`.
        when_in_range: Df,
        /// `when_out_of_range`.
        when_out_of_range: Df,
    },
    /// `interval_select`.
    IntervalSelect {
        /// `input`.
        input: Df,
        /// `thresholds`, ascending, one fewer than the functions.
        thresholds: Vec<f64>,
        /// `functions`.
        functions: Vec<Df>,
    },
    /// `spline`: a `CubicSpline` in `f32` on coordinates narrowed from `f64`.
    Spline(Arc<Spline>),
    /// `find_top_surface`.
    FindTopSurface {
        /// `density`.
        density: Df,
        /// `upper_bound`.
        upper_bound: Df,
        /// `lower_bound`.
        lower_bound: i32,
        /// `cell_height`, positive.
        cell_height: i32,
    },
    /// `end_islands`: `EndIslandDensityFunction`, the central island in code.
    EndIslands,
    /// `old_blended_noise`: `BlendedNoise.compute`.
    OldBlendedNoise(BlendedNoise),
    /// `blend_alpha`: 1 without blending.
    BlendAlpha,
    /// `blend_offset`: 0 without blending.
    BlendOffset,
    /// `beardifier`: 0 without structures.
    Beardifier,
    /// `interpolated`, `flat_cache`, `cache_2d`, `cache_once`, `cache_all_in_cell` and
    /// `blend_density`: `DensityFunctions.Marker`, the input outside a `NoiseChunk`.
    Marker(Marker, Df),
}

impl DensityFunction {
    /// The value, if this is a `DensityFunctions.Constant` itself.
    pub fn as_constant(&self) -> Option<f64> {
        match self {
            DensityFunction::Constant(v) => Some(*v),
            _ => None,
        }
    }
}

/// `TwoArgumentSimpleFunction.Type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TwoArgument {
    /// `add`.
    Add,
    /// `mul`.
    Mul,
    /// `min`.
    Min,
    /// `max`.
    Max,
}

/// `DensityFunctions.Mapped.Type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mapped {
    /// `abs`.
    Abs,
    /// `square`.
    Square,
    /// `cube`.
    Cube,
    /// `half_negative`.
    HalfNegative,
    /// `quarter_negative`.
    QuarterNegative,
    /// `invert`: `1 / x`.
    Invert,
    /// `squeeze`.
    Squeeze,
}

/// `DensityFunctions.Marker.Type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Marker {
    /// `interpolated`.
    Interpolated,
    /// `flat_cache`.
    FlatCache,
    /// `cache_2d`.
    Cache2D,
    /// `cache_once`.
    CacheOnce,
    /// `cache_all_in_cell`.
    CacheAllInCell,
    /// `blend_density`.
    BlendDensity,
}

/// `CubicSpline`, with 26.2 density functions as coordinates.
#[derive(Debug, Clone, PartialEq)]
pub enum Spline {
    /// `CubicSpline.Constant`, or a bare number.
    Constant(f32),
    /// `CubicSpline.Multipoint`: at least one point, the three lists the same length.
    Multipoint {
        /// `coordinate`.
        coordinate: Df,
        /// Each point's `location`.
        locations: Vec<f32>,
        /// Each point's `value`.
        values: Vec<Spline>,
        /// Each point's `derivative`.
        derivatives: Vec<f32>,
    },
}
