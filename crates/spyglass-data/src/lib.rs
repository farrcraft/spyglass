//! Reading per-version worldgen data bundles, the JSON codec helpers every loader shares, and
//! NBT. Bundles are built by `spyglass-extract` (ADR 0016).

pub mod anchor;
pub mod build;
pub mod bundle;
pub mod codec;
pub mod dimension;
pub mod nbt;
pub mod versions;
