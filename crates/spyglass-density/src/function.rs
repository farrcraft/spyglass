//! The loaded, seed-independent form of a density-function graph.

use std::sync::Arc;

/// A shared node. Named functions referenced from several places are one node, as after the
/// game's `INLINE_REFERENCE` rewrite.
pub type Df = Arc<DensityFunction>;

/// A density function, with references resolved. Types follow the game's codec names (see
/// `docs/specs/density-functions.md`).
#[derive(Debug, Clone, PartialEq)]
pub enum DensityFunction {
    /// `constant`, or a bare number: `ConstantFunction`.
    Constant(f32),
    /// `noise`: `NoiseFunction`. Shifts default to the constant 0.
    Noise {
        /// The noise's id, e.g. `minecraft:temperature`.
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
        /// The noise's id.
        noise: String,
    },
    /// `shift_b`: the noise at `(z/4, x/4, 0)`, times 4.
    ShiftB {
        /// The noise's id.
        noise: String,
    },
    /// `shift`: the noise at `(x/4, y/4, z/4)`, times 4.
    Shift {
        /// The noise's id.
        noise: String,
    },
    /// `add`, `sub`, `mul`, `div`, `min` and `max`: `BinaryFunction`.
    Binary(Binary, Df, Df),
    /// `abs`, `square` and the other one-input functions: `UnaryFunction`.
    Unary(Unary, Df),
    /// `floor`, `round`, `ceil` and `truncate`: `RoundFunction`.
    Round {
        /// Which rounding.
        kind: Round,
        /// `input`.
        input: Df,
        /// `multiple`, default the constant 1.
        multiple: Df,
    },
    /// `clamp`: `ClampFunction`.
    Clamp {
        /// `input`.
        input: Df,
        /// `min`.
        min: f32,
        /// `max`, never below `min`.
        max: f32,
        /// `Math.clamp`, and a sampler chosen by the input's range (switch `clamp_positive_zero`,
        /// 26.4 on); else `Mth.clamp`, always (26.3).
        math_clamp: bool,
    },
    /// `pow`: `PowFunction`.
    Pow {
        /// `base`.
        base: Df,
        /// `exponent`.
        exponent: Df,
    },
    /// `cache`: a hint for the game's caches; transparent on the point path.
    Cache(Df),
    /// `lerp`.
    Lerp {
        /// `alpha`.
        alpha: Df,
        /// `first`: the value at alpha 0.
        first: Df,
        /// `second`: the value at alpha 1.
        second: Df,
    },
    /// `range_choice`: `RangeChoiceFunction`.
    RangeChoice {
        /// `input`.
        input: Df,
        /// `min_inclusive`.
        min_inclusive: f32,
        /// `max_exclusive`.
        max_exclusive: f32,
        /// `when_in_range`.
        when_in_range: Df,
        /// `when_out_of_range`.
        when_out_of_range: Df,
    },
    /// `interval_select`: `IntervalSelectFunction`.
    IntervalSelect {
        /// `input`.
        input: Df,
        /// `thresholds`, in order, one fewer than the functions.
        thresholds: Vec<f32>,
        /// `functions`, at least two.
        functions: Vec<Df>,
    },
    /// `slice`: the input with one coordinate fixed.
    Slice {
        /// `axis`.
        axis: Axis,
        /// `coordinate`.
        coordinate: i32,
        /// `input`.
        input: Df,
    },
    /// `interpolated`: the input at cell corners, interpolated inside cells.
    Interpolated {
        /// `input`.
        input: Df,
        /// `cell_size_xz`, positive.
        cell_size_xz: i32,
        /// `cell_size_y`, positive.
        cell_size_y: i32,
    },
    /// `upscale`: `UpscaleFunction` (26.4 on): the input at cell corners, upscaled inside cells
    /// one axis at a time. Not bit-identical to `interpolated`, even with linear modes.
    Upscale {
        /// `input`.
        input: Df,
        /// `x`.
        x: UpscaleAxis,
        /// `y`.
        y: UpscaleAxis,
        /// `z`.
        z: UpscaleAxis,
    },
    /// `find_top_surface`: `FindTopSurfaceFunction`.
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
    /// `old_blended_noise`: `BlendedNoise`.
    OldBlendedNoise(BlendedNoise),
    /// `gradient`.
    Gradient(Gradient),
    /// `spline`.
    Spline(Arc<Spline>),
    /// `blend_alpha`: 1 without blending.
    BlendAlpha,
    /// `blend_offset`: 0 without blending.
    BlendOffset,
    /// `beardifier`: 0 without structures.
    Beardifier,
    /// `blend_density`: the input, without blending.
    BlendDensity(Df),
    /// `distance_to_point`: the End's; loads, but can't be bound yet.
    DistanceToPoint {
        /// `point`.
        point: [i32; 3],
        /// `metric`.
        metric: DistanceMetric,
    },
    /// `end_outer_islands`: the End's; loads, but can't be bound yet.
    EndOuterIslands,
}

impl DensityFunction {
    /// Whether this is `DensityFunctions.zero()` by the game's record equality: the constant
    /// `+0.0` (`Float.compare` tells `-0.0` apart).
    pub fn is_zero(&self) -> bool {
        matches!(self, DensityFunction::Constant(v) if v.to_bits() == 0)
    }

    /// The value, if this is a `ConstantFunction` as the game's compiler sees it: a `cache` of a
    /// constant isn't one.
    pub fn as_constant(&self) -> Option<f32> {
        match self {
            DensityFunction::Constant(v) => Some(*v),
            _ => None,
        }
    }
}

/// `BinaryFunction.Type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Binary {
    /// `add`.
    Add,
    /// `sub`.
    Sub,
    /// `mul`.
    Mul,
    /// `div`.
    Div,
    /// `min`.
    Min,
    /// `max`.
    Max,
}

/// `UnaryFunction.Type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unary {
    /// `abs`.
    Abs,
    /// `square`.
    Square,
    /// `cube`.
    Cube,
    /// `sqrt`.
    Sqrt,
    /// `half_negative`: negative values halved.
    HalfNegative,
    /// `quarter_negative`: negative values quartered.
    QuarterNegative,
    /// `reciprocal`.
    Reciprocal,
    /// `negate`.
    Negate,
    /// `squeeze`.
    Squeeze,
    /// `log`.
    Log,
    /// `sign`.
    Sign,
}

/// `RoundFunction.Type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Round {
    /// `floor`.
    Floor,
    /// `round`: halves up.
    Round,
    /// `ceil`.
    Ceil,
    /// `truncate`: toward zero.
    Truncate,
}

/// `Direction.Axis`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    /// `x`.
    X,
    /// `y`.
    Y,
    /// `z`.
    Z,
}

impl Axis {
    /// `Axis.choose(int, int, int)`.
    pub fn choose(self, x: i32, y: i32, z: i32) -> i32 {
        match self {
            Axis::X => x,
            Axis::Y => y,
            Axis::Z => z,
        }
    }
}

/// `TilingMode`: what a gradient does beyond its coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tiling {
    /// `clamp_to_edge`, the default: the end values continue.
    #[default]
    ClampToEdge,
    /// `repeat`.
    Repeat,
    /// `mirrored_repeat`.
    MirroredRepeat,
}

/// `GradientFunction`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Gradient {
    /// `axis`.
    pub axis: Axis,
    /// `tiling`.
    pub tiling: Tiling,
    /// `from_coordinate`.
    pub from_coordinate: i32,
    /// `to_coordinate`; never equal to `from_coordinate`.
    pub to_coordinate: i32,
    /// `from_value`.
    pub from_value: f32,
    /// `to_value`.
    pub to_value: f32,
}

/// `BlendedNoise`'s parameters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlendedNoise {
    /// `xz_scale`, 0.001 to 1000.
    pub xz_scale: f64,
    /// `y_scale`, 0.001 to 1000.
    pub y_scale: f64,
    /// `xz_factor`, 0.001 to 1000.
    pub xz_factor: f64,
    /// `y_factor`, 0.001 to 1000.
    pub y_factor: f64,
    /// `smear_scale_multiplier`, 1 to 8.
    pub smear_scale_multiplier: f64,
}

/// `DistanceMetric`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DistanceMetric {
    /// `euclidean`.
    Euclidean,
    /// `euclidean_squared`.
    EuclideanSquared,
    /// `manhattan`.
    Manhattan,
    /// `chebyshev`.
    Chebyshev,
}

/// `UpscaleFunction.Axis`: one axis's mode and cell size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UpscaleAxis {
    /// `mode`.
    pub mode: UpscaleMode,
    /// `cell_size`, 1 to 64.
    pub cell_size: i32,
}

/// `UpscaleFunction.Mode`: how an axis fills its cells from the corner samples around them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpscaleMode {
    /// `nearest_neighbor`: the cell's first corner.
    NearestNeighbor,
    /// `interpolate_linear`.
    InterpolateLinear,
    /// `interpolate_cosine`: linear, eased by `Ease.inOutSine`.
    InterpolateCosine,
    /// `interpolate_cubic`: Catmull-Rom.
    InterpolateCubic,
    /// `b_spline_cubic`: a cubic B-spline, which doesn't pass through the corners.
    BSplineCubic,
}

impl UpscaleMode {
    /// `samplesBefore`: corners sampled before the cell's first.
    pub fn samples_before(self) -> i32 {
        match self {
            UpscaleMode::InterpolateCubic | UpscaleMode::BSplineCubic => 1,
            _ => 0,
        }
    }

    /// `samplesAfter`: corners sampled after the cell's first.
    pub fn samples_after(self) -> i32 {
        match self {
            UpscaleMode::NearestNeighbor => 0,
            UpscaleMode::InterpolateLinear | UpscaleMode::InterpolateCosine => 1,
            UpscaleMode::InterpolateCubic | UpscaleMode::BSplineCubic => 2,
        }
    }

    /// `sampleCount`: `samplesBefore + 1 + samplesAfter`.
    pub fn sample_count(self) -> i32 {
        self.samples_before() + 1 + self.samples_after()
    }

    /// `exactValueOnVertex`: whether the value on a corner is the corner's sample.
    pub fn exact_on_corner(self) -> bool {
        self != UpscaleMode::BSplineCubic
    }
}

/// `CubicSpline`, with density functions as coordinates.
#[derive(Debug, Clone, PartialEq)]
pub enum Spline {
    /// `CubicSpline.Constant`, or a bare number.
    Constant(f32),
    /// `CubicSpline.Multipoint`.
    Multipoint(Points),
}

/// `CubicSpline.Multipoint`'s coordinate and points: at least one point, each with a location,
/// a value and a derivative, in the order given.
#[derive(Debug, Clone, PartialEq)]
pub struct Points {
    coordinate: Df,
    locations: Vec<f32>,
    values: Vec<Spline>,
    derivatives: Vec<f32>,
    nan_is_nan: bool,
}

impl Points {
    /// The points, or `None` unless there's at least one and the three lists have the same
    /// length.
    pub fn new(
        coordinate: Df,
        locations: Vec<f32>,
        values: Vec<Spline>,
        derivatives: Vec<f32>,
    ) -> Option<Self> {
        let n = locations.len();
        (n > 0 && values.len() == n && derivatives.len() == n).then_some(Points {
            coordinate,
            locations,
            values,
            derivatives,
            nan_is_nan: false,
        })
    }

    /// The points, sampled as 26.4 samples them if `on` (switch `spline_nan_is_nan`): a NaN
    /// coordinate gives NaN, and a coordinate on the first location takes the first point,
    /// extended.
    pub fn with_nan_is_nan(self, on: bool) -> Self {
        Points {
            nan_is_nan: on,
            ..self
        }
    }

    /// Whether sampling follows switch `spline_nan_is_nan` (see
    /// [`with_nan_is_nan`](Self::with_nan_is_nan)).
    pub fn nan_is_nan(&self) -> bool {
        self.nan_is_nan
    }

    /// `coordinate`: the function whose value picks the interval.
    pub fn coordinate(&self) -> &Df {
        &self.coordinate
    }

    /// Each point's `location`.
    pub fn locations(&self) -> &[f32] {
        &self.locations
    }

    /// Each point's `value`.
    pub fn values(&self) -> &[Spline] {
        &self.values
    }

    /// Each point's `derivative`.
    pub fn derivatives(&self) -> &[f32] {
        &self.derivatives
    }
}
