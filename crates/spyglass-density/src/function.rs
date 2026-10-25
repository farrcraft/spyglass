//! The loaded, seed-independent form of a density-function graph.

use std::sync::Arc;

/// A shared node. Named functions referenced from several places are one node, as after the
/// game's `INLINE_REFERENCE` rewrite.
pub type Df = Arc<DensityFunction>;

/// A density function, with references resolved. Types follow the game's codec names; only the
/// ones the overworld's climate needs are supported so far (see `docs/specs/density-functions.md`).
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
    /// `add`: `left + right`.
    Add(Df, Df),
    /// `mul`: `left * right`.
    Mul(Df, Df),
    /// `abs`.
    Abs(Df),
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
    /// `gradient`.
    Gradient(Gradient),
    /// `spline`.
    Spline(Arc<Spline>),
    /// `blend_alpha`: 1 without blending.
    BlendAlpha,
    /// `blend_offset`: 0 without blending.
    BlendOffset,
}

impl DensityFunction {
    /// Whether this is `DensityFunctions.zero()` by the game's record equality: the constant
    /// `+0.0` (`Float.compare` tells `-0.0` apart).
    pub fn is_zero(&self) -> bool {
        matches!(self, DensityFunction::Constant(v) if v.to_bits() == 0)
    }
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

/// `GradientFunction` with the default `clamp_to_edge` tiling, the only one supported so far.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Gradient {
    /// `axis`.
    pub axis: Axis,
    /// `from_coordinate`.
    pub from_coordinate: i32,
    /// `to_coordinate`; never equal to `from_coordinate`.
    pub to_coordinate: i32,
    /// `from_value`.
    pub from_value: f32,
    /// `to_value`.
    pub to_value: f32,
}

/// `CubicSpline`, with density functions as coordinates.
#[derive(Debug, Clone, PartialEq)]
pub enum Spline {
    /// `CubicSpline.Constant`, or a bare number.
    Constant(f32),
    /// `CubicSpline.Multipoint`: points sorted by location.
    Multipoint {
        /// `coordinate`: the function whose value picks the interval.
        coordinate: Df,
        /// Each point's `location`.
        locations: Vec<f32>,
        /// Each point's `value`.
        values: Vec<Spline>,
        /// Each point's `derivative`.
        derivatives: Vec<f32>,
    },
}
