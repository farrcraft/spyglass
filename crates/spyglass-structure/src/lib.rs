//! Structure placement, biome validation and terrain-dependent validity, with the start piece of jigsaw structures.
//!
//! [`StructureSets::load`] reads a bundle's structure sets for one biome source;
//! [`StructureSets::with_seed`] binds them to a seed as [`Placements`], which answer where each
//! set may start, and [`Starts::locate`] is `/locate structure`. Slime chunks are
//! [`is_slime_chunk`]. See `docs/specs/structure-placement.md`.

mod biomes;
mod check;
mod jigsaw;
mod load;
mod locate;
mod placement;
mod slime;

pub use biomes::BiomeSet;
pub use check::{Check, Selection, Starts};
pub use jigsaw::{GenerationHeights, Jigsaw, JigsawCheck};
pub use load::{Entry, Structure, StructureSet, StructureSets, StructureType};
pub use locate::{LOCATE_RADIUS, MAX_UNCERTAIN, StructureHit, StructureSearch};
pub use placement::{
    ConcentricRings, ExclusionZone, FrequencyReduction, Placement, PlacementKind, Placements,
    RandomSpread, RingPosition, SeedPlacements, SpreadType,
};
pub use slime::is_slime_chunk;
