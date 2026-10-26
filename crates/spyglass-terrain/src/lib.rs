//! What the Overworld's generator answers about terrain before any chunk exists: the density
//! column, the aquifer, and the heights and block columns built on them
//! (`NoiseBasedChunkGenerator.getBaseHeight`, `getBaseColumn`). Structures decide their starts
//! from these. See `docs/specs/terrain.md` and ADR 0022.

mod aquifer;
mod column;
mod settings;
mod tags;

pub use aquifer::{Aquifer, AquiferSamplers, Block, FluidPicker, FluidStatus, WAY_BELOW_MIN_Y};
pub use column::{BlockColumn, Heightmap, Terrain};
pub use settings::{BoundTerrain, TerrainSettings};
