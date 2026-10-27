//! The 1.21 family's density evaluation, which 26.2 has (ADR 0026): every function in `f64`
//! at a point (`DensityFunction.compute`), no volume arithmetic, and caches and interpolation
//! only inside the chunk generator's [`NoiseChunk`]. See
//! `docs/specs/density-functions.md#262-evaluation`.
//!
//! [`Loader`] reads 26.2's types; [`NoiseRouter`] reads a dimension's 15-entry router and noise
//! settings, and binds them to a seed as [`RandomState`], which evaluates the router at a point,
//! samples climate, and makes [`NoiseChunk`]s. [`Binder`] binds any loaded function as
//! `RandomState`'s noise wiring would. Which family a dimension is follows its noise settings'
//! shape ([`is_v1_21`]).

mod bind;
mod chunk;
mod function;
mod load;
mod router;

pub use bind::{Binder, Function};
pub use chunk::NoiseChunk;
pub use function::{BlendedNoise, DensityFunction, Df, Mapped, Marker, Spline, TwoArgument};
pub use load::Loader;
pub use router::{ClimateSampler, NoiseRouter, NoiseSettings, RandomState, RouterEntry, is_v1_21};
