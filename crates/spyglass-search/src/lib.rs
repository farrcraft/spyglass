//! Seed search: the query language, the placement prefilter over the lower 48 bits, and the
//! parallel runner with ranking.
//!
//! [`Query::parse`] reads a query's text into constraints, and a [`Query`]'s `Display` is its
//! canonical text. The engine's layering is ADR 0021: constraint semantics live in the `spyglass`
//! crate, which resolves a query against a world and evaluates seeds. See
//! `docs/specs/search.md`.

mod group;
mod parse;
mod prefilter;
mod query;
mod run;

pub use group::{distance_sqr, largest_group};
pub use parse::ParseError;
pub use prefilter::{Named, Prefilter, SPAWN_REACH};
pub use query::{
    Anchor, Constraint, Count, DEFAULT_STEP, DEFAULT_Y, Importance, Kind, MAX_COORDINATE,
    MAX_PLACE_SIDE, MAX_SAMPLES, Place, Query, Region, Sampling,
};
pub use run::{Checked, Hit, Progress, RunOptions, SeedSource, run};
