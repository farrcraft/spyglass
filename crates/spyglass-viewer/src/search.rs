//! Seed search from the viewer (`docs/specs/search.md`): the panel's settings, a search on its
//! own thread, and what opening a hit shows on the map. The searching is the library's
//! `World::search`; the matched features come from `Session::evaluate`.

use std::ops::ControlFlow;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use spyglass::query::{Anchor, Kind, Place, Query, Region};
use spyglass::{
    Evaluation, Feature, SearchOptions, SearchOutcome, SearchProgress, SearchQuery, SeedSource,
    World,
};

/// The search panel's settings, kept between runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SearchSettings {
    pub query: String,
    pub top: usize,
    /// How many seeds to search; empty for no limit.
    pub seeds: String,
    /// Seconds to search for; 0 for no limit.
    pub seconds: u64,
    /// Search `first..=last` instead of every seed.
    pub range: bool,
    pub first: String,
    pub last: String,
    /// The index to start at; a finished search's `next` continues it.
    pub from: u64,
    pub certain: bool,
}

impl Default for SearchSettings {
    fn default() -> Self {
        SearchSettings {
            query: "require 2 minecraft:swamp_hut within 600 of spawn\n\
                    prefer biome minecraft:cherry_grove within 500 of spawn"
                .to_string(),
            top: 20,
            seeds: String::new(),
            seconds: 60,
            range: false,
            first: "-2147483648".to_string(),
            last: "2147483647".to_string(),
            from: 0,
            certain: false,
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
        })
    }
}

/// Threads for a search: every core but one, which the map's tiles and the window keep.
pub fn threads() -> usize {
    thread::available_parallelism().map_or(1, |n| n.get().saturating_sub(1).max(1))
}

/// A search running on its own thread.
pub struct Running {
    stop: Arc<AtomicBool>,
    progress: Arc<Mutex<Option<SearchProgress>>>,
    done: Receiver<Result<SearchOutcome, String>>,
}

impl Running {
    /// Starts searching; `repaint` is called whenever there's news.
    pub fn start(
        world: &'static World,
        query: SearchQuery,
        options: SearchOptions,
        repaint: impl Fn() + Send + 'static,
    ) -> Running {
        let stop = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(Mutex::new(None));
        let (send, done) = channel();
        {
            let (stop, progress) = (stop.clone(), progress.clone());
            thread::spawn(move || {
                let outcome = world.search(&query, &options, |p| {
                    *progress.lock().unwrap_or_else(PoisonError::into_inner) = Some(p.clone());
                    repaint();
                    if stop.load(Ordering::Relaxed) {
                        ControlFlow::Break(())
                    } else {
                        ControlFlow::Continue(())
                    }
                });
                let _ = send.send(outcome.map_err(|e| e.to_string()));
                repaint();
            });
        }
        Running {
            stop,
            progress,
            done,
        }
    }

    /// Asks the search to stop after its current round.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    /// The latest progress.
    pub fn progress(&self) -> Option<SearchProgress> {
        self.progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The outcome, once the search has ended.
    pub fn poll(&self) -> Option<Result<SearchOutcome, String>> {
        match self.done.try_recv() {
            Ok(outcome) => Some(outcome),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err("the search failed".to_string())),
        }
    }
}

/// Something a hit's constraint matched, to ring on the map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    pub at: [i32; 2],
    pub what: String,
    pub certain: bool,
    /// The constraint's canonical line.
    pub constraint: String,
}

/// Everything a hit's constraints matched.
pub fn matches(query: &SearchQuery, evaluation: &Evaluation) -> Vec<Match> {
    let text = query.to_string();
    let mut out = Vec::new();
    for (line, result) in text.lines().zip(&evaluation.constraints) {
        for feature in &result.features {
            let (at, what, certain) = match feature {
                Feature::Structure(s) => (
                    [s.position[0], s.position[2]],
                    s.structures.join(" or "),
                    s.certain,
                ),
                Feature::SlimeChunk(c) => (c.map(|v| v * 16 + 8), "Slime chunk".to_string(), true),
                Feature::Biome {
                    position,
                    biome,
                    certain,
                } => ([position[0], position[2]], biome.biome.clone(), *certain),
                Feature::Spawn(s) => (
                    s.map_block(),
                    "World spawn".to_string(),
                    !s.platform_dependent,
                ),
                _ => continue,
            };
            out.push(Match {
                at,
                what,
                certain,
                constraint: line.to_string(),
            });
        }
    }
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

/// Where to centre the map on a hit: the first constraint's place, at the spawn for a spawn
/// anchor.
pub fn focus(query: &Query, spawn: [i32; 2]) -> [i32; 2] {
    let centre = |place: &Place| match *place {
        Place::Within {
            anchor: Anchor::Point(p),
            ..
        } => p,
        Place::Within {
            anchor: Anchor::Spawn,
            ..
        } => spawn,
        Place::Box { min, max } => {
            [0, 1].map(|i| ((i64::from(min[i]) + i64::from(max[i])) / 2) as i32)
        }
    };
    let Some(first) = query.constraints().first() else {
        return spawn;
    };
    match &first.kind {
        Kind::Structures { region, .. } | Kind::Slime { region, .. } => match region {
            Region::Place(p) | Region::NearStructure { place: p, .. } => centre(p),
        },
        Kind::Cluster { place, .. }
        | Kind::Biome { place, .. }
        | Kind::Coverage { place, .. }
        | Kind::Spawn { place } => centre(place),
    }
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
            (o.source, o.seeds, o.time, o.top, o.threads),
            (SeedSource::All, Some(5000), None, 1000, Some(3))
        );
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
        let world = World::open(fixture.path()).unwrap();
        let query = world
            .search_query(
                "prefer 3 hut within 400 of -100,0
prefer spawn within 9 of 0,0",
            )
            .unwrap();
        let evaluation = world.session(4).unwrap().evaluate(&query).unwrap();
        let found = matches(&query, &evaluation);
        let huts = evaluation.constraints[0].features.len();
        assert_eq!(found.len(), huts + 1);
        assert!(found[..huts].iter().all(|m| {
            m.what == "minecraft:hut"
                && !m.certain
                && m.constraint == "prefer 3 minecraft:hut within 400 of -100,0"
        }));
        assert_eq!(found[huts].what, "World spawn");
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
}
