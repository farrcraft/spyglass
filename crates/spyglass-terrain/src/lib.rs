//! What the Overworld's generator answers about terrain before any chunk exists: the density
//! column, the aquifer, and the heights and block columns built on them
//! (`NoiseBasedChunkGenerator.getBaseHeight`, `getBaseColumn`). Structures decide their starts
//! from these. See `docs/specs/terrain.md` and ADR 0022; 26.2's terrain is another algorithm
//! family behind the same types (ADR 0026).

mod aquifer;
mod column;
mod settings;
mod tags;
mod v1_21;

pub use aquifer::{
    Aquifer, AquiferSamplers, Block, FluidPicker, FluidStatus, SurfaceLevels, VeinBlock,
    WAY_BELOW_MIN_Y,
};
pub use column::{BlockColumn, Heightmap, Terrain};
pub use settings::{BoundTerrain, TerrainSettings};
#[cfg(feature = "timing")]
pub use spyglass_density::timing;
