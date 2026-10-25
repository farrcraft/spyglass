//! Structure placement, biome validation, terrain-dependent validity and jigsaw assembly.
//!
//! [`StructureSets::load`] reads a bundle's structure sets for one biome source;
//! [`StructureSets::with_seed`] binds them to a seed as [`Placements`], which answer where each
//! set may start. Slime chunks are [`is_slime_chunk`]. See `docs/specs/structure-placement.md`.

mod biomes;
mod check;
mod jigsaw;
mod load;
mod placement;
mod slime;

pub use biomes::BiomeSet;
pub use check::{Check, Selection, Starts};
pub use jigsaw::{
    BoundingBox, GenerationHeights, HeightProvider, Jigsaw, JigsawBlock, JigsawCheck, Pool,
    PoolElement, Rotation, Template, VerticalAnchor,
};
pub use load::{Entry, Structure, StructureSet, StructureSets, StructureType};
pub use placement::{
    ConcentricRings, ExclusionZone, FrequencyReduction, Placement, PlacementKind, Placements,
    RandomSpread, RingPosition, SpreadType, java_round, round_trig_product,
};
pub use slime::is_slime_chunk;
