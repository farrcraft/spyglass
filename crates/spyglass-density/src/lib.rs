//! Density-function graph loading and evaluation.
//!
//! Loading ([`Loader`], [`ClimateRouter`]) turns a bundle's JSON into a seed-independent graph,
//! following the game's codecs (ADR 0010). Binding ([`Binder`]) creates the seed's noises as
//! `RandomState` does, and a [`Sampler`] evaluates the result at a block position with the
//! game's `float` arithmetic. See `docs/specs/density-functions.md`.

mod function;
mod load;
mod router;
mod sampler;

pub use function::{Axis, DensityFunction, Df, Gradient, Points, Spline};
pub use load::{Loader, Resources};
pub use router::{CLIMATE_ENTRIES, ClimateRouter, ClimateSampler, NOISE_SETTINGS};
pub use sampler::{Binder, Sampler};
