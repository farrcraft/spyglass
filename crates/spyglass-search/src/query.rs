//! A query's syntax tree and its canonical text (`docs/specs/search.md`).

use std::fmt;

use crate::parse::{self, ParseError};

/// The largest coordinate, radius or distance a query may name: the world border.
pub const MAX_COORDINATE: i32 = 30_000_000;
/// The most blocks a place's bounding box may span on each side, so every count is bounded.
pub const MAX_PLACE_SIDE: u32 = 65_536;
/// The height biomes are sampled at unless a constraint says `at y`, as on the map.
pub const DEFAULT_Y: i32 = 64;
/// The biome grid's spacing in blocks unless a constraint says `step`.
pub const DEFAULT_STEP: u32 = 16;
/// The most grid points a biome constraint may have, counted over its place's bounding box.
pub const MAX_SAMPLES: u64 = 1 << 20;

/// A parsed query: its constraints in order.
#[derive(Debug, Clone)]
pub struct Query {
    constraints: Vec<Constraint>,
    lines: Vec<usize>,
}

impl Query {
    /// Parses a query's text. Errors name the line and column.
    pub fn parse(text: &str) -> Result<Query, ParseError> {
        let parsed = parse::query(text)?;
        let (lines, constraints) = parsed.into_iter().unzip();
        Ok(Query { constraints, lines })
    }

    /// The constraints, in the order written.
    pub fn constraints(&self) -> &[Constraint] {
        &self.constraints
    }

    /// The line (from 1) each constraint was written on, by index into
    /// [`constraints`](Self::constraints).
    pub fn lines(&self) -> &[usize] {
        &self.lines
    }
}

/// The canonical text: one constraint per line, each ending in a newline.
impl fmt::Display for Query {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for c in &self.constraints {
            writeln!(f, "{c}")?;
        }
        Ok(())
    }
}

/// One line of a query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Constraint {
    /// Required, or preferred with a weight.
    pub importance: Importance,
    /// What it asks for.
    pub kind: Kind,
}

/// Whether a constraint must hold or adds to the score.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Importance {
    /// `require`: a seed is reported only if it holds.
    Required,
    /// `prefer … weight N`: adds `weight × satisfaction` to the score.
    Preferred {
        /// 1 to 1,000; 1 when not written.
        weight: u32,
    },
}

/// A count of features.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Count {
    /// `N`: at least N, 1 to 10,000; 1 when not written.
    AtLeast(u32),
    /// `no`: none.
    No,
}

/// What a constraint asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    /// `[count] STRUCTURE region`: structure starts.
    Structures {
        /// How many.
        count: Count,
        /// A full structure id, or `#` and a full tag id.
        structure: String,
        /// Where.
        region: Region,
    },
    /// `cluster N STRUCTURE within D of each other place`.
    Cluster {
        /// N, 2 to 64.
        size: u32,
        /// A full structure id, or `#` and a full tag id.
        structure: String,
        /// D: the most any two may be apart.
        apart: u32,
        /// Where all of them are.
        place: Place,
    },
    /// `[count] slime region`: slime chunks.
    Slime {
        /// How many.
        count: Count,
        /// Where.
        region: Region,
    },
    /// `[no] biome BIOME place [sampling]`: present, or absent.
    Biome {
        /// Whether the biome must be present (`false` for `no biome`).
        present: bool,
        /// A full biome id.
        biome: String,
        /// Where.
        place: Place,
        /// The grid.
        sampling: Sampling,
    },
    /// `biome BIOME covers P% place [sampling]`.
    Coverage {
        /// A full biome id.
        biome: String,
        /// P, 1 to 100.
        percent: u32,
        /// Where.
        place: Place,
        /// The grid.
        sampling: Sampling,
    },
    /// `spawn place`: the spawn is in the place, which isn't anchored at the spawn.
    Spawn {
        /// Where.
        place: Place,
    },
}

/// Where structures or slime chunks are counted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Region {
    /// A disc or box.
    Place(Place),
    /// `within R of STRUCTURE place`: within R of some start of the structure in the place.
    NearStructure {
        /// R.
        radius: u32,
        /// A full structure id, or `#` and a full tag id.
        structure: String,
        /// Where the anchor's starts are.
        place: Place,
    },
}

/// A disc or box of blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    /// `within R of anchor`.
    Within {
        /// R.
        radius: u32,
        /// The centre.
        anchor: Anchor,
    },
    /// `in X1,Z1 to X2,Z2`, corners included, stored with `min` the smaller on each axis.
    Box {
        /// The smaller x and z.
        min: [i32; 2],
        /// The larger x and z.
        max: [i32; 2],
    },
}

/// The centre of a disc.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    /// The spawn.
    Spawn,
    /// A block `x,z`.
    Point([i32; 2]),
}

/// A biome grid's height and spacing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sampling {
    /// `at y`.
    pub y: i32,
    /// `step`, 1 to 4,096.
    pub step: u32,
}

impl Default for Sampling {
    fn default() -> Self {
        Sampling {
            y: DEFAULT_Y,
            step: DEFAULT_STEP,
        }
    }
}

impl Place {
    /// How many grid points at spacing `step` the place's bounding box holds: what
    /// [`MAX_SAMPLES`] limits.
    pub fn grid_bound(&self, step: u32) -> u64 {
        let step = u64::from(step);
        match *self {
            Place::Within { radius, .. } => {
                let side = 2 * (u64::from(radius) / step) + 1;
                side * side
            }
            Place::Box { min, max } => {
                let side = |a: i32, b: i32| u64::from(b.abs_diff(a)) / step + 1;
                side(min[0], max[0]) * side(min[1], max[1])
            }
        }
    }
}

impl fmt::Display for Constraint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.importance {
            Importance::Required => write!(f, "require {}", self.kind),
            Importance::Preferred { weight: 1 } => write!(f, "prefer {}", self.kind),
            Importance::Preferred { weight } => write!(f, "prefer {} weight {weight}", self.kind),
        }
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Kind::Structures {
                count,
                structure,
                region,
            } => write!(f, "{count}{structure} {region}"),
            Kind::Cluster {
                size,
                structure,
                apart,
                place,
            } => write!(
                f,
                "cluster {size} {structure} within {apart} of each other {place}"
            ),
            Kind::Slime { count, region } => write!(f, "{count}slime {region}"),
            Kind::Biome {
                present,
                biome,
                place,
                sampling,
            } => {
                let no = if *present { "" } else { "no " };
                write!(f, "{no}biome {biome} {place}{sampling}")
            }
            Kind::Coverage {
                biome,
                percent,
                place,
                sampling,
            } => write!(f, "biome {biome} covers {percent}% {place}{sampling}"),
            Kind::Spawn { place } => write!(f, "spawn {place}"),
        }
    }
}

/// The count and the space after it; nothing for the default of 1.
impl fmt::Display for Count {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Count::AtLeast(1) => Ok(()),
            Count::AtLeast(n) => write!(f, "{n} "),
            Count::No => write!(f, "no "),
        }
    }
}

impl fmt::Display for Region {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Region::Place(place) => write!(f, "{place}"),
            Region::NearStructure {
                radius,
                structure,
                place,
            } => write!(f, "within {radius} of {structure} {place}"),
        }
    }
}

impl fmt::Display for Place {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Place::Within { radius, anchor } => write!(f, "within {radius} of {anchor}"),
            Place::Box { min, max } => {
                write!(f, "in {},{} to {},{}", min[0], min[1], max[0], max[1])
            }
        }
    }
}

impl fmt::Display for Anchor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Anchor::Spawn => write!(f, "spawn"),
            Anchor::Point([x, z]) => write!(f, "{x},{z}"),
        }
    }
}

/// The options that differ from the defaults, each with a space before it.
impl fmt::Display for Sampling {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.y != DEFAULT_Y {
            write!(f, " at y {}", self.y)?;
        }
        if self.step != DEFAULT_STEP {
            write!(f, " step {}", self.step)?;
        }
        Ok(())
    }
}
