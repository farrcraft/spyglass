//! Structure placement, biome validation and terrain-dependent validity, and jigsaw assembly.
//!
//! [`StructureSets::load`] reads a bundle's structure sets for one biome source;
//! [`StructureSets::with_seed`] binds them to a seed as [`Placements`], which answer where each
//! set may start, and [`Starts::locate`] is `/locate structure`. Slime chunks are
//! [`is_slime_chunk`]. [`Starts::assemble`] builds mineshafts' pieces, and with [`JigsawPools`] jigsaw starts'.
//! See `docs/specs/structure-placement.md`.

mod assembly;
mod biomes;
mod check;
mod corner;
mod jigsaw;
mod load;
mod locate;
mod mineshaft;
mod placement;
mod portal;
mod scattered;
mod slime;
mod template_piece;

pub use assembly::{Assembly, JigsawPools, LaidPiece, Layout};
pub use biomes::BiomeSet;
pub use check::{Check, Selection, Starts};
pub use jigsaw::{
    BoundingBox, GenerationHeights, HeightProvider, Jigsaw, JigsawCheck, Marker, NbtBlock,
    PoolElement, Processors, Projection, Rotation, Template,
};
pub use load::{Entry, STEPS, Structure, StructureSet, StructureSets, StructureType};
pub use locate::{LOCATE_RADIUS, MAX_UNCERTAIN, StructureHit, StructureSearch};
pub use mineshaft::{Mineshaft, ShaftPiece};
pub use placement::{
    ConcentricRings, ExclusionZone, FrequencyReduction, Placement, PlacementKind, Placements,
    RandomSpread, RingPosition, SeedPlacements, SpreadType,
};
pub use portal::{PortalLayout, RuinedPortal, Setup, VerticalPlacement};
pub use scattered::{Facing, ScatteredPiece};
pub use slime::is_slime_chunk;
pub use template_piece::{TemplatePiece, template_pieces};
