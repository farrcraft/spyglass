//! The Default world preset's dimensions, by id.

use crate::codec::full_id;

/// One of the Default preset's dimensions (ADR 0023).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Dimension {
    /// `minecraft:overworld`.
    Overworld,
    /// `minecraft:the_nether`.
    Nether,
    /// `minecraft:the_end`.
    End,
}

impl Dimension {
    /// All three, in this order.
    pub const ALL: [Dimension; 3] = [Dimension::Overworld, Dimension::Nether, Dimension::End];

    /// The dimension's id, e.g. `minecraft:the_nether`.
    pub fn id(self) -> &'static str {
        match self {
            Dimension::Overworld => "minecraft:overworld",
            Dimension::Nether => "minecraft:the_nether",
            Dimension::End => "minecraft:the_end",
        }
    }

    /// The dimension's name in English prose, e.g. `the Nether`.
    pub fn name(self) -> &'static str {
        match self {
            Dimension::Overworld => "the Overworld",
            Dimension::Nether => "the Nether",
            Dimension::End => "the End",
        }
    }

    /// The position in [`ALL`](Self::ALL), for arrays indexed by dimension.
    pub fn index(self) -> usize {
        self as usize
    }

    /// The dimension with id `id` (with or without the `minecraft:` namespace).
    pub fn from_id(id: &str) -> Option<Dimension> {
        let id = full_id(id);
        Dimension::ALL.into_iter().find(|d| d.id() == id)
    }
}

impl std::fmt::Display for Dimension {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.id())
    }
}
