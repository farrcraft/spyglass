//! Density-function graph loading and evaluation.
//!
//! Loading ([`Loader`], [`ClimateRouter`]) turns a bundle's JSON into a seed-independent graph,
//! following the game's codecs (ADR 0010). Binding ([`Binder`]) creates the seed's noises as
//! `RandomState` does and compiles the graph as `DensityFunctionCompiler` does, choosing samplers
//! by constants and by [`Interval`] ranges. A [`Sampler`] evaluates the result with the game's
//! `float` arithmetic, at a block position or over a [`Volume`] of them, which the game computes
//! differently in places. See `docs/specs/density-functions.md`.
//!
//! 26.2's density evaluation is another algorithm family (ADR 0026), in [`v1_21`]. The climate
//! router and sampler dispatch on a dimension's noise settings, so callers don't need to know
//! which family they hold.

mod function;
mod interval;
mod java;
mod load;
mod range;
mod router;
mod sampler;
pub mod timing;
pub mod v1_21;
mod volume;

pub use function::{
    Axis, Binary, BlendedNoise, DensityFunction, Df, DistanceMetric, Gradient, GradientCoordinate,
    Points, Round, Spline, Tiling, Unary, UpscaleAxis, UpscaleMode,
};
pub use interval::{Interval, IntervalError};
pub use load::{Loader, Resources};
pub use router::{CLIMATE_ENTRIES, ClimateRouter, ClimateSampler, NOISE_SETTINGS};
pub use sampler::{Binder, Sampler};
pub use spyglass_noise::Volume;
