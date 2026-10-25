//! Climate sampling and biome lookup for the Overworld, Nether and End.
//!
//! A [`BiomeSource`] loads a dimension's climate router and biome parameter list from a bundle;
//! [`BiomeSource::with_seed`] binds it to a seed for [`Biomes::biome_at`] and
//! [`Biomes::biomes_in`]. The nearest-point search is exact and reports ties, where the game's
//! answer depends on lookup history. See `docs/specs/biome-lookup.md`.

mod parameters;
mod search;
mod source;

pub use parameters::{BiomeId, Parameter, ParameterList, ParameterPoint, TargetPoint, quantize};
pub use search::{Lookup, SearchTree};
pub use source::{BiomeRegion, BiomeSource, Biomes};
