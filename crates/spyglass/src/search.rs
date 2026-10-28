//! Seed search (`docs/specs/search.md`, ADRs 0020, 0021, 0024 and 0025): queries resolved
//! against a world or a whole game, and seeds judged against them.

mod evaluate;

use std::ops::ControlFlow;
use std::time::Duration;

use spyglass_search::{
    Anchor, Kind, Named, Place, Prefilter, Query, Region, RunOptions, SPAWN_REACH,
};
use spyglass_structure::PlacementKind;

use evaluate::Sessions;
pub use evaluate::{ConstraintResult, Evaluation, Feature, Outcome};
pub use spyglass_search::{
    Hit as RankedSeed, Progress as SearchProgress, SeedSource, Stop as SearchStop,
};

use crate::{BiomeQuery, Dimension, Error, Game, Result, StructureQuery, World};

/// A search query parsed and resolved by [`World::search_query`] or [`Game::search_query`].
/// Its `Display` is the canonical text.
#[derive(Debug, Clone)]
pub struct SearchQuery {
    /// The id of the world each dimension the query reads was resolved against.
    worlds: [Option<u64>; 3],
    query: Query,
    /// By constraint.
    resolved: Vec<Resolved>,
    /// Necessary conditions on the seed's lower 48 bits, in each dimension that has any.
    prefilters: Vec<(Dimension, Prefilter)>,
    /// Constraint indices, cheapest to check first.
    order: Vec<usize>,
}

/// A constraint's dimension and names, resolved.
#[derive(Debug, Clone)]
struct Resolved {
    /// The dimension it counts in, whose coordinates its places are in.
    dimension: Dimension,
    /// What's counted: a structure for structure counts and clusters, a biome for biome
    /// constraints.
    target: Option<Target>,
    /// The structure of a structure anchor, and the dimension its starts are in.
    anchor: Option<(Dimension, StructureQuery)>,
}

#[derive(Debug, Clone)]
enum Target {
    Structure(StructureQuery),
    Biome(BiomeQuery),
}

impl SearchQuery {
    /// The parsed query, with each dimension written only where it isn't the inferred one.
    pub fn query(&self) -> &Query {
        &self.query
    }

    /// The dimension constraint `index` counts in (`docs/specs/search.md#dimensions`). Panics
    /// if there's no such constraint.
    pub fn dimension(&self, index: usize) -> Dimension {
        self.resolved[index].dimension
    }
}

impl std::fmt::Display for SearchQuery {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.query.fmt(f)
    }
}

/// `[x, z]` in a dimension of coordinate scale `from`, in one of scale `to`, as the game scales
/// a teleport: times `from / to` in `double`, rounded down, kept in `i32`.
pub(crate) fn convert([x, z]: [i32; 2], from: f64, to: f64) -> [i32; 2] {
    let scale = from / to;
    [x, z].map(|v| (f64::from(v) * scale).floor() as i32)
}

/// What decides a constraint's dimension.
#[derive(Clone, Copy)]
enum Subject<'q> {
    /// A structure id or `#tag`.
    Structure(&'q str),
    /// A biome id.
    Biome(&'q str),
    /// Something only the Overworld has, described for errors.
    Overworld(&'static str),
}

/// Where a query is resolved: one world, whose dimension every constraint is in, or a game's
/// dimensions, each opened when a constraint needs it.
#[derive(Clone, Copy)]
enum Scope<'a> {
    World(&'a World),
    Game(&'a Game),
}

impl<'a> Scope<'a> {
    fn world(self, dimension: Dimension) -> Result<&'a World> {
        match self {
            Scope::World(w) if w.dimension_kind() == dimension => Ok(w),
            Scope::World(w) => Err(Error::Unsupported(format!(
                "this needs {}, and the search is in {} only; search every dimension with a Game",
                dimension.name(),
                w.dimension_kind().name()
            ))),
            Scope::Game(g) => g.world(dimension),
        }
    }

    /// Whether `world` places `subject`: some of a structure or tag's structures can start
    /// there, its biome source has the biome, or it's the Overworld.
    fn places(world: &World, subject: Subject<'_>) -> Result<bool> {
        Ok(match subject {
            Subject::Structure(s) => {
                let query = world.structure_query(s)?;
                let structures = world.sets.structures();
                query
                    .targets
                    .iter()
                    .any(|&t| !structures[t].biomes.is_empty())
            }
            Subject::Biome(b) => world.biome_query(b)?.biome.is_some(),
            Subject::Overworld(_) => world.dimension_kind() == Dimension::Overworld,
        })
    }

    /// The dimension `subject` infers: in a world, the world's (the Overworld's for what only
    /// it has); in a game, the first of the Overworld, the Nether and the End that places it,
    /// or the Overworld if none does.
    fn infer(self, subject: Subject<'_>) -> Result<Dimension> {
        let game = match (self, subject) {
            (_, Subject::Overworld(_)) => return Ok(Dimension::Overworld),
            (Scope::World(w), _) => return Ok(w.dimension_kind()),
            (Scope::Game(g), _) => g,
        };
        for d in Dimension::ALL {
            match game.world(d) {
                Ok(world) if Self::places(world, subject)? => return Ok(d),
                // A dimension the bundle doesn't have, or a save can't answer for, places
                // nothing that can be asked about.
                Ok(_) | Err(Error::Unknown { .. } | Error::Unsupported(_)) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(Dimension::Overworld)
    }
}

impl World {
    /// Parses a search query and resolves its structures, tags and biomes for this world's
    /// sessions. Every constraint is in this world's dimension: one written in another, or
    /// that needs another (slime chunks and the spawn are the Overworld's), is an error.
    /// Errors name the line, and the column where the text doesn't parse.
    pub fn search_query(&self, text: &str) -> Result<SearchQuery> {
        resolve(Scope::World(self), text)
    }
}

impl Game {
    /// Parses a search query and resolves it across dimensions
    /// (`docs/specs/search.md#dimensions`): each constraint's dimension, written or inferred,
    /// with its anchors converted into it. Opens the worlds it needs. Errors name the line, and
    /// the column where the text doesn't parse.
    pub fn search_query(&self, text: &str) -> Result<SearchQuery> {
        resolve(Scope::Game(self), text)
    }
}

/// Whether `place` is a disc around the spawn.
fn spawn_anchored(place: &Place) -> bool {
    matches!(
        place,
        Place::Within {
            anchor: Anchor::Spawn,
            ..
        }
    )
}

/// [`World::search_query`] or [`Game::search_query`].
fn resolve(scope: Scope<'_>, text: &str) -> Result<SearchQuery> {
    let mut query = Query::parse(text).map_err(|e| Error::InvalidQuery {
        line: e.line,
        column: Some(e.column),
        message: e.message,
    })?;
    let mut worlds = [None; 3];
    let mut resolved = Vec::with_capacity(query.constraints().len());
    let mut forget = Vec::new();
    for (i, (constraint, &line)) in query.constraints().iter().zip(query.lines()).enumerate() {
        let message = |message: String| Error::InvalidQuery {
            line,
            column: None,
            message,
        };
        let at_line = |e: Error| message(e.to_string());
        let mut world = |d: Dimension| -> Result<&World> {
            let w = scope.world(d).map_err(at_line)?;
            worlds[d.index()] = Some(w.id);
            Ok(w)
        };
        let kind = &constraint.kind;
        let subject = match kind {
            Kind::Structures { structure, .. } | Kind::Cluster { structure, .. } => {
                Subject::Structure(structure)
            }
            Kind::Biome { biome, .. } | Kind::Coverage { biome, .. } => Subject::Biome(biome),
            Kind::Slime { .. } => Subject::Overworld("slime chunks are"),
            Kind::Spawn { .. } => Subject::Overworld("the spawn is"),
        };
        let inferred = scope.infer(subject).map_err(at_line)?;
        let dimension = constraint.dimension.unwrap_or(inferred);
        if constraint.dimension == Some(inferred) {
            forget.push(i);
        }
        if let (Subject::Overworld(what), true) = (subject, dimension != Dimension::Overworld) {
            return Err(message(format!("{what} only in the Overworld")));
        }
        let here = world(dimension)?;
        if constraint.dimension.is_some_and(|d| d != inferred)
            && !Scope::places(here, subject).map_err(at_line)?
        {
            return Err(message(match subject {
                Subject::Structure(s) => format!("{s} isn't placed in {}", dimension.name()),
                Subject::Biome(b) => format!("{} has no {b}", dimension.name()),
                Subject::Overworld(_) => unreachable!("checked above"),
            }));
        }
        // A disc around the spawn in dimension `d` is around the Overworld's, converted.
        let mut spawn_in = |p: &Place, d: Dimension| -> Result<()> {
            if !spawn_anchored(p) {
                Ok(())
            } else if d == Dimension::End {
                Err(message(
                    "`spawn` isn't linked to the End: its coordinates aren't the Overworld's"
                        .to_string(),
                ))
            } else {
                world(Dimension::Overworld).map(|_| ())
            }
        };
        let anchor = match kind {
            Kind::Structures { region, .. } | Kind::Slime { region, .. } => match region {
                Region::Place(p) => {
                    spawn_in(p, dimension)?;
                    None
                }
                Region::NearStructure {
                    structure, place, ..
                } => {
                    // The constraint's own dimension if it places the anchor, else the
                    // anchor's.
                    let subject = Subject::Structure(structure);
                    let d = if Scope::places(here, subject).map_err(at_line)? {
                        dimension
                    } else {
                        scope.infer(subject).map_err(at_line)?
                    };
                    if d != dimension && (d == Dimension::End || dimension == Dimension::End) {
                        return Err(at_line(Error::NotLinked {
                            from: d,
                            to: dimension,
                        }));
                    }
                    spawn_in(place, d)?;
                    let anchor = world(d)?.structure_query(structure).map_err(at_line)?;
                    Some((d, anchor))
                }
            },
            Kind::Cluster { place, .. }
            | Kind::Biome { place, .. }
            | Kind::Coverage { place, .. }
            | Kind::Spawn { place } => {
                spawn_in(place, dimension)?;
                None
            }
        };
        let target = match kind {
            Kind::Structures { structure, .. } | Kind::Cluster { structure, .. } => Some(
                Target::Structure(here.structure_query(structure).map_err(at_line)?),
            ),
            Kind::Biome {
                biome, sampling, ..
            }
            | Kind::Coverage {
                biome, sampling, ..
            } => {
                let heights = here.sets.heights();
                let (min_y, max_y) = (
                    heights.level_min_y,
                    heights.level_min_y + heights.level_height - 1,
                );
                if !(min_y..=max_y).contains(&sampling.y) {
                    return Err(message(format!(
                        "y {} is outside the build height of {}, {min_y} to {max_y}",
                        sampling.y,
                        dimension.name()
                    )));
                }
                Some(Target::Biome(here.biome_query(biome).map_err(at_line)?))
            }
            Kind::Slime { .. } | Kind::Spawn { .. } => None,
        };
        resolved.push(Resolved {
            dimension,
            target,
            anchor,
        });
    }
    for i in forget {
        query.forget_dimension(i);
    }
    let constraints = query.constraints();
    let mut prefilters = Vec::new();
    for d in Dimension::ALL {
        let mine: Vec<usize> = (0..resolved.len())
            .filter(|&i| resolved[i].dimension == d)
            .collect();
        if mine.is_empty() {
            continue;
        }
        let world = scope.world(d)?;
        let subset: Vec<_> = mine.iter().map(|&i| constraints[i].clone()).collect();
        let named: Vec<Named<'_>> = mine
            .iter()
            .map(|&i| {
                let r = &resolved[i];
                Named {
                    target: match &r.target {
                        Some(Target::Structure(q)) => Some(&q.targets[..]),
                        _ => None,
                    },
                    anchor: r
                        .anchor
                        .as_ref()
                        .filter(|(anchor_in, _)| *anchor_in == d)
                        .map(|(_, q)| &q.targets[..]),
                }
            })
            .collect();
        let uses_spawn = mine
            .iter()
            .any(|&i| evaluate::uses_spawn(&constraints[i].kind));
        let reach = if d == Dimension::Overworld || !uses_spawn {
            SPAWN_REACH
        } else {
            // Converted and rounded down, the spawn is at most a block further on each axis.
            let scale =
                scope.world(Dimension::Overworld)?.coordinate_scale() / world.coordinate_scale();
            (f64::from(SPAWN_REACH) * scale).ceil() as u32 + 2
        };
        let prefilter = Prefilter::new(&world.sets, &subset, &named, reach);
        if !prefilter.is_empty() {
            prefilters.push((d, prefilter));
        }
    }
    let costs = (0..resolved.len())
        .map(|i| cost(scope, &constraints[i].kind, &resolved[i]))
        .collect::<Result<Vec<_>>>()?;
    let mut order: Vec<usize> = (0..resolved.len()).collect();
    order.sort_by_key(|&i| costs[i]);
    Ok(SearchQuery {
        worlds,
        query,
        resolved,
        prefilters,
        order,
    })
}

/// A rough rank of how long a constraint takes to check, for ordering checks cheapest first:
/// structures and slime chunks, then those that need the spawn (about 1 ms), the spawn itself,
/// biome grids by size, and last anything on concentric rings (strongholds, about 1 s).
fn cost(scope: Scope<'_>, kind: &Kind, resolved: &Resolved) -> Result<(u8, u64)> {
    let rings = |d: Dimension, q: &StructureQuery| -> Result<bool> {
        Ok(scope.world(d)?.sets.sets().iter().any(|s| {
            s.possible
                && matches!(s.placement.kind, PlacementKind::ConcentricRings(_))
                && s.entries.iter().any(|e| q.targets.contains(&e.structure))
        }))
    };
    let target_on_rings = match &resolved.target {
        Some(Target::Structure(q)) => rings(resolved.dimension, q)?,
        _ => false,
    };
    let anchor_on_rings = match &resolved.anchor {
        Some((d, q)) => rings(*d, q)?,
        None => false,
    };
    Ok(match kind {
        _ if target_on_rings || anchor_on_rings => (4, 0),
        Kind::Biome {
            place, sampling, ..
        }
        | Kind::Coverage {
            place, sampling, ..
        } => (3, place.grid_bound(sampling.step)),
        Kind::Spawn { .. } => (2, 0),
        _ if evaluate::uses_spawn(kind) => (1, 0),
        _ => (0, 0),
    })
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
    /// Decide structure starts with each seed's terrain, as the game does. Slower: terrain is
    /// checked last, for the seeds that pass everything else.
    pub confirm: bool,
    /// Stops the search from another thread, before its next seed. [`SearchStop::stop`] still
    /// evaluates the best seeds found; [`SearchStop::stop_now`] cuts that short too.
    pub stop: SearchStop,
}

impl Default for SearchOptions {
    /// Every seed from index 0, until stopped, keeping the best 10, on all cores, unconfirmed.
    fn default() -> Self {
        SearchOptions {
            source: SeedSource::All,
            from: 0,
            seeds: None,
            time: None,
            top: 10,
            threads: None,
            certain: false,
            confirm: false,
            stop: SearchStop::new(),
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
    /// Whether [`SearchStop::stop_now`] cut evaluating the ranked seeds short: then `hits` has
    /// only the best of `progress.top`, in rank order.
    pub partial: bool,
}

impl World {
    /// Searches seeds for `query`, keeping the best (ADR 0021). Runs on its own thread pool;
    /// calls `progress` between rounds of work with the best so far, and stops when the budget
    /// in `options` is spent, the source runs out, `progress` returns `Break`, or
    /// [`SearchOptions::stop`] is asked. Then the best seeds are evaluated, in parallel. The same
    /// query, source, `from` and seed count give the same hits on any machine and any number
    /// of threads. The query must be in this world's dimension only: [`Game::search`] searches
    /// any.
    pub fn search(
        &self,
        query: &SearchQuery,
        options: &SearchOptions,
        progress: impl FnMut(&SearchProgress) -> ControlFlow<()>,
    ) -> Result<SearchOutcome> {
        self.session(0)?.check_worlds(query)?;
        search(
            query,
            options,
            |seed| {
                query
                    .prefilters
                    .iter()
                    .all(|(_, p)| p.accepts(&self.sets, seed))
            },
            |seed, threshold| {
                self.session(seed)?
                    .check(query, options.certain, threshold, options.confirm)
            },
            |seed| self.session(seed)?.evaluate(query, options.confirm),
            progress,
        )
    }
}

impl Game {
    /// [`World::search`] across dimensions: each seed's dimensions are bound only as its
    /// constraints need them, and the placement prefilter runs in every dimension that has
    /// one.
    pub fn search(
        &self,
        query: &SearchQuery,
        options: &SearchOptions,
        progress: impl FnMut(&SearchProgress) -> ControlFlow<()>,
    ) -> Result<SearchOutcome> {
        self.session(0).check_worlds(query)?;
        let prefilters = query
            .prefilters
            .iter()
            .map(|(d, p)| Ok((self.world(*d)?, p)))
            .collect::<Result<Vec<_>>>()?;
        search(
            query,
            options,
            |seed| prefilters.iter().all(|(w, p)| p.accepts(&w.sets, seed)),
            |seed, threshold| {
                self.session(seed)
                    .check(query, options.certain, threshold, options.confirm)
            },
            |seed| self.session(seed).evaluate(query, options.confirm),
            progress,
        )
    }
}

/// [`World::search`] or [`Game::search`], given the prefilter, the fast path and the plain
/// evaluation for a seed.
fn search(
    query: &SearchQuery,
    options: &SearchOptions,
    accepts: impl Fn(i64) -> bool + Sync,
    check: impl Fn(i64, u64) -> Result<spyglass_search::Checked> + Sync,
    evaluate: impl Fn(i64) -> Result<Evaluation> + Sync,
    progress: impl FnMut(&SearchProgress) -> ControlFlow<()>,
) -> Result<SearchOutcome> {
    let pool = spyglass_search::pool(options.threads)
        .map_err(|e| Error::Threads(crate::Cause(Box::new(e))))?;
    let run = RunOptions {
        source: options.source.clone(),
        from: options.from,
        seeds: options.seeds,
        time: options.time,
        top: options.top,
        stop: options.stop.clone(),
    };
    let progress = spyglass_search::run(&pool, &run, accepts, check, progress)?;
    let stop = || options.stop.is_stopped_now();
    let hits = spyglass_search::map_prefix(&pool, &progress.top, stop, |hit| {
        let evaluation = evaluate(hit.seed)?;
        debug_assert!(
            evaluation.reported(options.certain) && evaluation.score(options.certain) == hit.score,
            "the fast path disagrees with evaluate on seed {}: {query}",
            hit.seed
        );
        Ok::<_, Error>(SearchHit {
            index: hit.index,
            seed: hit.seed,
            score: hit.score,
            evaluation,
        })
    })?;
    let partial = hits.len() < progress.top.len();
    Ok(SearchOutcome {
        progress,
        hits,
        partial,
    })
}

#[cfg(test)]
mod tests {
    use spyglass_oracle::fixtures;
    use spyglass_search::Checked;

    use crate::StructureStart;

    use super::*;

    /// Whether `world`'s prefilters accept `seed` for `query`.
    fn accepts(world: &World, query: &SearchQuery, seed: i64) -> bool {
        query
            .prefilters
            .iter()
            .all(|(_, p)| p.accepts(&world.sets, seed))
    }

    /// Queries the prefilter has something to say about, and some it doesn't.
    const QUERIES: [&str; 6] = [
        "require 3 hut within 150 of 40,-30\nprefer 2 tower within 300 of 0,0",
        "require 2 tower within 150 of hut in -400,-400 to 0,0\nprefer no hut within 100 of -50,0",
        "require cluster 3 hut within 120 of each other in -300,-300 to 300,300",
        "require 16 slime in -100,-100 to 100,100\nrequire monument within 400 of 0,0",
        "require 4 hut within 100 of spawn\nprefer biome plains covers 40% within 200 of spawn step 32",
        "require 2 keep within 2000 of 0,0\nprefer 2 hut within 300 of -200,0",
    ];

    /// Terrain only narrows a result: the confirmed best case is no better than the
    /// unconfirmed one, and the confirmed worst case no worse. The confirmed fast path relies
    /// on it.
    fn narrows(plain: &Evaluation, confirmed: &Evaluation, context: &str) {
        assert!(confirmed.best_score <= plain.best_score, "{context}");
        assert!(confirmed.worst_score >= plain.worst_score, "{context}");
        for (p, c) in plain.constraints.iter().zip(&confirmed.constraints) {
            assert!(c.best <= p.best && c.worst >= p.worst, "{context}");
            // Met stays met, failed stays failed.
            if p.outcome != Outcome::PossiblyMet {
                assert_eq!(c.outcome, p.outcome, "{context}");
            }
        }
    }

    /// For every seed in a range: a seed the prefilter rejects is never reported, the prefilter
    /// answers the same for every upper 16 bits, `check` gives `evaluate`'s answer, confirmed
    /// or not, and confirming only narrows. Counts rejected seeds, reported seeds and
    /// constraints that confirming decided.
    fn fast_path_agrees(world: &World, seeds: impl Iterator<Item = i64>) -> (usize, usize, usize) {
        let queries: Vec<SearchQuery> = QUERIES
            .iter()
            .map(|q| world.search_query(q).unwrap())
            .collect();
        let (mut rejected, mut reported, mut decided) = (0, 0, 0);
        for seed in seeds {
            let session = world.session(seed).unwrap();
            for query in &queries {
                let accepts = accepts(world, query, seed);
                let flipped = seed ^ (0x5a5a << 48);
                assert_eq!(accepts, self::accepts(world, query, flipped));
                let plain = session.evaluate(query, false).unwrap();
                let confirmed = session.evaluate(query, true).unwrap();
                narrows(&plain, &confirmed, &format!("seed {seed}: {query}"));
                decided += plain
                    .constraints
                    .iter()
                    .zip(&confirmed.constraints)
                    .filter(|(p, c)| p.outcome != c.outcome)
                    .count();
                if !accepts {
                    rejected += 1;
                    assert_eq!(plain.verdict, Outcome::Failed, "seed {seed}: {query}");
                }
                for (confirm, e) in [(false, &plain), (true, &confirmed)] {
                    for certain in [false, true] {
                        reported += usize::from(e.reported(certain));
                        for threshold in [0, 1, 500, 1000, 1500, 2501] {
                            let expected = match (e.reported(certain), e.score(certain)) {
                                (false, _) => Checked::Failed,
                                (true, s) if s >= threshold => Checked::Scored(s),
                                (true, _) => Checked::Below,
                            };
                            assert_eq!(
                                session.check(query, certain, threshold, confirm).unwrap(),
                                expected,
                                "seed {seed} confirm {confirm} certain {certain} threshold {threshold}: {query}"
                            );
                        }
                    }
                }
            }
        }
        (rejected, reported, decided)
    }

    #[test]
    fn the_fast_path_agrees_with_evaluate() {
        let fixture = fixtures::structured_overworld(false);
        let world = World::open(fixture.path()).unwrap();
        let (rejected, reported, _) = fast_path_agrees(&world, 0..200);
        assert!(rejected > 100, "the prefilter rejected only {rejected}");
        assert!(reported > 100, "only {reported} reported");
    }

    /// Queries across the Overworld and the Nether: spawn anchors converted, structure anchors
    /// in the other dimension both ways, a tag picked by its written dimension, and constraints
    /// in each dimension side by side.
    const GAME_QUERIES: [&str; 6] = [
        "require 2 fort within 200 of spawn\nprefer tower within 300 of spawn",
        "require fort within 40 of tower within 400 of 0,0\nrequire 2 fort in 0,0 to 47,47",
        "require tower within 300 of fort in -50,-50 to 50,50\nprefer 2 hut within 300 of fort within 60 of 0,0",
        "require nether #minecraft:mixed within 100 of spawn\nprefer cluster 2 fort within 60 of each other within 150 of 0,0",
        "require 3 slime in -64,-64 to 64,64\nrequire biome nether_wastes within 50 of spawn\nprefer no fort within 30 of 10,10",
        "require 3 fort within 50 of spawn\nrequire 2 hut within 200 of spawn\nprefer 2 fort within 30 of fort within 80 of spawn",
    ];

    /// [`fast_path_agrees`] for a game: every query's prefilters in every dimension, and
    /// `GameSession::check` against `GameSession::evaluate`. Counts rejected and reported seeds.
    fn game_fast_path_agrees(game: &Game, seeds: impl Iterator<Item = i64>) -> (usize, usize) {
        let queries: Vec<SearchQuery> = GAME_QUERIES
            .iter()
            .map(|q| game.search_query(q).unwrap_or_else(|e| panic!("{q}: {e}")))
            .collect();
        let accepts = |query: &SearchQuery, seed| {
            query
                .prefilters
                .iter()
                .all(|(d, p)| p.accepts(&game.world(*d).unwrap().sets, seed))
        };
        let (mut rejected, mut reported) = (0, 0);
        for seed in seeds {
            let session = game.session(seed);
            for query in &queries {
                let accepted = accepts(query, seed);
                assert_eq!(accepted, accepts(query, seed ^ (0x5a5a << 48)));
                let plain = session.evaluate(query, false).unwrap();
                let confirmed = session.evaluate(query, true).unwrap();
                narrows(&plain, &confirmed, &format!("seed {seed}: {query}"));
                if !accepted {
                    rejected += 1;
                    assert_eq!(plain.verdict, Outcome::Failed, "seed {seed}: {query}");
                }
                for (confirm, e) in [(false, &plain), (true, &confirmed)] {
                    for certain in [false, true] {
                        reported += usize::from(e.reported(certain));
                        for threshold in [0, 1, 500, 1000, 2501] {
                            let expected = match (e.reported(certain), e.score(certain)) {
                                (false, _) => Checked::Failed,
                                (true, s) if s >= threshold => Checked::Scored(s),
                                (true, _) => Checked::Below,
                            };
                            assert_eq!(
                                session.check(query, certain, threshold, confirm).unwrap(),
                                expected,
                                "seed {seed} confirm {confirm} certain {certain} threshold {threshold}: {query}"
                            );
                        }
                    }
                }
            }
        }
        (rejected, reported)
    }

    #[test]
    fn the_fast_path_agrees_with_evaluate_across_dimensions() {
        let fixture = fixtures::structured_overworld_and_nether();
        let game = Game::open(fixture.path()).unwrap();
        let (rejected, reported) = game_fast_path_agrees(&game, 0..150);
        assert!(rejected > 100, "the prefilter rejected only {rejected}");
        assert!(reported > 100, "only {reported} reported");
    }

    #[test]
    fn spawn_anchors_convert_into_the_nether() {
        let fixture = fixtures::structured_overworld_and_nether();
        let game = Game::open(fixture.path()).unwrap();
        let query = game
            .search_query("require 1 fort within 120 of spawn")
            .unwrap();
        for seed in 0..40 {
            let session = game.session(seed);
            let spawn = session
                .dimension(Dimension::Overworld)
                .unwrap()
                .spawn_origin()
                .map_block();
            let [x, z] = spawn.map(|v| v.div_euclid(8));
            let nether = session.dimension(Dimension::Nether).unwrap();
            let fort = game
                .world(Dimension::Nether)
                .unwrap()
                .structure_query("fort")
                .unwrap();
            let chunks = |v: i32| [(v - 120).div_euclid(16) - 1, (v + 120).div_euclid(16) + 1];
            let ([x0, x1], [z0, z1]) = (chunks(x), chunks(z));
            let near: Vec<StructureStart> = nether
                .structures_in(&fort, [x0, z0], [x1, z1], false)
                .unwrap()
                .into_iter()
                .filter(|s| distance_sqr_i64([x, z], [s.position[0], s.position[2]]) <= 120 * 120)
                .collect();
            let evaluation = session.evaluate(&query, false).unwrap();
            let result = &evaluation.constraints[0];
            assert_eq!(result.dimension, Dimension::Nether);
            let found: Vec<&StructureStart> = result
                .features
                .iter()
                .map(|f| match f {
                    Feature::Structure(s) => s,
                    other => panic!("{other:?}"),
                })
                .collect();
            assert_eq!(found.len(), near.len(), "seed {seed}");
            assert!(found.iter().all(|s| near.contains(s)), "seed {seed}");
        }
    }

    fn distance_sqr_i64(a: [i32; 2], b: [i32; 2]) -> i64 {
        let d = |i: usize| i64::from(a[i]) - i64::from(b[i]);
        d(0) * d(0) + d(1) * d(1)
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
    #[ignore = "needs bundles/<version> (SPYGLASS_ORACLE_VERSION, else the newest release)"]
    fn the_fast_path_agrees_with_evaluate_on_the_real_world() {
        let dir = spyglass_oracle::bundle_dir();
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
        let (mut rejected, mut decided) = (0, 0);
        for seed in (0..40).map(|i: i64| i.wrapping_mul(0x1e37_79b9_7f4a_7c15)) {
            let session = world.session(seed).unwrap();
            for query in &queries {
                let accepts = accepts(&world, query, seed);
                let plain = session.evaluate(query, false).unwrap();
                let confirmed = session.evaluate(query, true).unwrap();
                narrows(&plain, &confirmed, &format!("seed {seed}: {query}"));
                decided += usize::from(plain != confirmed);
                if !accepts {
                    rejected += 1;
                    assert_eq!(plain.verdict, Outcome::Failed, "seed {seed}: {query}");
                }
                for (confirm, e) in [(false, &plain), (true, &confirmed)] {
                    for certain in [false, true] {
                        let expected = if e.reported(certain) {
                            Checked::Scored(e.score(certain))
                        } else {
                            Checked::Failed
                        };
                        assert_eq!(
                            session.check(query, certain, 0, confirm).unwrap(),
                            expected,
                            "seed {seed} confirm {confirm} certain {certain}: {query}"
                        );
                    }
                }
            }
        }
        assert!(rejected > 20, "the prefilter rejected only {rejected}");
        assert!(
            decided > 20,
            "confirming changed only {decided} evaluations"
        );
    }

    #[test]
    #[ignore = "needs bundles/<version> (SPYGLASS_ORACLE_VERSION, else the newest release)"]
    fn the_fast_path_agrees_across_dimensions_on_the_real_game() {
        let dir = spyglass_oracle::bundle_dir();
        let game = Game::open(dir).unwrap();
        let queries = [
            "require minecraft:fortress within 300 of spawn\nprefer minecraft:bastion_remnant within 200 of minecraft:fortress within 1000 of spawn",
            "require nether #minecraft:ruined_portal within 200 of spawn\nprefer #minecraft:village within 400 of spawn",
            "require minecraft:end_city within 1500 of 0,0\nprefer biome minecraft:end_highlands within 800 of 0,0 step 64",
            "require minecraft:fortress within 150 of #minecraft:village within 1200 of spawn",
            "require 2 minecraft:nether_fossil within 100 of 0,0\nprefer biome minecraft:soul_sand_valley within 200 of spawn step 32",
            "require 3 minecraft:fortress in 0,0 to 600,600\nprefer minecraft:end_city within 2000 of 0,0",
        ];
        let queries: Vec<SearchQuery> = queries
            .iter()
            .map(|q| game.search_query(q).unwrap_or_else(|e| panic!("{q}: {e}")))
            .collect();
        let d = |q: &SearchQuery, i| q.dimension(i);
        assert_eq!(d(&queries[0], 1), Dimension::Nether);
        assert_eq!(d(&queries[1], 1), Dimension::Overworld);
        assert_eq!(d(&queries[2], 0), Dimension::End);
        // The End isn't linked to the others, and has no spawn anchor.
        for text in [
            "require minecraft:end_city within 100 of spawn",
            "require minecraft:end_city within 100 of minecraft:fortress in 0,0 to 10,10",
            "require minecraft:fortress within 100 of minecraft:end_city in 0,0 to 10,10",
        ] {
            assert!(game.search_query(text).is_err(), "{text}");
        }
        let (mut rejected, mut reported, mut decided) = (0, 0, 0);
        for seed in (0..30).map(|i: i64| i.wrapping_mul(0x1e37_79b9_7f4a_7c15)) {
            let session = game.session(seed);
            for query in &queries {
                let accepts = query
                    .prefilters
                    .iter()
                    .all(|(d, p)| p.accepts(&game.world(*d).unwrap().sets, seed));
                let plain = session.evaluate(query, false).unwrap();
                let confirmed = session.evaluate(query, true).unwrap();
                narrows(&plain, &confirmed, &format!("seed {seed}: {query}"));
                decided += usize::from(plain != confirmed);
                if !accepts {
                    rejected += 1;
                    assert_eq!(plain.verdict, Outcome::Failed, "seed {seed}: {query}");
                }
                for (confirm, e) in [(false, &plain), (true, &confirmed)] {
                    for certain in [false, true] {
                        reported += usize::from(e.reported(certain));
                        let expected = if e.reported(certain) {
                            Checked::Scored(e.score(certain))
                        } else {
                            Checked::Failed
                        };
                        assert_eq!(
                            session.check(query, certain, 0, confirm).unwrap(),
                            expected,
                            "seed {seed} confirm {confirm} certain {certain}: {query}"
                        );
                    }
                }
            }
        }
        println!("{rejected} rejected, {reported} reported, {decided} decided by terrain");
        assert!(rejected > 10, "the prefilter rejected only {rejected}");
        assert!(reported > 10, "only {reported} reported");
    }
}
