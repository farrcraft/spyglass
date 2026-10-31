//! Decoration (ADR 0052): the order features place in a chunk, their seeds, placement
//! modifiers and the features themselves. See `docs/specs/features.md`.

pub mod blocks;
mod decorate;
mod modifier;
pub mod order;
pub mod place;
pub mod predicate;
pub mod region;
mod settings;

pub use blocks::{Blocks, Heightmap};
pub use decorate::{Decorator, Drawn, Try};
pub use modifier::{Modifier, Read, Reading};
pub use place::{Outcome, Placement, Placer};
pub use region::{Doubt, Region};
pub use settings::{FeatureSettings, Placed};
