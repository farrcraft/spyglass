//! Map tiles: rendered on worker threads, cached on the UI thread.
//!
//! Workers compute the slow part, a tile's biome map, plus its slime chunks. The UI thread turns
//! those into a texture with the current style (ties, highlight, slime) through the library's
//! `BiomeMap::colorize` and `Image::draw_features`, so restyling never samples biomes again.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread;

use eframe::egui;
use spyglass::{BiomeMap, MAX_SLIME_SCALE, MapFeatures, Session};

use crate::view::Tile;

/// What a tile shows: the world and seed, the height, and its place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TileKey {
    /// The app's number for the world and seed shown; each new session gets a new one.
    pub source: u64,
    pub y: i32,
    pub tile: Tile,
}

/// A computed tile.
pub struct TileData {
    pub biomes: BiomeMap,
    /// Slime chunks, at scales where they're drawn.
    pub slime_chunks: Vec<[i32; 2]>,
}

/// A tile to compute.
pub struct Job {
    pub key: TileKey,
    pub session: Arc<Session<'static>>,
}

type Done = (TileKey, Result<TileData, String>);

#[derive(Default)]
struct Queue {
    /// Waiting, most wanted first.
    waiting: Vec<Job>,
    running: HashSet<TileKey>,
}

/// Worker threads computing tiles, most wanted first. Jobs no longer wanted are dropped before
/// they start.
pub struct Pool {
    queue: Arc<(Mutex<Queue>, Condvar)>,
    done: Receiver<Done>,
}

impl Pool {
    /// A pool of one thread per core but one, calling `wake` when a tile is done.
    pub fn new(wake: impl Fn() + Send + Sync + 'static) -> Pool {
        let threads =
            thread::available_parallelism().map_or(2, |n| n.get().saturating_sub(1).max(1));
        Pool::with_threads(threads, wake)
    }

    fn with_threads(threads: usize, wake: impl Fn() + Send + Sync + 'static) -> Pool {
        let queue = Arc::new((Mutex::new(Queue::default()), Condvar::new()));
        let (send, done) = channel();
        let wake = Arc::new(wake);
        for i in 0..threads {
            let (queue, send, wake) = (queue.clone(), send.clone(), wake.clone());
            thread::Builder::new()
                .name(format!("tiles-{i}"))
                .spawn(move || work(&queue, &send, &*wake))
                .expect("a tile worker thread");
        }
        Pool { queue, done }
    }

    /// Replaces the waiting jobs with `jobs`, most wanted first. Jobs already running are
    /// skipped.
    pub fn want(&self, jobs: Vec<Job>) {
        let (lock, wake) = &*self.queue;
        let mut queue = lock.lock().unwrap_or_else(PoisonError::into_inner);
        let running = &queue.running;
        let jobs: Vec<Job> = jobs
            .into_iter()
            .filter(|j| !running.contains(&j.key))
            .collect();
        queue.waiting = jobs;
        wake.notify_all();
    }

    /// Whether a tile is being computed or waiting.
    pub fn busy(&self) -> bool {
        let queue = self.queue.0.lock().unwrap_or_else(PoisonError::into_inner);
        !queue.waiting.is_empty() || !queue.running.is_empty()
    }

    /// The tiles waiting, most wanted first.
    #[cfg(test)]
    fn waiting(&self) -> Vec<TileKey> {
        let queue = self.queue.0.lock().unwrap_or_else(PoisonError::into_inner);
        queue.waiting.iter().map(|j| j.key).collect()
    }

    /// Tiles finished since the last call.
    pub fn finished(&self) -> Vec<Done> {
        self.done.try_iter().collect()
    }
}

fn work(queue: &(Mutex<Queue>, Condvar), send: &Sender<Done>, wake: &(dyn Fn() + Send + Sync)) {
    let (lock, signal) = queue;
    loop {
        let job = {
            let mut q = lock.lock().unwrap_or_else(PoisonError::into_inner);
            while q.waiting.is_empty() {
                q = signal.wait(q).unwrap_or_else(PoisonError::into_inner);
            }
            let job = q.waiting.remove(0);
            q.running.insert(job.key);
            job
        };
        let result = compute(&job);
        lock.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .running
            .remove(&job.key);
        if send.send((job.key, result)).is_err() {
            return;
        }
        wake();
    }
}

fn compute(job: &Job) -> Result<TileData, String> {
    let area = job
        .key
        .tile
        .area(job.key.y)
        .ok_or("the tile is outside block coordinates")?;
    let biomes = job.session.biome_map(&area).map_err(|e| e.to_string())?;
    let slime_chunks = if area.scale <= MAX_SLIME_SCALE {
        let layers = spyglass::Layers {
            biomes: false,
            slime_chunks: true,
            ..spyglass::Layers::default()
        };
        job.session
            .map_features(&area, &layers)
            .map_err(|e| e.to_string())?
            .slime_chunks
    } else {
        Vec::new()
    };
    Ok(TileData {
        biomes,
        slime_chunks,
    })
}

/// How tiles are coloured.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Style {
    pub ties: bool,
    /// Biome ids to keep bright.
    pub highlight: Vec<String>,
    pub slime_chunks: bool,
}

/// A cached tile and its texture in some style.
pub struct Entry {
    pub data: Result<TileData, String>,
    pub texture: Option<(Style, egui::TextureHandle)>,
    last_used: u64,
}

/// Computed tiles, least recently used dropped first.
pub struct Cache {
    entries: HashMap<TileKey, Entry>,
    capacity: usize,
    frame: u64,
}

impl Cache {
    pub fn new(capacity: usize) -> Cache {
        Cache {
            entries: HashMap::new(),
            capacity,
            frame: 0,
        }
    }

    /// Starts a frame: tiles used from now on count as recent.
    pub fn next_frame(&mut self) {
        self.frame += 1;
    }

    pub fn contains(&self, key: &TileKey) -> bool {
        self.entries.contains_key(key)
    }

    pub fn insert(&mut self, key: TileKey, data: Result<TileData, String>) {
        self.entries.insert(
            key,
            Entry {
                data,
                texture: None,
                last_used: self.frame,
            },
        );
        if self.entries.len() > self.capacity {
            let mut ages: Vec<(u64, TileKey)> = self
                .entries
                .iter()
                .map(|(k, e)| (e.last_used, *k))
                .collect();
            ages.sort_by_key(|(age, _)| *age);
            let excess = self.entries.len() - self.capacity;
            for (_, key) in ages.into_iter().take(excess) {
                self.entries.remove(&key);
            }
        }
    }

    /// A tile, marked as used this frame.
    pub fn get(&mut self, key: &TileKey) -> Option<&mut Entry> {
        let frame = self.frame;
        let entry = self.entries.get_mut(key)?;
        entry.last_used = frame;
        Some(entry)
    }

    /// Drops every tile.
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

/// Slime chunks as the library's features, for `Image::draw_features`.
pub fn slime_features(data: &TileData) -> MapFeatures {
    MapFeatures {
        slime_chunks: data.slime_chunks.clone(),
        ..MapFeatures::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(x: i32) -> TileKey {
        TileKey {
            source: 1,
            y: 64,
            tile: Tile { scale: 4, x, z: 0 },
        }
    }

    fn session() -> Arc<Session<'static>> {
        let fixture = spyglass_oracle::fixtures::overworld(&[], true);
        let world: &'static spyglass::World =
            Box::leak(Box::new(spyglass::World::open(fixture.path()).unwrap()));
        Arc::new(world.session(9).unwrap())
    }

    #[test]
    fn jobs_no_longer_wanted_are_dropped() {
        let session = session();
        let pool = Pool::with_threads(0, || ());
        let job = |x| Job {
            key: key(x),
            session: session.clone(),
        };
        pool.want(vec![job(0), job(1)]);
        assert_eq!(pool.waiting(), [key(0), key(1)]);
        pool.want(vec![job(2)]);
        assert_eq!(pool.waiting(), [key(2)]);
        assert!(pool.busy());
    }

    #[test]
    fn workers_compute_what_the_library_does() {
        let session = session();
        let (wake, woken) = channel();
        let pool = Pool::with_threads(2, move || {
            let _ = wake.send(());
        });
        pool.want(
            (0..3)
                .map(|x| Job {
                    key: key(x),
                    session: session.clone(),
                })
                .collect(),
        );
        let mut done = Vec::new();
        while done.len() < 3 {
            woken.recv().unwrap();
            done.extend(pool.finished());
        }
        for (key, result) in done {
            let data = result.unwrap();
            let area = key.tile.area(key.y).unwrap();
            assert_eq!(data.biomes, session.biome_map(&area).unwrap());
            assert_eq!(
                data.slime_chunks,
                session.slime_chunks(area.min.map(|m| m >> 4), area.min.map(|m| (m + 1023) >> 4))
            );
        }
        assert!(!pool.busy());
    }

    #[test]
    fn the_least_recently_used_tile_goes_first() {
        let mut cache = Cache::new(2);
        cache.insert(key(0), Err(String::new()));
        cache.next_frame();
        cache.insert(key(1), Err(String::new()));
        cache.next_frame();
        assert!(cache.get(&key(0)).is_some());
        cache.next_frame();
        cache.insert(key(2), Err(String::new()));
        assert!(cache.contains(&key(0)) && cache.contains(&key(2)));
        assert!(!cache.contains(&key(1)));
    }
}
