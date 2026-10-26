//! Seed search (`docs/specs/search.md`, ADRs 0020 and 0021): queries resolved against a world,
//! and seeds judged against them.

mod evaluate;

use spyglass_search::{Kind, Query, Region};

pub use evaluate::{ConstraintResult, Evaluation, Feature, Outcome};

use crate::{BiomeQuery, Error, Result, StructureQuery, World};

/// A search query parsed and resolved by [`World::search_query`]. Its `Display` is the
/// canonical text.
#[derive(Debug, Clone)]
pub struct SearchQuery {
    world: u64,
    query: Query,
    /// By constraint.
    resolved: Vec<Resolved>,
}

/// A constraint's names, resolved.
#[derive(Debug, Clone)]
struct Resolved {
    /// What's counted: a structure for structure counts and clusters, a biome for biome
    /// constraints.
    target: Option<Target>,
    /// The structure of a structure anchor.
    anchor: Option<StructureQuery>,
}

#[derive(Debug, Clone)]
enum Target {
    Structure(StructureQuery),
    Biome(BiomeQuery),
}

impl SearchQuery {
    /// The parsed query.
    pub fn query(&self) -> &Query {
        &self.query
    }
}

impl std::fmt::Display for SearchQuery {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.query.fmt(f)
    }
}

impl World {
    /// Parses a search query and resolves its structures, tags and biomes for this world's
    /// sessions. Errors name the line, and the column where the text doesn't parse.
    pub fn search_query(&self, text: &str) -> Result<SearchQuery> {
        let query = Query::parse(text).map_err(|e| Error::InvalidQuery {
            line: e.line,
            column: Some(e.column),
            message: e.message,
        })?;
        let heights = self.sets.heights();
        let (min_y, max_y) = (
            heights.level_min_y,
            heights.level_min_y + heights.level_height - 1,
        );
        let resolved = query
            .constraints()
            .iter()
            .zip(query.lines())
            .map(|(constraint, &line)| {
                let at_line = |e: Error| Error::InvalidQuery {
                    line,
                    column: None,
                    message: e.to_string(),
                };
                let structure = |s: &str| self.structure_query(s).map_err(at_line);
                let anchor = |region: &Region| match region {
                    Region::NearStructure { structure: s, .. } => structure(s).map(Some),
                    Region::Place(_) => Ok(None),
                };
                Ok(match &constraint.kind {
                    Kind::Structures {
                        structure: s,
                        region,
                        ..
                    } => Resolved {
                        target: Some(Target::Structure(structure(s)?)),
                        anchor: anchor(region)?,
                    },
                    Kind::Cluster { structure: s, .. } => Resolved {
                        target: Some(Target::Structure(structure(s)?)),
                        anchor: None,
                    },
                    Kind::Slime { region, .. } => Resolved {
                        target: None,
                        anchor: anchor(region)?,
                    },
                    Kind::Biome {
                        biome, sampling, ..
                    }
                    | Kind::Coverage {
                        biome, sampling, ..
                    } => {
                        if !(min_y..=max_y).contains(&sampling.y) {
                            return Err(Error::InvalidQuery {
                                line,
                                column: None,
                                message: format!(
                                    "y {} is outside the build height, {min_y} to {max_y}",
                                    sampling.y
                                ),
                            });
                        }
                        Resolved {
                            target: Some(Target::Biome(self.biome_query(biome).map_err(at_line)?)),
                            anchor: None,
                        }
                    }
                    Kind::Spawn { .. } => Resolved {
                        target: None,
                        anchor: None,
                    },
                })
            })
            .collect::<Result<_>>()?;
        Ok(SearchQuery {
            world: self.id,
            query,
            resolved,
        })
    }
}
