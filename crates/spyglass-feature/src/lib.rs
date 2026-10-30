//! Decoration (ADR 0052): the order features place in a chunk, their seeds, placement
//! modifiers and the features themselves. See `docs/specs/features.md`.

pub mod order;
mod settings;

pub use settings::{FeatureSettings, Placed};
