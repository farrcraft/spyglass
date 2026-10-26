//! Seed search (`docs/specs/search.md`, ADRs 0020 and 0021): queries resolved against a world,
//! and seeds judged against them.

mod evaluate;

use std::ops::ControlFlow;
use std::time::Duration;

use spyglass_search::{Kind, Named, Prefilter, Query, Region, RunOptions};
use spyglass_structure::PlacementKind;

pub use evaluate::{ConstraintResult, Evaluation, Feature, Outcome};
pub use spyglass_search::{Hit as RankedSeed, Progress as SearchProgress, SeedSource};

use crate::{BiomeQuery, Error, Result, StructureQuery, World};

/// A search query parsed and resolved by [`World::search_query`]. Its `Display` is the
/// canonical text.
#[derive(Debug, Clone)]
pub struct SearchQuery {
    world: u64,
    query: Query,
    /// By constraint.
    resolved: Vec<Resolved>,
    /// Necessary conditions on the seed's lower 48 bits.
    prefilter: Prefilter,
    /// Constraint indices, cheapest to check first.
    order: Vec<usize>,
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
            .collect::<Result<Vec<Resolved>>>()?;
        let named: Vec<Named<'_>> = resolved
            .iter()
            .map(|r| Named {
                target: match &r.target {
                    Some(Target::Structure(q)) => Some(&q.targets[..]),
                    _ => None,
                },
                anchor: r.anchor.as_ref().map(|q| &q.targets[..]),
            })
            .collect();
        let prefilter = Prefilter::new(&self.sets, query.constraints(), &named);
        let mut order: Vec<usize> = (0..resolved.len()).collect();
        order.sort_by_key(|&i| self.cost(&query.constraints()[i].kind, &resolved[i]));
        Ok(SearchQuery {
            world: self.id,
            query,
            resolved,
            prefilter,
            order,
        })
    }

    /// A rough rank of how long a constraint takes to check, for ordering checks cheapest
    /// first: structures and slime chunks, then those that need the spawn (about 1 ms), the
    /// spawn itself, biome grids by size, and last anything on concentric rings (strongholds,
    /// about 1 s).
    fn cost(&self, kind: &Kind, resolved: &Resolved) -> (u8, u64) {
        let rings = |q: &StructureQuery| {
            self.sets.sets().iter().any(|s| {
                s.possible
                    && matches!(s.placement.kind, PlacementKind::ConcentricRings(_))
                    && s.entries.iter().any(|e| q.targets.contains(&e.structure))
            })
        };
        let on_rings = matches!(&resolved.target, Some(Target::Structure(q)) if rings(q))
            || resolved.anchor.as_ref().is_some_and(rings);
        match kind {
            _ if on_rings => (4, 0),
            Kind::Biome {
                place, sampling, ..
            }
            | Kind::Coverage {
                place, sampling, ..
            } => (3, place.grid_bound(sampling.step)),
            Kind::Spawn { .. } => (2, 0),
            _ if evaluate::uses_spawn(kind) => (1, 0),
            _ => (0, 0),
        }
    }
}

/// How [`World::search`] runs (`docs/specs/search.md#seed-sources`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchOptions {
    /// The seeds.
    pub source: SeedSource,
    /// The index in the source to start at.
    pub from: u64,
    /// Evaluate exactly this many indices, or to the end of the source.
    pub seeds: Option<u64>,
    /// Stop once this much time has passed; the outcome's `next` says where.
    pub time: Option<Duration>,
    /// How many seeds to keep; at least 1.
    pub top: usize,
    /// Worker threads; all cores if `None`.
    pub threads: Option<usize>,
    /// Report only seeds whose required constraints are certainly met, and rank by the worst
    /// case.
    pub certain: bool,
}

impl Default for SearchOptions {
    /// Every seed from index 0, until stopped, keeping the best 10, on all cores.
    fn default() -> Self {
        SearchOptions {
            source: SeedSource::All,
            from: 0,
            seeds: None,
            time: None,
            top: 10,
            threads: None,
            certain: false,
        }
    }
}

/// A reported seed with its evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    /// Its index in the source.
    pub index: u64,
    /// The seed.
    pub seed: i64,
    /// The score it ranked by.
    pub score: u64,
    /// Every constraint's result, from [`Session::evaluate`](crate::Session::evaluate).
    pub evaluation: Evaluation,
}

/// How a search ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchOutcome {
    /// The counts and the ranked seeds when it stopped.
    pub progress: SearchProgress,
    /// The ranked seeds, best first, with their evaluations.
    pub hits: Vec<SearchHit>,
}

impl World {
    /// Searches seeds for `query`, keeping the best (ADR 0021). Runs on its own thread pool;
    /// calls `progress` between rounds of work with the best so far, and stops when the budget
    /// in `options` is spent, the source runs out, or `progress` returns `Break`. The same
    /// query, source, `from` and seed count give the same hits on any machine and any number
    /// of threads.
    pub fn search(
        &self,
        query: &SearchQuery,
        options: &SearchOptions,
        progress: impl FnMut(&SearchProgress) -> ControlFlow<()>,
    ) -> Result<SearchOutcome> {
        if query.world != self.id {
            return Err(Error::OtherWorld(query.to_string().trim_end().to_string()));
        }
        let run = RunOptions {
            source: options.source.clone(),
            from: options.from,
            seeds: options.seeds,
            time: options.time,
            top: options.top,
            threads: options.threads,
        };
        let accepts =
            |seed| query.prefilter.is_empty() || query.prefilter.accepts(&self.sets, seed);
        let check = |seed, threshold| self.session(seed)?.check(query, options.certain, threshold);
        let progress = spyglass_search::run(&run, accepts, check, progress)?;
        let hits = progress
            .top
            .iter()
            .map(|hit| {
                let evaluation = self.session(hit.seed)?.evaluate(query)?;
                debug_assert!(
                    evaluation.reported(options.certain)
                        && evaluation.score(options.certain) == hit.score,
                    "the fast path disagrees with evaluate on seed {}",
                    hit.seed
                );
                Ok(SearchHit {
                    index: hit.index,
                    seed: hit.seed,
                    score: hit.score,
                    evaluation,
                })
            })
            .collect::<Result<_>>()?;
        Ok(SearchOutcome { progress, hits })
    }
}

#[cfg(test)]
mod tests {
    use spyglass_oracle::fixtures;

    use super::*;

    /// Queries the prefilter has something to say about, and some it doesn't.
    const QUERIES: [&str; 6] = [
        "require 3 hut within 150 of 40,-30\nprefer 2 tower within 300 of 0,0",
        "require 2 tower within 150 of hut in -400,-400 to 0,0\nprefer no hut within 100 of -50,0",
        "require cluster 3 hut within 120 of each other in -300,-300 to 300,300",
        "require 16 slime in -100,-100 to 100,100\nrequire monument within 400 of 0,0",
        "require 4 hut within 100 of spawn\nprefer biome plains covers 40% within 200 of spawn step 32",
        "require 2 keep within 2000 of 0,0\nprefer 2 hut within 300 of -200,0",
    ];

    /// For every seed in a range: a seed the prefilter rejects is never reported, the prefilter
    /// answers the same for every upper 16 bits, and `check` gives `evaluate`'s answer.
    fn fast_path_agrees(world: &World, seeds: impl Iterator<Item = i64>) -> (usize, usize) {
        let queries: Vec<SearchQuery> = QUERIES
            .iter()
            .map(|q| world.search_query(q).unwrap())
            .collect();
        let (mut rejected, mut reported) = (0, 0);
        for seed in seeds {
            let session = world.session(seed).unwrap();
            for query in &queries {
                let accepts = query.prefilter.accepts(&world.sets, seed);
                let flipped = seed ^ (0x5a5a << 48);
                assert_eq!(accepts, query.prefilter.accepts(&world.sets, flipped));
                let e = session.evaluate(query).unwrap();
                if !accepts {
                    rejected += 1;
                    assert_eq!(e.verdict, Outcome::Failed, "seed {seed}: {query}");
                }
                for certain in [false, true] {
                    reported += usize::from(e.reported(certain));
                    let expected = e.reported(certain).then(|| e.score(certain));
                    for threshold in [0, 1, 500, 1000, 1500, 2501] {
                        assert_eq!(
                            session.check(query, certain, threshold).unwrap(),
                            expected.filter(|&s| s >= threshold),
                            "seed {seed} certain {certain} threshold {threshold}: {query}"
                        );
                    }
                }
            }
        }
        (rejected, reported)
    }

    #[test]
    fn the_fast_path_agrees_with_evaluate() {
        let fixture = fixtures::structured_overworld(false);
        let world = World::open(fixture.path()).unwrap();
        let (rejected, reported) = fast_path_agrees(&world, 0..200);
        assert!(rejected > 100, "the prefilter rejected only {rejected}");
        assert!(reported > 100, "only {reported} reported");
    }

    #[test]
    fn checks_run_cheapest_first() {
        let fixture = fixtures::structured_overworld(false);
        let world = World::open(fixture.path()).unwrap();
        let query = world
            .search_query(
                "require keep within 2000 of 0,0\n\
                 require biome plains within 300 of 0,0\n\
                 prefer spawn within 10 of 0,0\n\
                 require hut within 100 of spawn\n\
                 require biome plains within 30 of 0,0\n\
                 require tower within 100 of 0,0",
            )
            .unwrap();
        assert_eq!(query.order, [5, 3, 2, 4, 1, 0]);
    }

    #[test]
    #[ignore = "needs a bundle: spyglass-extract 26.3"]
    fn the_fast_path_agrees_with_evaluate_on_the_real_world() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../bundles/26.3");
        let world = World::open(dir).unwrap();
        let queries = [
            "require 3 minecraft:swamp_hut within 1500 of 0,0\nprefer 2 #minecraft:village within 800 of 0,0",
            "require cluster 2 minecraft:swamp_hut within 300 of each other within 3000 of spawn",
            "require #minecraft:village within 300 of minecraft:pillager_outpost within 1000 of 0,0",
            "require 2 #minecraft:ocean_ruin within 200 of 0,0\nrequire 3 slime in -64,-64 to 64,64",
            "require minecraft:stronghold within 1500 of 0,0\nprefer minecraft:trial_chambers within 500 of 0,0",
        ];
        let queries: Vec<SearchQuery> = queries
            .iter()
            .map(|q| world.search_query(q).unwrap())
            .collect();
        let mut rejected = 0;
        for seed in (0..40).map(|i: i64| i.wrapping_mul(0x1e37_79b9_7f4a_7c15)) {
            let session = world.session(seed).unwrap();
            for query in &queries {
                let accepts = query.prefilter.accepts(&world.sets, seed);
                let e = session.evaluate(query).unwrap();
                if !accepts {
                    rejected += 1;
                    assert_eq!(e.verdict, Outcome::Failed, "seed {seed}: {query}");
                }
                for certain in [false, true] {
                    let expected = e.reported(certain).then(|| e.score(certain));
                    assert_eq!(session.check(query, certain, 0).unwrap(), expected);
                }
            }
        }
        assert!(rejected > 20, "the prefilter rejected only {rejected}");
    }
}
