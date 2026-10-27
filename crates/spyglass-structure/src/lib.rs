//! Structure placement, biome validation and terrain-dependent validity, and jigsaw assembly.
//!
//! [`StructureSets::load`] reads a bundle's structure sets for one biome source;
//! [`StructureSets::with_seed`] binds them to a seed as [`Placements`], which answer where each
//! set may start, and [`Starts::locate`] is `/locate structure`. Slime chunks are
//! [`is_slime_chunk`]. With [`JigsawPools`], [`Starts::assemble`] builds jigsaw starts' pieces.
//! See `docs/specs/structure-placement.md`.

mod assembly;
mod biomes;
mod check;
mod corner;
mod jigsaw;
mod load;
mod locate;
mod placement;
mod portal;
mod slime;

pub use assembly::{Assembly, JigsawPools, Layout};
pub use biomes::BiomeSet;
pub use check::{Check, Selection, Starts};
pub use jigsaw::{BoundingBox, GenerationHeights, HeightProvider, Jigsaw, JigsawCheck};
pub use load::{Entry, Structure, StructureSet, StructureSets, StructureType};
pub use locate::{LOCATE_RADIUS, MAX_UNCERTAIN, StructureHit, StructureSearch};
pub use placement::{
    ConcentricRings, ExclusionZone, FrequencyReduction, Placement, PlacementKind, Placements,
    RandomSpread, RingPosition, SeedPlacements, SpreadType,
};
pub use portal::{RuinedPortal, Setup, VerticalPlacement};
pub use slime::is_slime_chunk;
