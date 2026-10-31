//! Decoration (ADR 0052): the order features place in a chunk, their seeds, placement
//! modifiers and the features themselves. See `docs/specs/features.md`.

mod decorate;
mod modifier;
pub mod order;
mod settings;

pub use decorate::{Decorator, Drawn, Try};
pub use modifier::Modifier;
pub use settings::{FeatureSettings, Placed};
