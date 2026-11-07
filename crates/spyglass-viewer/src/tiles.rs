//! Map tiles: computed on worker threads, cached on the UI thread.
//!
//! Workers do the slow parts: a tile's biome map, heights, underground layers and slime
//! chunks (keeping each tile's carving map across heights), separately the structure starts on
//! a tile for the structures chosen, the features and loot containers chosen up close, and the
//! layouts of jigsaw structures shown up close. The UI thread turns biome maps into textures with the current
//! style (ties, highlight, slime, relief) through the library's `BiomeMap::colorize`,
//! `Image::shade` and `Image::draw_features`, so restyling never samples biomes again, and draws
//! structure markers and bounds itself.
//!
//! Each frame, the map lists every job it would like, most wanted first, and the list replaces
//! the queue: a tile scrolled away before a worker took it is never computed. Finished jobs come
//! back over a channel, and the map keeps them in a [`Cache`] that drops the tiles least
//! recently drawn.

use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread;

use eframe::egui;
use spyglass::{
    BiomeMap, CarvingMap, DEFAULT_MAP_Y, FeatureQuery, FoundContainer, FoundFeature, HeightMap,
    ItemQuery, Layers, MAX_SLIME_SCALE, MapFeatures, Session, StructureLayout, StructureQuery,
    StructureStart, UndergroundMap,
};

use crate::timing;
use crate::view::{TILE, Tile};

/// A tile's biomes: the world and seed, the height, what terrain it needs, and its place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TileKey {
    /// The app's number for the world and seed shown; each new session gets a new one.
    pub source: u64,
    /// The height biomes are sampled at, and underground layers drawn at.
    pub y: i32,
    /// Compute the tile's heights, for relief or surface biomes.
    pub heights: bool,
    /// Biomes at the surface rather than at `y`; needs `heights`.
    pub surface: bool,
    /// The underground layers to compute at `y`.
    pub underground: Underground,
    /// Where the tile is, and its scale.
    pub tile: Tile,
}

/// Which underground layers a tile has.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Underground {
    /// Ore veins.
    pub veins: bool,
    /// The density graph's caves, before carvers.
    pub noise_caves: bool,
    /// The carvers' caves and canyons.
    pub carvers: bool,
}

impl Underground {
    /// Whether the tile has any underground layer.
    pub fn any(self) -> bool {
        self.veins || self.noise_caves || self.carvers
    }

    /// The library's layers for these.
    pub fn layers(self) -> Layers {
        Layers {
            biomes: false,
            veins: self.veins,
            noise_caves: self.noise_caves,
            carvers: self.carvers,
            ..Layers::default()
        }
    }
}

/// Carving maps kept across heights, by world and seed and tile.
const CARVING_TILES: usize = 256;
/// The carving maps kept, shared by the workers.
type Carvings = Mutex<Cache<(u64, Tile), Arc<CarvingMap>>>;

/// A tile's structure markers: the world and seed, the structures chosen, whether terrain
/// decides them, and its place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MarkerKey {
    /// The app's number for the world and seed shown.
    pub source: u64,
    /// The app's number for the structures chosen.
    pub selection: u64,
    /// Decide starts with the seed's terrain (`Layers::confirm`).
    pub confirm: bool,
    /// Where the tile is, and its scale.
    pub tile: Tile,
}

/// A tile's feature markers: the world and seed, the features chosen, and its place, at a scale
/// they're drawn at (`spyglass::MAX_FEATURE_SCALE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FeatureKey {
    /// The app's number for the world and seed shown.
    pub source: u64,
    /// The app's number for the features chosen.
    pub selection: u64,
    /// Where the tile is, and its scale.
    pub tile: Tile,
}

/// A loot container on the map, and whether it's ringed: it holds the Loot item, or might.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LootMark {
    /// The container, with its contents where they're known.
    pub container: FoundContainer,
    /// Whether it holds the item asked for, or might.
    pub ringed: bool,
}

/// A structure's layout: the world and seed, the structure (an index into
/// `World::structures`) and its start chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LayoutKey {
    /// The app's number for the world and seed shown.
    pub source: u64,
    /// The structure, as an index into `World::structures`.
    pub structure: usize,
    /// The structure's start chunk, x and z.
    pub chunk: [i32; 2],
}

/// A computed tile.
pub struct TileData {
    /// The biomes, one cell a pixel.
    pub biomes: BiomeMap,
    /// Heights, where the key asked for them.
    pub heights: Option<HeightMap>,
    /// The underground layers, where the key asked for them.
    pub underground: Option<Box<UndergroundMap>>,
    /// Slime chunks, at scales where they're drawn.
    pub slime_chunks: Vec<[i32; 2]>,
}

/// What a job computes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JobKey {
    /// A tile's biomes, heights, underground layers and slime chunks.
    Biomes(TileKey),
    /// A tile's structure starts.
    Markers(MarkerKey),
    /// A tile's features.
    Features(FeatureKey),
    /// A tile's loot containers, up close.
    Loot(FeatureKey),
    /// A structure's layout, for its bounds.
    Layout(LayoutKey),
}

impl Job {
    /// The app's number for the job's world and seed.
    fn session_id(&self) -> u64 {
        match self.key {
            JobKey::Biomes(k) => k.source,
            JobKey::Markers(k) => k.source,
            JobKey::Features(k) | JobKey::Loot(k) => k.source,
            JobKey::Layout(k) => k.source,
        }
    }
}

/// Something to compute.
pub struct Job {
    /// What to compute, and the key its result is kept under.
    pub key: JobKey,
    /// The world and seed to compute it in.
    pub session: Arc<Session<'static>>,
    /// For markers: the structures chosen.
    pub structures: Arc<[StructureQuery]>,
    /// For feature markers: the features chosen.
    pub features: Arc<[FeatureQuery]>,
    /// For loot: the item whose containers are ringed.
    pub loot: Option<ItemQuery>,
    /// For loot: every container, not only those the item's in.
    pub all_containers: bool,
}

/// What a job computed, by the kind of job.
pub enum Output {
    /// A tile's biomes and the layers drawn with them.
    Biomes(TileData),
    /// The structure starts whose marker centre is on the tile, in drawing order.
    Markers(Vec<StructureStart>),
    /// The features whose marker centre is on the tile, in drawing order.
    Features(Vec<FoundFeature>),
    /// The loot containers on the tile, in drawing order.
    Loot(Vec<LootMark>),
    /// `None` if the structure is neither a jigsaw nor a mineshaft.
    Layout(Option<StructureLayout>),
}

/// A finished job: its key, and what it computed or why it failed.
type Done = (JobKey, Result<Output, String>);

/// The jobs the UI thread and the workers share, under one lock.
#[derive(Default)]
struct Queue {
    /// Waiting, most wanted first.
    waiting: Vec<Job>,
    /// Taken by a worker and not yet done, so a new list doesn't queue them again.
    running: HashSet<JobKey>,
}

/// Worker threads computing tiles, most wanted first. Jobs no longer wanted are dropped before
/// they start.
pub struct Pool {
    /// The queue, and the signal that wakes idle workers when it changes.
    queue: Arc<(Mutex<Queue>, Condvar)>,
    /// Finished jobs, sent by the workers.
    done: Receiver<Done>,
}

impl Pool {
    /// A pool of one thread per core but one, calling `wake` when a job is done.
    pub fn new(wake: impl Fn() + Send + Sync + 'static) -> Pool {
        let threads =
            thread::available_parallelism().map_or(2, |n| n.get().saturating_sub(1).max(1));
        Pool::with_threads(threads, wake)
    }

    /// A pool of `threads` workers; none, in tests, leaves every job waiting.
    fn with_threads(threads: usize, wake: impl Fn() + Send + Sync + 'static) -> Pool {
        let queue = Arc::new((Mutex::new(Queue::default()), Condvar::new()));
        let (send, done) = channel();
        let wake = Arc::new(wake);
        let carvings: Arc<Carvings> = Arc::new(Mutex::new(Cache::new(CARVING_TILES)));
        for i in 0..threads {
            let (queue, send, wake) = (queue.clone(), send.clone(), wake.clone());
            let carvings = carvings.clone();
            thread::Builder::new()
                .name(format!("tiles-{i}"))
                .spawn(move || work(&queue, &send, &*wake, &carvings))
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

    /// How many jobs are being computed or waiting.
    pub fn pending(&self) -> usize {
        let queue = self.queue.0.lock().unwrap_or_else(PoisonError::into_inner);
        queue.waiting.len() + queue.running.len()
    }

    /// The jobs waiting, most wanted first.
    #[cfg(test)]
    fn waiting(&self) -> Vec<JobKey> {
        let queue = self.queue.0.lock().unwrap_or_else(PoisonError::into_inner);
        queue.waiting.iter().map(|j| j.key).collect()
    }

    /// Jobs finished since the last call.
    pub fn finished(&self) -> Vec<Done> {
        self.done.try_iter().collect()
    }
}

/// A worker's loop: take the most wanted job, compute it outside the lock, and send it back.
/// It sleeps while nothing waits, and ends when the pool's receiver is dropped.
fn work(
    queue: &(Mutex<Queue>, Condvar),
    send: &Sender<Done>,
    wake: &(dyn Fn() + Send + Sync),
    carvings: &Carvings,
) {
    let (lock, signal) = queue;
    loop {
        let job = {
            let mut queue = lock.lock().unwrap_or_else(PoisonError::into_inner);
            while queue.waiting.is_empty() {
                queue = signal.wait(queue).unwrap_or_else(PoisonError::into_inner);
            }
            let job = queue.waiting.remove(0);
            queue.running.insert(job.key);
            job
        };
        // A panic in the library is a bug, but it fails one tile: the worker lives on, and the
        // job leaves `running`, so the tile isn't waited on forever.
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| compute(&job, carvings)))
                .unwrap_or_else(|_| Err("spyglass failed on this tile".to_string()));
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

/// Computes one job with the library, timing each part.
fn compute(job: &Job, carvings: &Carvings) -> Result<Output, String> {
    let (tile, y) = match job.key {
        JobKey::Biomes(key) => (key.tile, key.y),
        JobKey::Markers(key) => (key.tile, DEFAULT_MAP_Y),
        JobKey::Features(key) | JobKey::Loot(key) => (key.tile, DEFAULT_MAP_Y),
        JobKey::Layout(key) => {
            let structure = job
                .session
                .world()
                .structures()
                .nth(key.structure)
                .ok_or("no such structure")?;
            return timing::job(
                || "layout".to_string(),
                || job.session.structure_layout(structure, key.chunk),
            )
            .map(Output::Layout)
            .map_err(|e| e.to_string());
        }
    };
    let area = tile
        .area(y)
        .ok_or("the tile is outside block coordinates")?;
    // The name a part is timed under, such as "biomes 1:4".
    let timing_name = |what: &'static str| move || format!("{what} 1:{}", tile.scale);
    let features = |layers: Layers| {
        job.session
            .map_features(&area, &layers)
            .map_err(|e| e.to_string())
    };
    match job.key {
        JobKey::Biomes(key) => {
            let heights = if key.heights {
                let heights = timing::job(timing_name("heights"), || job.session.height_map(&area));
                Some(heights.map_err(|e| e.to_string())?)
            } else {
                None
            };
            let biomes = timing::job(timing_name("biomes"), || match &heights {
                Some(heights) if key.surface => job.session.surface_biome_map(heights),
                _ => job.session.biome_map(&area),
            })
            .map_err(|e| e.to_string())?;
            let underground = if key.underground.any() {
                let carved = if key.underground.carvers {
                    Some(carving_map(job, carvings, key.tile, &area)?)
                } else {
                    None
                };
                let map = timing::job(timing_name("underground"), || {
                    job.session
                        .underground_map(&area, &key.underground.layers(), carved.as_deref())
                });
                Some(Box::new(map.map_err(|e| e.to_string())?))
            } else {
                None
            };
            let slime = Layers {
                biomes: false,
                slime_chunks: true,
                ..Layers::default()
            };
            // Only where the dimension has them (the Overworld).
            let slime_chunks =
                if area.scale <= MAX_SLIME_SCALE && slime.check(job.session.world()).is_ok() {
                    features(slime)?.slime_chunks
                } else {
                    Vec::new()
                };
            Ok(Output::Biomes(TileData {
                biomes,
                heights,
                underground,
                slime_chunks,
            }))
        }
        JobKey::Markers(key) => {
            let layers = Layers {
                biomes: false,
                structures: job.structures.to_vec(),
                confirm: key.confirm,
                ..Layers::default()
            };
            let mut starts = timing::job(timing_name("markers"), || features(layers))?.structures;
            // Each marker belongs to the tile its centre is on, so it's drawn once.
            let inside = 0..i64::from(TILE);
            starts.retain(|s| {
                let [x, z] = area.pixel([s.position[0], s.position[2]]);
                inside.contains(&x) && inside.contains(&z)
            });
            Ok(Output::Markers(starts))
        }
        JobKey::Features(_) => {
            let layers = Layers {
                biomes: false,
                features: job.features.to_vec(),
                ..Layers::default()
            };
            let mut found = timing::job(timing_name("features"), || features(layers))?.features;
            // Each marker belongs to the tile its centre is on, so it's drawn once.
            let inside = 0..i64::from(TILE);
            found.retain(|f| {
                let [x, z] = area.pixel([f.position[0], f.position[2]]);
                inside.contains(&x) && inside.contains(&z)
            });
            Ok(Output::Features(found))
        }
        JobKey::Loot(_) => {
            let marks = timing::job(timing_name("loot"), || loot_marks(job, &area))
                .map_err(|e| e.to_string())?;
            Ok(Output::Loot(marks))
        }
        JobKey::Layout(_) => unreachable!("layouts are computed above"),
    }
}

/// The loot containers on `area`'s tile: every one with the job's `all_containers`, those
/// holding its item otherwise (`Session::containers_holding`), each ringed if it holds the item
/// or might.
fn loot_marks(job: &Job, area: &spyglass::MapArea) -> spyglass::Result<Vec<LootMark>> {
    let session = &job.session;
    // The tile's chunks and one more on every side: a dungeon's or a ruined portal's chest can
    // be beside the chunk that places it.
    let last = |d: usize| {
        let end = i64::from(area.min[d]) + i64::from(area.size[d]) * i64::from(area.scale);
        ((end - 1) >> 4) as i32 + 1
    };
    let min = area.min.map(|m| (m >> 4) - 1);
    let max = [last(0), last(1)];
    let found = match (&job.loot, job.all_containers) {
        (Some(query), false) => session.containers_holding(query, min, max)?,
        (_, true) => session.containers_in(min, max)?,
        (None, false) => Vec::new(),
    };
    let world = session.world();
    let mut marks = Vec::new();
    for container in found {
        // Each marker belongs to the tile its centre is on, so it's drawn once.
        let [x, z] = area.pixel([container.position[0], container.position[2]]);
        let inside = 0..i64::from(TILE);
        if !inside.contains(&x) || !inside.contains(&z) {
            continue;
        }
        let ringed = match &job.loot {
            None => false,
            Some(query) if container.contents.is_some() => container.holds(query),
            Some(query) => world.table_could_hold(query, &container.table)?,
        };
        marks.push(LootMark { container, ringed });
    }
    Ok(marks)
}

/// The tile's carving map, kept across heights: carving a tile is most of its underground
/// layers' cost.
fn carving_map(
    job: &Job,
    carvings: &Carvings,
    tile: Tile,
    area: &spyglass::MapArea,
) -> Result<Arc<CarvingMap>, String> {
    let key = (job.session_id(), tile);
    {
        let mut kept = carvings.lock().unwrap_or_else(PoisonError::into_inner);
        kept.next_frame();
        if let Some(map) = kept.get(&key) {
            return Ok(map.clone());
        }
    }
    let map = timing::job(
        || format!("carvers 1:{}", tile.scale),
        || job.session.carving_map(area),
    )
    .map_err(|e| e.to_string())?;
    let map = Arc::new(map);
    carvings
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(key, map.clone());
    Ok(map)
}

/// How tiles are coloured.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Style {
    /// Show tied cells as a checker of their candidates.
    pub ties: bool,
    /// Biome ids to keep bright.
    pub highlight: Vec<String>,
    /// Draw slime chunks, where the tile has them.
    pub slime_chunks: bool,
    /// Shade by the tile's heights, where it has them.
    pub relief: bool,
}

/// A computed tile and its texture in some style.
pub struct Entry {
    /// The tile, or why it failed.
    pub data: Result<TileData, String>,
    /// The texture last made, and the style it was made in: another style makes it again.
    pub texture: Option<(Style, egui::TextureHandle)>,
}

impl Entry {
    pub fn new(data: Result<TileData, String>) -> Self {
        Entry {
            data,
            texture: None,
        }
    }
}

/// Values by key, least recently used dropped first.
pub struct Cache<K, V> {
    /// Each value, with the frame it was last used in.
    entries: HashMap<K, (V, u64)>,
    /// How many values are kept.
    capacity: usize,
    /// The current frame's number.
    frame: u64,
}

impl<K: Copy + Eq + Hash, V> Cache<K, V> {
    pub fn new(capacity: usize) -> Self {
        Cache {
            entries: HashMap::new(),
            capacity,
            frame: 0,
        }
    }

    /// Starts a frame: values used from now on count as recent.
    pub fn next_frame(&mut self) {
        self.frame += 1;
    }

    pub fn contains(&self, key: &K) -> bool {
        self.entries.contains_key(key)
    }

    pub fn insert(&mut self, key: K, value: V) {
        self.entries.insert(key, (value, self.frame));
        if self.entries.len() > self.capacity {
            let mut ages: Vec<(u64, K)> = self
                .entries
                .iter()
                .map(|(key, (_, last_used))| (*last_used, *key))
                .collect();
            ages.sort_by_key(|(age, _)| *age);
            let excess = self.entries.len() - self.capacity;
            for (_, key) in ages.into_iter().take(excess) {
                self.entries.remove(&key);
            }
        }
    }

    /// A value, marked as used this frame.
    pub fn get(&mut self, key: &K) -> Option<&mut V> {
        let frame = self.frame;
        let (value, used) = self.entries.get_mut(key)?;
        *used = frame;
        Some(value)
    }

    /// Drops everything.
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
    use serde_json::json;
    use spyglass_oracle::fixtures::{self, resource};

    use super::*;

    fn key(x: i32) -> TileKey {
        TileKey {
            source: 1,
            y: 64,
            heights: false,
            surface: false,
            underground: Underground::default(),
            tile: Tile { scale: 4, x, z: 0 },
        }
    }

    /// The fixture world with one structure set of towers in plains.
    fn session() -> Arc<Session<'static>> {
        let fixture = fixtures::overworld(
            &[
                resource(
                    "worldgen/structure_set",
                    "towers",
                    json!({"placement": {"type": "minecraft:random_spread", "salt": 1,
                            "spacing": 4, "separation": 1},
                        "structures": [{"structure": "minecraft:tower", "weight": 1}]}),
                ),
                resource(
                    "worldgen/structure",
                    "tower",
                    json!({"type": "minecraft:stronghold", "biomes": "minecraft:plains", "step": "surface_structures"}),
                ),
            ],
            true,
        );
        // Kept for the test's life: the world reads its block settings when first asked.
        let fixture = Box::leak(Box::new(fixture));
        let world: &'static spyglass::World =
            Box::leak(Box::new(spyglass::World::open(fixture.path()).unwrap()));
        Arc::new(world.session(9).unwrap())
    }

    fn job(session: &Arc<Session<'static>>, key: JobKey) -> Job {
        let structures: Arc<[StructureQuery]> = Arc::from(vec![
            session.world().structure_query("minecraft:tower").unwrap(),
        ]);
        Job {
            key,
            session: session.clone(),
            structures,
            features: Arc::from(Vec::new()),
            loot: None,
            all_containers: false,
        }
    }

    fn run(pool: &Pool, woken: &Receiver<()>, count: usize) -> Vec<Done> {
        let mut done = Vec::new();
        while done.len() < count {
            woken.recv().unwrap();
            done.extend(pool.finished());
        }
        done
    }

    #[test]
    fn jobs_no_longer_wanted_are_dropped() {
        let session = session();
        let pool = Pool::with_threads(0, || ());
        let biomes = |x| job(&session, JobKey::Biomes(key(x)));
        pool.want(vec![biomes(0), biomes(1)]);
        assert_eq!(
            pool.waiting(),
            [JobKey::Biomes(key(0)), JobKey::Biomes(key(1))]
        );
        pool.want(vec![biomes(2)]);
        assert_eq!(pool.waiting(), [JobKey::Biomes(key(2))]);
        assert!(pool.pending() > 0);
    }

    #[test]
    fn workers_compute_what_the_library_does() {
        let session = session();
        let (wake, woken) = channel();
        let pool = Pool::with_threads(2, move || {
            let _ = wake.send(());
        });
        let terrain = TileKey {
            heights: true,
            surface: true,
            tile: Tile {
                scale: 1,
                x: 0,
                z: 0,
            },
            ..key(0)
        };
        let mut jobs: Vec<Job> = (0..3)
            .map(|x| job(&session, JobKey::Biomes(key(x))))
            .collect();
        jobs.push(job(&session, JobKey::Biomes(terrain)));
        pool.want(jobs);
        for (key, result) in run(&pool, &woken, 4) {
            let (JobKey::Biomes(key), Ok(Output::Biomes(data))) = (key, result) else {
                panic!("a biome tile");
            };
            let area = key.tile.area(key.y).unwrap();
            if key.heights {
                let heights = session.height_map(&area).unwrap();
                assert_eq!(data.biomes, session.surface_biome_map(&heights).unwrap());
                assert_eq!(data.heights, Some(heights));
            } else {
                assert_eq!(data.biomes, session.biome_map(&area).unwrap());
                assert_eq!(data.heights, None);
            }
            let side = (TILE * key.tile.scale) as i32;
            assert_eq!(
                data.slime_chunks,
                session.slime_chunks(
                    area.min.map(|m| m >> 4),
                    area.min.map(|m| (m + side - 1) >> 4)
                )
            );
        }
        assert_eq!(pool.pending(), 0);
    }

    #[test]
    fn underground_tiles_are_the_library_s_at_every_height() {
        let session = session();
        let (wake, woken) = channel();
        let pool = Pool::with_threads(1, move || {
            let _ = wake.send(());
        });
        let underground = Underground {
            veins: false,
            noise_caves: true,
            carvers: true,
        };
        let deep = |y| TileKey {
            y,
            underground,
            tile: Tile {
                scale: 1,
                x: -1,
                z: 0,
            },
            ..key(0)
        };
        // One worker, so the second height finds the first's carving map kept.
        pool.want(vec![
            job(&session, JobKey::Biomes(deep(0))),
            job(&session, JobKey::Biomes(deep(-30))),
        ]);
        for (key, result) in run(&pool, &woken, 2) {
            if let Err(e) = &result {
                panic!("{e}");
            }
            let (JobKey::Biomes(key), Ok(Output::Biomes(data))) = (key, result) else {
                panic!("a biome tile");
            };
            let area = key.tile.area(key.y).unwrap();
            let expected = session
                .underground_map(&area, &underground.layers(), None)
                .unwrap();
            assert_eq!(data.underground.as_deref(), Some(&expected), "y {}", key.y);
        }
    }

    #[test]
    fn each_marker_is_on_one_tile() {
        let session = session();
        let (wake, woken) = channel();
        let pool = Pool::with_threads(2, move || {
            let _ = wake.send(());
        });
        // A 3 × 2 block of tiles west of x = 0, where the plains are.
        let tiles: Vec<Tile> = (-3..0)
            .flat_map(|x| (0..2).map(move |z| Tile { scale: 2, x, z }))
            .collect();
        let marker = |tile| MarkerKey {
            source: 1,
            selection: 1,
            confirm: false,
            tile,
        };
        pool.want(
            tiles
                .iter()
                .map(|&t| job(&session, JobKey::Markers(marker(t))))
                .collect(),
        );
        let mut found: Vec<StructureStart> = run(&pool, &woken, tiles.len())
            .into_iter()
            .flat_map(|(_, result)| match result {
                Ok(Output::Markers(starts)) => starts,
                _ => panic!("markers"),
            })
            .collect();
        // Together, exactly the starts in the blocks the tiles cover.
        let side = (TILE * 2) as i32;
        let query = session.world().structure_query("minecraft:tower").unwrap();
        let mut expected = session
            .structures_in(&query, [-200, -10], [10, 200], false)
            .unwrap();
        expected.retain(|s| {
            (-3 * side..0).contains(&s.position[0]) && (0..2 * side).contains(&s.position[2])
        });
        let order = |s: &StructureStart| (s.chunk[1], s.chunk[0]);
        found.sort_by_key(order);
        expected.sort_by_key(order);
        assert!(!expected.is_empty());
        assert_eq!(found, expected);
    }

    #[test]
    fn the_least_recently_used_value_goes_first() {
        let mut cache = Cache::new(2);
        cache.insert(0, ());
        cache.next_frame();
        cache.insert(1, ());
        cache.next_frame();
        assert!(cache.get(&0).is_some());
        cache.next_frame();
        cache.insert(2, ());
        assert!(cache.contains(&0) && cache.contains(&2));
        assert!(!cache.contains(&1));
    }
}
