//! Search from the viewer (`docs/specs/search.md`): the panel's settings, a search on its own
//! thread, and what opening a hit shows on the map. Seeds are the library's `Game::search`,
//! across dimensions, with the matched features from `GameSession::evaluate`; places in the
//! seed shown are `GameSession::find` (ADR 0047).

use std::ops::ControlFlow;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use spyglass::query::{Anchor, Constraint, ConstraintKind, Importance, Place, Query, Region};
use spyglass::{
    Dimension, Evaluation, Feature, FindMatch, FindOptions, FindOutcome, FindProgress,
    FoundContainer, Game, SearchOptions, SearchOutcome, SearchProgress, SearchQuery, SearchStop,
    SearchThrottle, SeedSource,
};

use crate::userdata::{self, Kind};

/// What the Search tab searches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Mode {
    /// Seeds that fit a query.
    #[default]
    Seeds,
    /// Places in the seed shown that fit a `find` query.
    ThisSeed,
}

/// Ready-made `find` queries: a name, and the text. Each is a way into the language, edited
/// like any other query.
pub const PRESETS: &[(&str, &str)] = &[
    (
        "Double witch huts",
        "# Two witch huts close enough to share an AFK spot.\n\
         find minecraft:swamp_hut within 4096\n\
         require minecraft:swamp_hut within 200 of here",
    ),
    (
        "Monument in deep ocean",
        "# An ocean monument with only deep ocean around it, for a guardian farm.\n\
         find minecraft:monument within 4096\n\
         require biome #minecraft:required_ocean_monument_surrounding covers 100% within 128 of here",
    ),
    (
        "Pillager outpost on flat ground",
        "# An outpost on flat ground, for a raid farm.\n\
         find minecraft:pillager_outpost within 4096\n\
         require flat 4 within 48 of here step 4",
    ),
    (
        "Mushroom fields",
        "find biome minecraft:mushroom_fields within 4096 step 32",
    ),
    (
        "Ice spikes",
        "find biome minecraft:ice_spikes within 4096 step 32",
    ),
    (
        "Pale garden",
        "find biome minecraft:pale_garden within 4096 step 32",
    ),
    (
        "Cherry grove by the sea",
        "# Plains for a base, with a cherry grove and the sea nearby.\n\
         find point within 4096 step 64\n\
         require biome minecraft:plains within 0 of here\n\
         require biome minecraft:cherry_grove within 200 of here\n\
         require biome #minecraft:is_ocean within 300 of here",
    ),
];

/// The search panel's settings, kept between runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SearchSettings {
    /// Searching seeds, or places in the seed shown.
    pub mode: Mode,
    /// The seed search's query.
    pub query: String,
    /// The `find` query, kept apart so switching modes doesn't lose either.
    pub find: String,
    /// How many places to find.
    pub find_top: usize,
    /// How many of the best seeds to keep.
    pub top: usize,
    /// How many seeds to search; empty for no limit.
    pub seeds: String,
    /// Seconds to search for; 0 for no limit.
    pub seconds: u64,
    /// Search `first..=last` instead of every seed.
    pub range: bool,
    /// The range's first seed, as typed.
    pub first: String,
    /// The range's last seed, as typed.
    pub last: String,
    /// The index to start at; a finished search's `next` continues it.
    pub from: u64,
    /// Only hits whose required constraints hold whatever is undecided.
    pub certain: bool,
    /// Decide structure starts with each seed's terrain.
    pub confirm: bool,
}

impl Default for SearchSettings {
    fn default() -> Self {
        SearchSettings {
            mode: Mode::Seeds,
            query: "require 2 minecraft:swamp_hut within 600 of spawn\n\
                    prefer biome minecraft:cherry_grove within 500 of spawn"
                .to_string(),
            find: PRESETS[0].1.to_string(),
            find_top: 50,
            top: 20,
            seeds: String::new(),
            seconds: 60,
            range: false,
            first: "-2147483648".to_string(),
            last: "2147483647".to_string(),
            from: 0,
            certain: false,
            confirm: false,
        }
    }
}

impl SearchSettings {
    /// The library's options, on `threads` threads, or what's wrong with the settings.
    pub fn options(&self, threads: usize) -> Result<SearchOptions, String> {
        let number = |text: &str, what: &str| {
            text.trim()
                .parse::<i64>()
                .map_err(|_| format!("{what} must be a whole number"))
        };
        let source = if self.range {
            let (first, last) = (
                number(&self.first, "The first seed")?,
                number(&self.last, "The last seed")?,
            );
            if first > last {
                return Err("The first seed must be at most the last".to_string());
            }
            SeedSource::Range { first, last }
        } else {
            SeedSource::All
        };
        let seeds = match self.seeds.trim() {
            "" => None,
            text => Some(
                text.parse::<u64>()
                    .ok()
                    .filter(|&n| n > 0)
                    .ok_or("The number of seeds must be a positive whole number")?,
            ),
        };
        if seeds.is_none() && self.seconds == 0 && !self.range {
            return Err("Give a number of seeds or a time limit to search every seed".to_string());
        }
        Ok(SearchOptions {
            source,
            from: self.from,
            seeds,
            time: (self.seconds > 0).then(|| Duration::from_secs(self.seconds)),
            top: self.top.clamp(1, 1000),
            threads: Some(threads),
            certain: self.certain,
            confirm: self.confirm,
            stop: SearchStop::new(),
            throttle: SearchThrottle::new(),
        })
    }

    /// The library's options for a `find` from `origin`, on `threads` threads.
    pub fn find_options(&self, origin: [i32; 2], threads: usize) -> FindOptions {
        FindOptions {
            origin,
            top: self.find_top.clamp(1, 1000),
            threads: Some(threads),
            certain: self.certain,
            confirm: self.confirm,
            time: None,
            stop: SearchStop::new(),
            throttle: SearchThrottle::new(),
        }
    }
}

/// Threads for a search: every core but one, which the map's tiles and the window keep.
pub fn threads() -> usize {
    thread::available_parallelism().map_or(1, |n| n.get().saturating_sub(1).max(1))
}

/// A search running on its own thread, reporting progress `P` and ending with `O`.
pub struct Running<P, O> {
    /// Asks the search to stop.
    stop: SearchStop,
    /// How many threads the search may use now.
    throttle: SearchThrottle,
    /// The latest progress the search reported.
    progress: Arc<Mutex<Option<P>>>,
    /// The outcome, sent when the thread ends.
    done: Receiver<Result<O, String>>,
}

/// A seed search running.
pub type SeedRun = Running<SearchProgress, SearchOutcome>;
/// A `find` running in one seed.
pub type FindRun = Running<FindProgress, FindOutcome>;

impl<P: Clone + Send + 'static, O: Send + 'static> Running<P, O> {
    /// Runs `work` on its own thread, with a callback for its progress; `repaint` is called
    /// whenever there's news.
    fn spawn(
        stop: SearchStop,
        throttle: SearchThrottle,
        work: impl FnOnce(&dyn Fn(&P)) -> Result<O, String> + Send + 'static,
        repaint: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        let progress = Arc::new(Mutex::new(None));
        let (send, done) = channel();
        {
            let progress = progress.clone();
            thread::spawn(move || {
                let report = |p: &P| {
                    *progress.lock().unwrap_or_else(PoisonError::into_inner) = Some(p.clone());
                    repaint();
                };
                let outcome = work(&report);
                let _ = send.send(outcome);
                repaint();
            });
        }
        Running {
            stop,
            throttle,
            progress,
            done,
        }
    }
}

impl SeedRun {
    /// Starts searching seeds; `repaint` is called whenever there's news.
    pub fn start(
        game: &'static Game,
        query: SearchQuery,
        options: SearchOptions,
        repaint: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        let (stop, throttle) = (options.stop.clone(), options.throttle.clone());
        Running::spawn(
            stop,
            throttle,
            move |report| {
                game.search(&query, &options, |p| {
                    report(p);
                    ControlFlow::Continue(())
                })
                .map_err(|e| e.to_string())
            },
            repaint,
        )
    }
}

impl FindRun {
    /// Starts finding places in `seed`; `repaint` is called whenever there's news.
    pub fn find(
        game: &'static Game,
        seed: i64,
        query: SearchQuery,
        options: FindOptions,
        repaint: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        let (stop, throttle) = (options.stop.clone(), options.throttle.clone());
        Running::spawn(
            stop,
            throttle,
            move |report| {
                game.session(seed)
                    .find(&query, &options, |p| {
                        report(p);
                        ControlFlow::Continue(())
                    })
                    .map_err(|e| e.to_string())
            },
            repaint,
        )
    }
}

impl<P: Clone, O> Running<P, O> {
    /// Lends the search's cores to the map while it has tiles to compute, keeping one thread
    /// searching, and gives them back when it's done (ADR 0046).
    pub fn yield_to_map(&self, busy: bool) {
        self.throttle.set(busy.then_some(1));
    }

    /// Asks the search to stop before its next seed; the best seeds found are still
    /// evaluated. Asked again, it stops evaluating them too, keeping the best done so far.
    pub fn stop(&self) {
        if self.stop.is_stopped() {
            self.stop.stop_now();
        } else {
            self.stop.stop();
        }
    }

    /// Whether [`stop`](Self::stop) has been asked.
    pub fn stopping(&self) -> bool {
        self.stop.is_stopped()
    }

    /// The latest progress.
    pub fn progress(&self) -> Option<P> {
        self.progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The outcome, once the search has ended.
    pub fn poll(&self) -> Option<Result<O, String>> {
        match self.done.try_recv() {
            Ok(outcome) => Some(outcome),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err("the search failed".to_string())),
        }
    }
}

/// A search no one is waiting for any more stops at once rather than running on unseen.
impl<P, O> Drop for Running<P, O> {
    fn drop(&mut self) {
        self.stop.stop_now();
    }
}

/// Something a hit's constraint matched, to ring on the map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    /// The dimension `at` is in.
    pub dimension: Dimension,
    /// Its block, x and z.
    pub at: [i32; 2],
    /// What it is, for its tooltip.
    pub what: String,
    /// Whether it matched whatever is undecided.
    pub certain: bool,
    /// The constraint's canonical line.
    pub constraint: String,
    /// What marking it done marks; `None` for what can't be done (the spawn, slime chunks).
    pub place: Option<userdata::Place>,
}

/// The done key of a structure start, feature or biome a search found.
fn place_of(feature: &Feature) -> Option<userdata::Place> {
    match feature {
        Feature::Placed(f) => Some(crate::locate::feature_place(f)),
        Feature::Structure(s) => Some(userdata::Place {
            kind: Kind::Structure,
            id: s.set.clone(),
            at: [s.position[0], s.position[2]],
        }),
        Feature::Biome { biome, .. } => Some(userdata::Place {
            kind: Kind::Biome,
            id: biome.biome.clone(),
            at: feature.map_block(),
        }),
        _ => None,
    }
}

/// A container's tooltip: what it is and its table, e.g. `minecraft:chest,
/// minecraft:chests/simple_dungeon`.
fn container_name(container: &FoundContainer) -> String {
    format!("{}, {}", container.holder, container.table)
}

/// A query's constraint lines in canonical text: without a `find` line.
pub fn constraint_lines(query: &SearchQuery) -> Vec<String> {
    let skip = usize::from(query.query().find().is_some());
    query
        .to_string()
        .lines()
        .skip(skip)
        .map(str::to_string)
        .collect()
}

/// Everything a hit's constraints matched.
pub fn matches(query: &SearchQuery, evaluation: &Evaluation) -> Vec<Match> {
    let mut out = Vec::new();
    for (line, result) in constraint_lines(query).iter().zip(&evaluation.constraints) {
        for feature in &result.features {
            let (what, certain) = match feature {
                Feature::Structure(s) => (s.structures.join(" or "), s.certain),
                Feature::Placed(f) => (f.feature.clone(), f.certain),
                Feature::Container(c) => (container_name(c), c.contents_certain()),
                Feature::SlimeChunk(_) => ("Slime chunk".to_string(), true),
                Feature::Biome { biome, certain, .. } => (biome.biome.clone(), *certain),
                Feature::Spawn(s) => ("World spawn".to_string(), !s.platform_dependent),
                _ => continue,
            };
            let at = feature.map_block();
            out.push(Match {
                dimension: result.dimension,
                at,
                what,
                certain,
                constraint: line.to_string(),
                place: place_of(feature),
            });
        }
    }
    out
}

/// Everything a `find` match shows: the target itself, then what its constraints matched.
pub fn find_matches(query: &SearchQuery, dimension: Dimension, found: &FindMatch) -> Vec<Match> {
    let (what, certain) = match &found.target {
        Some(Feature::Structure(s)) => (s.structures.join(" or "), s.certain),
        Some(Feature::Placed(f)) => (f.feature.clone(), f.certain),
        Some(Feature::Container(c)) => (container_name(c), c.contents_certain()),
        Some(Feature::Biome { biome, certain, .. }) => (biome.biome.clone(), *certain),
        _ => ("Here".to_string(), true),
    };
    let first_line = query.to_string().lines().next().unwrap_or("").to_string();
    let place = found.target.as_ref().map_or(
        Some(userdata::Place {
            kind: Kind::Place,
            id: String::new(),
            at: found.position,
        }),
        place_of,
    );
    let mut out = vec![Match {
        dimension,
        at: found.position,
        what,
        certain,
        constraint: first_line,
        place,
    }];
    out.extend(matches(query, &found.evaluation));
    out
}

/// The structure ids a hit matched, to switch their layer on.
pub fn structures(evaluation: &Evaluation) -> Vec<String> {
    let mut ids: Vec<String> = evaluation
        .constraints
        .iter()
        .flat_map(|c| &c.features)
        .filter_map(|f| match f {
            Feature::Structure(s) => Some(s.structures.iter().cloned()),
            _ => None,
        })
        .flatten()
        .collect();
    ids.sort();
    ids.dedup();
    ids
}

/// The place a constraint is judged in: its disc or box.
fn place(kind: &ConstraintKind) -> Place {
    match kind {
        ConstraintKind::Structures { region, .. } | ConstraintKind::Slime { region, .. } => {
            match region {
                Region::Place(p) | Region::NearStructure { place: p, .. } => *p,
            }
        }
        ConstraintKind::Cluster { place, .. }
        | ConstraintKind::Features { place, .. }
        | ConstraintKind::FeatureCluster { place, .. }
        | ConstraintKind::Loot { place, .. }
        | ConstraintKind::Biome { place, .. }
        | ConstraintKind::Coverage { place, .. }
        | ConstraintKind::Spawn { place }
        | ConstraintKind::Height { place, .. }
        | ConstraintKind::Flat { place, .. } => *place,
    }
}

/// Where a place is on the map, with `spawn` for a spawn anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Area {
    /// Blocks within `radius` of `centre`.
    Disc { centre: [i32; 2], radius: u32 },
    /// Blocks from `min` to `max`, both included.
    Box { min: [i32; 2], max: [i32; 2] },
}

impl Area {
    /// `place` with `spawn` for a spawn anchor and `here` for a `find` target (or its origin).
    fn of(place: Place, spawn: [i32; 2], here: [i32; 2]) -> Area {
        match place {
            Place::Within { radius, anchor } => Area::Disc {
                centre: match anchor {
                    Anchor::Point(p) => p,
                    Anchor::Spawn => spawn,
                    Anchor::Here | Anchor::Origin => here,
                },
                radius,
            },
            Place::Box { min, max } => Area::Box { min, max },
        }
    }

    /// The area's middle block.
    fn centre(self) -> [i32; 2] {
        match self {
            Area::Disc { centre, .. } => centre,
            Area::Box { min, max } => {
                [0, 1].map(|i| ((i64::from(min[i]) + i64::from(max[i])) / 2) as i32)
            }
        }
    }
}

/// A constraint's area, with `spawn` for a spawn anchor and `here` for a `find` target.
pub fn area_of(constraint: &Constraint, spawn: [i32; 2], here: [i32; 2]) -> Area {
    Area::of(place(&constraint.kind), spawn, here)
}

/// Each constraint's area, in the query's order, with `spawn` for a spawn anchor.
pub fn areas(query: &Query, spawn: [i32; 2]) -> Vec<Area> {
    query
        .constraints()
        .iter()
        .map(|c| area_of(c, spawn, spawn))
        .collect()
}

/// Where to centre the map on a hit: the first constraint's place, at `spawn` for a spawn
/// anchor (the spawn in the first constraint's dimension).
pub fn focus(query: &Query, spawn: [i32; 2]) -> [i32; 2] {
    areas(query, spawn).first().map_or(spawn, |a| a.centre())
}

/// How many of a query's constraints are required, and how many preferred.
pub fn counts(query: &Query) -> (usize, usize) {
    let required = query
        .constraints()
        .iter()
        .filter(|c| c.importance == Importance::Required)
        .count();
    (required, query.constraints().len() - required)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_become_options() {
        let settings = SearchSettings {
            seeds: " 5000 ".to_string(),
            seconds: 0,
            top: 5000,
            ..SearchSettings::default()
        };
        let o = settings.options(3).unwrap();
        assert_eq!(
            (o.source, o.seeds, o.time, o.top, o.threads, o.confirm),
            (SeedSource::All, Some(5000), None, 1000, Some(3), false)
        );
        let confirmed = SearchSettings {
            confirm: true,
            ..settings
        };
        assert!(confirmed.options(1).unwrap().confirm);
        let range = SearchSettings {
            range: true,
            first: "-5".to_string(),
            last: "5".to_string(),
            seconds: 0,
            ..SearchSettings::default()
        };
        assert_eq!(
            range.options(1).unwrap().source,
            SeedSource::Range { first: -5, last: 5 }
        );
        let unbounded = SearchSettings {
            seconds: 0,
            ..SearchSettings::default()
        };
        assert!(unbounded.options(1).is_err());
        let backwards = SearchSettings {
            first: "5".to_string(),
            last: "-5".to_string(),
            ..range.clone()
        };
        assert!(backwards.options(1).is_err());
        let junk = SearchSettings {
            seeds: "lots".to_string(),
            ..SearchSettings::default()
        };
        assert!(junk.options(1).is_err());
        assert_eq!(
            SearchSettings::default().options(1).unwrap().time,
            Some(Duration::from_secs(60))
        );
    }

    #[test]
    fn matches_ring_every_feature() {
        let fixture = spyglass_oracle::fixtures::structured_overworld(false);
        let game = Game::open(fixture.path()).unwrap();
        let query = game
            .search_query("prefer 3 hut within 400 of -100,0\nprefer spawn within 9 of 0,0")
            .unwrap();
        let evaluation = game.session(4).evaluate(&query, false).unwrap();
        let found = matches(&query, &evaluation);
        let huts = evaluation.constraints[0].features.len();
        assert_eq!(found.len(), huts + 1);
        assert!(found[..huts].iter().all(|m| {
            m.what == "minecraft:hut"
                && !m.certain
                && m.constraint == "prefer 3 minecraft:hut within 400 of -100,0"
        }));
        assert_eq!(found[huts].what, "World spawn");
        assert!(found.iter().all(|m| m.dimension == Dimension::Overworld));
        let ids = structures(&evaluation);
        assert_eq!(
            ids,
            if huts > 0 {
                vec!["minecraft:hut".to_string()]
            } else {
                vec![]
            }
        );
    }

    #[test]
    fn hits_focus_on_the_first_place() {
        let focus_of = |text: &str| focus(&Query::parse(text).unwrap(), [7, 9]);
        assert_eq!(focus_of("require igloo within 5 of spawn"), [7, 9]);
        assert_eq!(focus_of("require igloo within 5 of -100,40"), [-100, 40]);
        assert_eq!(
            focus_of("require biome plains in -10,0 to 20,-31"),
            [5, -15]
        );
        assert_eq!(
            focus_of(
                "require igloo within 5 of mansion within 900 of 300,300\nprefer spawn within 5 of 0,0"
            ),
            [300, 300]
        );
        assert_eq!(focus_of("prefer spawn within 50 of 1,2"), [1, 2]);
    }

    #[test]
    fn areas_follow_the_constraints() {
        let query = Query::parse(
            "require igloo within 600 of spawn\nprefer biome plains in -10,0 to 20,-31",
        )
        .unwrap();
        assert_eq!(
            areas(&query, [7, 9]),
            [
                Area::Disc {
                    centre: [7, 9],
                    radius: 600
                },
                Area::Box {
                    min: [-10, -31],
                    max: [20, 0]
                }
            ]
        );
        assert_eq!(counts(&query), (1, 1));
    }
}
