//! Searching within one seed: a `find` query's targets, nearest first, each judged with `here`
//! set to it by the seed search's evaluator (ADR 0047, `docs/specs/search.md`).

use std::ops::ControlFlow;
use std::time::{Duration, Instant};

use spyglass_search::{Anchor, Checked, Place, distance_sqr, map_prefix_throttled};

use super::evaluate::{Area, Here, Sessions, check_at, evaluate_at, lookup, sample, starts_in};
use super::{Looked, SearchQuery, SearchStop, SearchThrottle, convert};
use crate::features::Placings;
use crate::loot::LootPlacings;
use crate::{
    Dimension, Error, Evaluation, Feature, FeatureOptions, GameSession, Outcome, Result, Session,
    StructureStart,
};

/// The most targets a `find` query may have.
pub const MAX_FIND_TARGETS: usize = 1 << 20;

/// How [`Session::find`] runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FindOptions {
    /// The block `within R` alone is around, in the target's dimension's coordinates: the
    /// command's `--from`, the viewer's view centre.
    pub origin: [i32; 2],
    /// How many matches to find; at least 1.
    pub top: usize,
    /// Worker threads; all cores if `None`.
    pub threads: Option<usize>,
    /// Report only matches whose required constraints are certainly met, at targets that
    /// certainly are what's looked for.
    pub certain: bool,
    /// Decide structure starts with the seed's terrain, as the game does.
    pub confirm: bool,
    /// Stop once this much time has passed.
    pub time: Option<Duration>,
    /// Stops the search from another thread, before its next target.
    pub stop: SearchStop,
    /// Limits how many of its threads work at once, from another thread while it runs.
    pub throttle: SearchThrottle,
}

impl Default for FindOptions {
    /// From (0, 0), the first 100 matches, on all cores, unconfirmed, with no time limit.
    fn default() -> Self {
        FindOptions {
            origin: [0, 0],
            top: 100,
            threads: None,
            certain: false,
            confirm: false,
            time: None,
            stop: SearchStop::new(),
            throttle: SearchThrottle::new(),
        }
    }
}

/// A target that met a `find` query.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct FindMatch {
    /// Where `here` was: the start's or the grid point's block `[x, z]`, in the target's
    /// dimension.
    pub position: [i32; 2],
    /// What's there: the structure start, the feature, or the biome sample. `None` for a
    /// point.
    pub target: Option<Feature>,
    /// Its distance from the find place's centre, `isqrt(dx² + dz²)`.
    pub distance: u32,
    /// Its score: the preferred constraints' sum, in the case [`FindOptions::certain`] picks.
    pub score: u64,
    /// Every constraint's result with `here` at this target. Its verdict is possibly met if the
    /// target itself is uncertain (a candidate start or feature, or a tied biome sample).
    pub evaluation: Evaluation,
}

/// How a `find` is going, or how it ended.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct FindProgress {
    /// How many targets there are.
    pub targets: usize,
    /// How many have been judged, nearest first.
    pub judged: usize,
    /// How many matches so far.
    pub matches: usize,
    /// Time since the search started.
    pub elapsed: Duration,
}

/// How a `find` ended.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct FindOutcome {
    /// The counts when it stopped.
    pub progress: FindProgress,
    /// The matches, nearest first.
    pub matches: Vec<FindMatch>,
    /// Whether every target was judged, or `top` matches found: otherwise a stop or the time
    /// limit cut it short, and `matches` are those among the targets judged.
    pub complete: bool,
}

impl Session<'_> {
    /// Searches this seed for `query`'s targets that meet its constraints, nearest first: a
    /// `find` query (ADR 0047). Calls `progress` after each batch of targets. The same query,
    /// seed and origin give the same matches on any machine and any number of threads. The
    /// query must be in this session's dimension only: [`GameSession::find`] judges any.
    pub fn find(
        &self,
        query: &SearchQuery,
        options: &FindOptions,
        progress: impl FnMut(&FindProgress) -> ControlFlow<()>,
    ) -> Result<FindOutcome> {
        find(self, query, options, progress)
    }
}

impl GameSession<'_> {
    /// [`Session::find`] across dimensions: each is bound as the query needs it.
    pub fn find(
        &self,
        query: &SearchQuery,
        options: &FindOptions,
        progress: impl FnMut(&FindProgress) -> ControlFlow<()>,
    ) -> Result<FindOutcome> {
        find(self, query, options, progress)
    }
}

/// A target, before it's judged.
struct Target {
    /// Its block `[x, z]`, in the target's dimension: where `here` is set.
    position: [i32; 2],
    /// Its distance squared from the find place's centre.
    distance_sqr: u64,
    /// Whether it certainly is what's looked for.
    certain: bool,
    /// The structure start, feature or biome sample it is; `None` for a point.
    feature: Option<Feature>,
}

fn find<'w>(
    sessions: &(dyn Sessions<'w> + Sync),
    query: &SearchQuery,
    options: &FindOptions,
    mut progress: impl FnMut(&FindProgress) -> ControlFlow<()>,
) -> Result<FindOutcome> {
    sessions.check_worlds(query)?;
    let found = query.found().ok_or_else(|| {
        Error::Unsupported("this query has no `find` line: search seeds with it instead".into())
    })?;
    let start = Instant::now();
    let dimension = found.dimension;
    let session = sessions.session(dimension)?;
    let place = query.query.find().expect("resolved from a find line").place;
    let (area, centre) = find_area(sessions, &place, dimension, options.origin)?;
    let mut targets = enumerate(session, query, area, centre, options.confirm)?;
    if options.certain {
        targets.retain(|t| t.certain);
    }
    let apart = query.query.find().map_or(0, |f| f.apart);
    let pool = spyglass_search::pool(options.threads)
        .map_err(|e| Error::Threads(crate::Cause(Box::new(e))))?;
    let top = options.top.max(1);
    let batch = 4 * pool.current_num_threads().max(1);
    let deadline = options.time.map(|t| start + t);
    // The targets are judged in batches, nearest first, each batch in parallel. A batch's
    // matches are kept in target order, so the matches are the same whatever the threads do:
    // the first `top` targets that match and aren't within `apart` of a nearer match.
    let mut matches: Vec<FindMatch> = Vec::new();
    let mut judged = 0;
    let mut cut = false;
    let near_a_match = |position: [i32; 2], matches: &[FindMatch]| {
        apart > 0
            && matches
                .iter()
                .any(|m| distance_sqr(m.position, position) <= u64::from(apart).pow(2))
    };
    while judged < targets.len() && matches.len() < top {
        if options.stop.is_stopped() || deadline.is_some_and(|d| Instant::now() >= d) {
            cut = true;
            break;
        }
        let end = (judged + batch).min(targets.len());
        // Matches only grow, so a target near one now is skipped whatever the batch finds.
        let todo: Vec<usize> = (judged..end)
            .filter(|&i| !near_a_match(targets[i].position, &matches))
            .collect();
        let results = map_prefix_throttled(&pool, &todo, &options.stop, &options.throttle, |&i| {
            judge(sessions, query, found.dimension, &targets[i], options)
        })?;
        let done = results.len();
        for (&i, result) in todo.iter().zip(results) {
            let Some(found_match) = result else {
                continue;
            };
            if matches.len() < top && !near_a_match(targets[i].position, &matches) {
                matches.push(found_match);
            }
        }
        if done < todo.len() {
            // Stopped part way: what's kept is every target before the first not judged.
            judged = todo[done];
            cut = true;
            break;
        }
        judged = end;
        let report = FindProgress {
            targets: targets.len(),
            judged,
            matches: matches.len(),
            elapsed: start.elapsed(),
        };
        if progress(&report).is_break() {
            cut = judged < targets.len() && matches.len() < top;
            break;
        }
    }
    Ok(FindOutcome {
        progress: FindProgress {
            targets: targets.len(),
            judged,
            matches: matches.len(),
            elapsed: start.elapsed(),
        },
        complete: !cut,
        matches,
    })
}

/// The find place in the target's dimension, and the centre targets are ordered from.
fn find_area<'w>(
    sessions: &dyn Sessions<'w>,
    place: &Place,
    dimension: Dimension,
    origin: [i32; 2],
) -> Result<(Area, [i32; 2])> {
    Ok(match *place {
        Place::Within { radius, anchor } => {
            let centre = match anchor {
                Anchor::Origin => origin,
                Anchor::Point(p) => p,
                Anchor::Spawn => {
                    let spawn = sessions
                        .session(Dimension::Overworld)?
                        .spawn_origin()
                        .map_block();
                    let scale = |d| Ok::<_, Error>(sessions.session(d)?.world.coordinate_scale());
                    convert(spawn, scale(Dimension::Overworld)?, scale(dimension)?)
                }
                Anchor::Here => unreachable!("the parser keeps `here` out of the find line"),
            };
            (Area::Disc { centre, radius }, centre)
        }
        Place::Box { min, max } => {
            let mid = |i: usize| ((i64::from(min[i]) + i64::from(max[i])).div_euclid(2)) as i32;
            (Area::Box { min, max }, [mid(0), mid(1)])
        }
    })
}

/// Every target in `area`, nearest `centre` first, then by z, then x.
fn enumerate(
    session: &Session<'_>,
    query: &SearchQuery,
    area: Area,
    centre: [i32; 2],
    confirm: bool,
) -> Result<Vec<Target>> {
    let found = query.found().expect("checked by the caller");
    let find = query.query.find().expect("resolved from a find line");
    let mut targets: Vec<Target> = match &found.target {
        Looked::Structure(structure) => {
            let (min, max) = area.bounds();
            starts_in(session, structure, min, max, confirm)
                .into_iter()
                .filter_map(|s: StructureStart| {
                    let position = [s.position[0], s.position[2]];
                    area.distance(position)?;
                    Some(Target {
                        position,
                        distance_sqr: distance_sqr(centre, position),
                        certain: s.certain,
                        feature: Some(Feature::Structure(s)),
                    })
                })
                .collect()
        }
        Looked::Biome(biome) => {
            let y = find.sampling.y;
            area.grid(find.sampling.step)
                .into_iter()
                .filter_map(|(p, _)| {
                    let position = [p[0], y, p[1]];
                    let (possible, certain) = sample(session, position, biome);
                    possible.then(|| {
                        let lookup = lookup(session, position);
                        Target {
                            position: p,
                            distance_sqr: distance_sqr(centre, p),
                            certain,
                            feature: Some(Feature::Biome {
                                position,
                                biome: session.biome_names(&lookup),
                                certain,
                            }),
                        }
                    })
                })
                .collect()
        }
        Looked::Feature(feature) => {
            let (min, max) = area.bounds();
            let mut found = Vec::new();
            session.each_feature(
                feature,
                min.map(|v| v >> 4),
                max.map(|v| v >> 4),
                |chunk| area.touches_chunk(chunk),
                FeatureOptions::default(),
                &mut Placings::default(),
                |f| {
                    let position = [f.position[0], f.position[2]];
                    if area.distance(position).is_some() {
                        found.push(Target {
                            position,
                            distance_sqr: distance_sqr(centre, position),
                            certain: f.certain,
                            feature: Some(Feature::Placed(f)),
                        });
                    }
                    ControlFlow::Continue(())
                },
            )?;
            found
        }
        Looked::Loot(item) => {
            let (min, max) = area.bounds();
            let mut found = Vec::new();
            // A chest can be beside the chunk that placed it, so the neighbours are placed too.
            session.each_container(
                item,
                min.map(|v| (v >> 4) - 1),
                max.map(|v| (v >> 4) + 1),
                |chunk| area.near_chunk(chunk),
                &mut LootPlacings::default(),
                |c| {
                    let position = [c.position[0], c.position[2]];
                    if area.distance(position).is_some() {
                        found.push(Target {
                            position,
                            distance_sqr: distance_sqr(centre, position),
                            certain: c.contents_certain(),
                            feature: Some(Feature::Container(c)),
                        });
                    }
                    ControlFlow::Continue(())
                },
            )?;
            found
        }
        Looked::Point => area
            .grid(find.sampling.step)
            .into_iter()
            .map(|(p, _)| Target {
                position: p,
                distance_sqr: distance_sqr(centre, p),
                certain: true,
                feature: None,
            })
            .collect(),
    };
    if targets.len() > MAX_FIND_TARGETS {
        return Err(Error::TooLarge {
            what: "a find query's targets",
            asked: targets.len() as i64,
            max: MAX_FIND_TARGETS as i64,
        });
    }
    targets.sort_by_key(|t| (t.distance_sqr, t.position[1], t.position[0]));
    Ok(targets)
}

/// Judges one target: its match, or `None` if the query isn't met there.
fn judge<'w>(
    sessions: &dyn Sessions<'w>,
    query: &SearchQuery,
    dimension: Dimension,
    target: &Target,
    options: &FindOptions,
) -> Result<Option<FindMatch>> {
    let start = match &target.feature {
        Some(Feature::Structure(s)) => Some((dimension, s.set.clone(), s.chunk)),
        _ => None,
    };
    let feature = match &target.feature {
        Some(Feature::Placed(f)) => Some((dimension, f.feature.clone(), f.position)),
        _ => None,
    };
    let container = match &target.feature {
        Some(Feature::Container(c)) => Some((dimension, c.position)),
        _ => None,
    };
    let here = Here {
        dimension,
        position: target.position,
        start,
        feature,
        container,
    };
    let checked = check_at(
        sessions,
        query,
        options.certain,
        0,
        options.confirm,
        Some(&here),
    )?;
    if checked == Checked::Failed {
        return Ok(None);
    }
    let mut evaluation = evaluate_at(sessions, query, options.confirm, Some(&here))?;
    // An uncertain target makes everything about it uncertain.
    if !target.certain && evaluation.verdict == Outcome::Met {
        evaluation.verdict = Outcome::PossiblyMet;
    }
    if !evaluation.reported(options.certain) {
        return Ok(None);
    }
    Ok(Some(FindMatch {
        position: target.position,
        target: target.feature.clone(),
        distance: target.distance_sqr.isqrt() as u32,
        score: evaluation.score(options.certain),
        evaluation,
    }))
}
