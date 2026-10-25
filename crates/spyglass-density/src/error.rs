//! Load errors, which name the resource and the JSON path to the problem (ADR 0010).

use std::fmt;

use spyglass_data::bundle::BundleError;

/// Why a density function, noise or noise settings couldn't be loaded.
#[derive(Debug, thiserror::Error)]
pub struct LoadError {
    /// The resource being loaded, e.g. `minecraft:overworld/offset`.
    pub resource: String,
    /// Where in its JSON, e.g. `input.second.right.spline.points[5].value`. Empty for the root.
    pub path: String,
    /// What's wrong. Boxed, so results carrying a load error stay small.
    pub kind: Box<LoadErrorKind>,
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.path.is_empty() {
            write!(f, "{}: {}", self.resource, self.kind)
        } else {
            write!(f, "{}: {}: {}", self.resource, self.path, self.kind)
        }
    }
}

/// The kinds of [`LoadError`].
#[derive(Debug, thiserror::Error)]
pub enum LoadErrorKind {
    /// The resource couldn't be read from the bundle.
    #[error("{0}")]
    Bundle(#[from] BundleError),
    /// A `type` (or other variant) spyglass doesn't support yet.
    #[error("unsupported {what} {name:?}")]
    Unsupported {
        /// What kind of thing, e.g. `density function type`.
        what: &'static str,
        /// Its name, e.g. `minecraft:old_blended_noise`.
        name: String,
    },
    /// A required field is missing.
    #[error("missing field {0:?}")]
    Missing(&'static str),
    /// A value has the wrong JSON shape.
    #[error("expected {0}")]
    Shape(&'static str),
    /// A number is outside the range the game's codec accepts.
    #[error("{value} is outside {min}..={max}")]
    OutOfRange {
        /// The value, as written.
        value: String,
        /// The smallest value accepted.
        min: String,
        /// The largest value accepted.
        max: String,
    },
    /// A value the game's codec rejects for another reason.
    #[error("{0}")]
    Invalid(&'static str),
    /// Named functions refer to each other in a cycle.
    #[error("reference cycle: {}", .0.join(" -> "))]
    Cycle(Vec<String>),
}
