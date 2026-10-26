//! Density-function graph loading and evaluation.
//!
//! Loading ([`Loader`], [`ClimateRouter`]) turns a bundle's JSON into a seed-independent graph,
//! following the game's codecs (ADR 0010). Binding ([`Binder`]) creates the seed's noises as
//! `RandomState` does and compiles the graph as `DensityFunctionCompiler` does, choosing samplers
//! by constants and by [`Interval`] ranges. A [`Sampler`] evaluates the result at a block
//! position with the game's `float` arithmetic. See `docs/specs/density-functions.md`.

mod function;
mod interval;
mod java;
mod load;
mod range;
mod router;
mod sampler;

pub use function::{
    Axis, Binary, BlendedNoise, DensityFunction, Df, DistanceMetric, Gradient, Points, Round,
    Spline, Tiling, Unary,
};
pub use interval::{Interval, IntervalError};
pub use load::{Loader, Resources};
pub use router::{CLIMATE_ENTRIES, ClimateRouter, ClimateSampler, NOISE_SETTINGS};
pub use sampler::{Binder, Sampler};
