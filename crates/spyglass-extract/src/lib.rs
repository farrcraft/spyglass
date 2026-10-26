//! Building per-version worldgen data bundles from the user's own server jar: the version
//! manifest, a verified jar cache, finding Java, the data generator, and writing the bundle
//! (ADR 0016). `spyglass_data::bundle` reads what this writes.

pub mod bundler;
pub mod cache;
pub mod datagen;
pub mod extract;
pub mod java;
pub mod manifest;
pub mod write;
