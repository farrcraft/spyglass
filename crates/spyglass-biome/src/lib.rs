//! Climate sampling and biome lookup for the Overworld, Nether and End.
//!
//! A [`BiomeSource`] loads a dimension's climate router and biome parameter list from a bundle;
//! [`BiomeSource::with_seed`] binds it to a seed for [`Biomes::biome_at`] and
//! [`Biomes::biomes_in`]. The nearest-point search is exact and reports ties, where the game's
//! answer depends on lookup history. See `docs/specs/biome-lookup.md`.
//!
//! [`SpawnTargets`] is the world spawn's climate search, which picks the chunk the game's spawn
//! search starts from. See `docs/specs/spawn.md`.

mod locate;
mod parameters;
mod search;
mod source;
mod spawn;

pub use locate::{
    BiomeHit, BiomeSearch, MAX_UNCERTAIN, SearchVolume, out_from_origin, spiral_around,
};
pub use parameters::{BiomeId, Parameter, ParameterList, ParameterPoint, TargetPoint, quantize};
pub use search::{Lookup, SearchTree};
pub use source::{
    BiomeRegion, BiomeSource, Biomes, HorizontalArea, HorizontalMatch, HorizontalSearch,
    MAX_REPLAYED_TIES,
};
pub use spawn::{SpawnOrigin, SpawnPoint, SpawnSearch, SpawnTargets};
