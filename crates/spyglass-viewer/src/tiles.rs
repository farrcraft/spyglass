//! Map tiles: computed on worker threads, cached on the UI thread.
//!
//! Workers do the slow parts: a tile's biome map, heights, underground layers, flowers and slime
//! chunks (keeping each tile's carving map across heights), separately the structure starts on
//! a tile for the structures chosen, the features and loot containers chosen up close, and the
//! layouts of jigsaw structures shown up close. The UI thread turns biome maps into textures with the current
//! style (ties, highlight, slime, relief, the flower shown) through the library's
//! `BiomeMap::colorize`, `Image::shade`, `Image::draw_flowers` and `Image::draw_features`, so restyling never samples biomes again, and draws
//! structure markers and bounds itself.
//!
//! Each frame, the map lists every job it would like, most wanted first, and the list replaces
//! the queue: a tile scrolled away before a worker took it is never computed, and one a worker is
//! computing is stopped (`Session::stopping`, ADR 0078) and never cached. Finished jobs come
//! back over a channel, and the map keeps them in a [`Cache`] that drops the tiles least
//! recently drawn.

use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread;
use std::time::Instant;

use eframe::egui;
use spyglass::{
    BiomeMap, CarvingMap, DEFAULT_MAP_Y, FeatureQuery, FlowerCount, FlowerMap, FoundContainer,
    FoundFeature, GeneratedChunk, HeightMap, ItemQuery, Layers, MapFeatures, Session, Stop,
    Stopping, StructureLayout, StructureQuery, StructureStart, UndergroundMap,
};

use crate::timing;
use crate::view::{Region, TILE, Tile};
use crate::zoom;

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
    /// The flower bone meal grows at each pixel's block at `y`.
    pub flowers: bool,
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

/// A region's features or loot containers: the world and seed, what's chosen, and the region,
/// whatever the scale they're drawn at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FeatureKey {
    /// The app's number for the world and seed shown.
    pub source: u64,
    /// The app's number for the features chosen.
    pub selection: u64,
    /// The chunks whose features or containers these are.
    pub region: Region,
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
    /// The flowers, where the key asked for them, or why they couldn't be worked out: the
    /// tile's biomes are drawn either way.
    pub flowers: Option<Result<TileFlowers, String>>,
    /// Slime chunks, at scales where they're drawn.
    pub slime_chunks: Vec<[i32; 2]>,
    /// The session's save's generated chunks, at the same scales; empty without a save.
    pub generated: Vec<GeneratedChunk>,
}

/// A tile's flowers and its key, worked out once on the worker.
pub struct TileFlowers {
    /// What bone meal grows at each pixel's block.
    pub map: FlowerMap,
    /// The flowers it shows, with how many pixels each can grow at and is certain at.
    pub key: Vec<FlowerCount>,
}

/// What a job computes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JobKey {
    /// A tile's biomes, heights, underground layers and slime chunks.
    Biomes(TileKey),
    /// A tile's structure starts.
    Markers(MarkerKey),
    /// A region's features.
    Features(FeatureKey),
    /// A region's loot containers.
    Loot(FeatureKey),
    /// A structure's layout, for its bounds.
    Layout(LayoutKey),
}

impl JobKey {
    /// The kind of job, for timing lines: `biomes 1:4`, `features 1:2`, `layout`.
    fn kind(&self) -> String {
        let (what, tile) = match self {
            JobKey::Biomes(k) => ("biomes", k.tile),
            JobKey::Markers(k) => ("markers", k.tile),
            JobKey::Features(_) => return "features".to_string(),
            JobKey::Loot(_) => return "loot".to_string(),
            JobKey::Layout(_) => return "layout".to_string(),
        };
        format!("{what} 1:{}", tile.scale)
    }
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
    Biomes(Box<TileData>),
    /// The structure starts whose marker centre is on the tile, in drawing order.
    Markers(Vec<StructureStart>),
    /// The features the region's chunks place, in drawing order.
    Features(Vec<FoundFeature>),
    /// The loot containers the region's chunks place, in drawing order.
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
    running: HashMap<JobKey, Running>,
}

/// A job a worker is computing.
struct Running {
    /// Raised when no list wants the job any more.
    stop: Stop,
    /// When it was raised, for timing how long the job takes to stop.
    stopped_at: Option<Instant>,
}

/// Why a job gave no result.
#[derive(Debug)]
enum JobError {
    /// Its stop was raised: nothing wanted it any more. Not cached.
    Stopped,
    /// The library failed on it, as the tile then says.
    Failed(String),
}

impl From<spyglass::Error> for JobError {
    fn from(e: spyglass::Error) -> Self {
        match e {
            spyglass::Error::Stopped => JobError::Stopped,
            e => JobError::Failed(e.to_string()),
        }
    }
}

impl From<&str> for JobError {
    fn from(message: &str) -> Self {
        JobError::Failed(message.to_string())
    }
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
    /// skipped, and those running that `jobs` doesn't list are stopped.
    pub fn want(&self, jobs: Vec<Job>) {
        let (lock, wake) = &*self.queue;
        let mut queue = lock.lock().unwrap_or_else(PoisonError::into_inner);
        let wanted: HashSet<JobKey> = jobs.iter().map(|j| j.key).collect();
        #[expect(
            clippy::iter_over_hash_type,
            reason = "each job is stopped on its own, in any order"
        )]
        for (key, running) in &mut queue.running {
            if !wanted.contains(key) && running.stopped_at.is_none() {
                running.stop.stop();
                running.stopped_at = Some(Instant::now());
            }
        }
        let running = &queue.running;
        let jobs: Vec<Job> = jobs
            .into_iter()
            .filter(|j| !running.contains_key(&j.key))
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

/// A worker's loop: take the most wanted job, compute it outside the lock, and send it back,
/// unless it was stopped. It sleeps while nothing waits, and ends when the pool's receiver is
/// dropped.
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
            let stop = Stop::new();
            queue.running.insert(
                job.key,
                Running {
                    stop: stop.clone(),
                    stopped_at: None,
                },
            );
            (job, stop)
        };
        let (job, stop) = job;
        // A panic in the library is a bug, but it fails one tile: the worker lives on, and the
        // job leaves `running`, so the tile isn't waited on forever.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            compute(&job, carvings, &stop)
        }))
        .unwrap_or_else(|_| Err("spyglass failed on this tile".into()));
        let running = lock
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .running
            .remove(&job.key);
        let result = match result {
            Ok(output) => Ok(output),
            Err(JobError::Failed(message)) => Err(message),
            Err(JobError::Stopped) => {
                if let Some(at) = running.and_then(|r| r.stopped_at) {
                    timing::stopped(job.key.kind(), at.elapsed());
                }
                // Nothing to keep, but the count of jobs left changed.
                wake();
                continue;
            }
        };
        if send.send((job.key, result)).is_err() {
            return;
        }
        wake();
    }
}

/// Computes one job with the library, timing each part, through `stop`: the job gives up
/// with [`JobError::Stopped`] soon after it's raised.
fn compute(job: &Job, carvings: &Carvings, stop: &Stop) -> Result<Output, JobError> {
    let session = job.session.stopping(stop);
    let (tile, y) = match job.key {
        JobKey::Biomes(key) => (key.tile, key.y),
        JobKey::Markers(key) => (key.tile, DEFAULT_MAP_Y),
        JobKey::Features(key) => {
            let found = timing::job(
                || "features".to_string(),
                || region_features(job, &session, key.region),
            )?;
            return Ok(Output::Features(found));
        }
        JobKey::Loot(key) => {
            let marks = timing::job(
                || "loot".to_string(),
                || loot_marks(job, &session, key.region),
            )?;
            return Ok(Output::Loot(marks));
        }
        JobKey::Layout(key) => {
            let structure = job
                .session
                .world()
                .structures()
                .nth(key.structure)
                .ok_or("no such structure")?;
            // A layout takes milliseconds, so it isn't stopped.
            return Ok(Output::Layout(timing::job(
                || "layout".to_string(),
                || job.session.structure_layout(structure, key.chunk),
            )?));
        }
    };
    let area = tile
        .area(y)
        .ok_or("the tile is outside block coordinates")?;
    // The name a part is timed under, such as "biomes 1:4".
    let timing_name = |what: &'static str| move || format!("{what} 1:{}", tile.scale);
    let features = |layers: Layers| session.map_features(&area, &layers);
    match job.key {
        JobKey::Biomes(key) => {
            let heights = if key.heights {
                Some(timing::job(timing_name("heights"), || {
                    session.height_map(&area)
                })?)
            } else {
                None
            };
            let biomes = timing::job(timing_name("biomes"), || match &heights {
                Some(heights) if key.surface => session.surface_biome_map(heights),
                _ => session.biome_map(&area),
            })?;
            let underground = if key.underground.any() {
                let carved = if key.underground.carvers {
                    Some(carving_map(job, &session, carvings, key.tile, &area)?)
                } else {
                    None
                };
                let map = timing::job(timing_name("underground"), || {
                    session.underground_map(&area, &key.underground.layers(), carved.as_deref())
                });
                Some(Box::new(map?))
            } else {
                None
            };
            // A tile whose flowers fail keeps its biomes and says why, unless it was stopped.
            let flowers = match key
                .flowers
                .then(|| timing::job(timing_name("flowers"), || session.flower_map(&area)))
            {
                None => None,
                Some(Err(spyglass::Error::Stopped)) => return Err(JobError::Stopped),
                Some(Ok(map)) => Some(Ok(TileFlowers {
                    key: map.key(),
                    map,
                })),
                Some(Err(e)) => Some(Err(e.to_string())),
            };
            let slime = Layers {
                biomes: false,
                slime_chunks: true,
                ..Layers::default()
            };
            // Only where the dimension has them (the Overworld).
            let slime_chunks = if zoom::SLIME_CHUNKS.shows(area.scale)
                && slime.check(job.session.world()).is_ok()
            {
                features(slime)?.slime_chunks
            } else {
                Vec::new()
            };
            let generated = if zoom::GENERATED.shows(area.scale) && job.session.save().is_some() {
                features(Layers {
                    biomes: false,
                    generated: true,
                    ..Layers::default()
                })?
                .generated
            } else {
                Vec::new()
            };
            Ok(Output::Biomes(Box::new(TileData {
                biomes,
                heights,
                underground,
                flowers,
                slime_chunks,
                generated,
            })))
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
        JobKey::Features(_) | JobKey::Loot(_) | JobKey::Layout(_) => {
            unreachable!("computed above")
        }
    }
}

/// The features the job names that `region`'s chunks place (`Session::features_in`), each once,
/// in drawing order: north to south, then west to east. Each belongs to the region of the
/// chunk that placed it, so regions never share one.
fn region_features(
    job: &Job,
    session: &Stopping<'_, '_>,
    region: Region,
) -> spyglass::Result<Vec<FoundFeature>> {
    let (min, max) = region.chunks();
    let mut found: Vec<FoundFeature> = Vec::new();
    for query in job.features.iter() {
        for feature in session.features_in(query, min, max, Default::default())? {
            if !found.contains(&feature) {
                found.push(feature);
            }
        }
    }
    found.sort_by(|a, b| {
        let key = |f: &FoundFeature| (f.position[2], f.position[0], f.position[1]);
        key(a).cmp(&key(b)).then_with(|| a.feature.cmp(&b.feature))
    });
    Ok(found)
}

/// The loot containers `region`'s chunks place: every one with the job's `all_containers`,
/// those holding its item otherwise (`Session::containers_holding`), each ringed if it holds
/// the item or might. Each belongs to the region of the chunk that placed it, even where it's
/// beside that chunk (a dungeon's or a ruined portal's chest), so regions never share one.
fn loot_marks(
    job: &Job,
    session: &Stopping<'_, '_>,
    region: Region,
) -> spyglass::Result<Vec<LootMark>> {
    let (min, max) = region.chunks();
    let found = match (&job.loot, job.all_containers) {
        (Some(query), false) => session.containers_holding(query, min, max)?,
        (_, true) => session.containers_in(min, max)?,
        (None, false) => Vec::new(),
    };
    let world = job.session.world();
    let mut marks = Vec::new();
    for container in found {
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
    session: &Stopping<'_, '_>,
    carvings: &Carvings,
    tile: Tile,
    area: &spyglass::MapArea,
) -> Result<Arc<CarvingMap>, JobError> {
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
        || session.carving_map(area),
    )?;
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
    /// Draw a save's generated chunks, where the tile has them.
    pub generated: bool,
    /// Shade them by inhabited time.
    pub inhabited: bool,
    /// With flowers on the tile, the one flower shown, by block id; `None` shows each block's
    /// most likely flower.
    pub flower: Option<String>,
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

/// The chunk overlays `style` draws on a tile: its slime chunks and its save's generated chunks.
pub fn chunk_features(data: &TileData, style: &Style) -> MapFeatures {
    MapFeatures {
        slime_chunks: if style.slime_chunks {
            data.slime_chunks.clone()
        } else {
            Vec::new()
        },
        generated: if style.generated {
            data.generated.clone()
        } else {
            Vec::new()
        },
        inhabited: style.inhabited,
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
            flowers: false,
            tile: Tile { scale: 4, x, z: 0 },
        }
    }

    /// The fixture world with one structure set of towers in plains.
    fn session() -> Arc<Session<'static>> {
        session_with(&[])
    }

    /// [`session`]'s world with `extra` files too.
    fn session_with(extra: &[(String, serde_json::Value)]) -> Arc<Session<'static>> {
        let fixture = fixtures::overworld(
            &[
                extra,
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
            ]
            .concat(),
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
    fn flower_tiles_are_the_library_s_at_every_height() {
        // Plains grows a poppy or a dandelion, 2 to 1; the tied forest grows nothing.
        let session = session_with(&[
            resource(
                "worldgen/biome",
                "plains",
                json!({"temperature": 0.8, "features": [["minecraft:flower_test"]]}),
            ),
            resource(
                "worldgen/placed_feature",
                "flower_test",
                json!({"feature": "minecraft:flower_test", "placement": []}),
            ),
            resource(
                "worldgen/configured_feature",
                "flower_test",
                json!({"type": "minecraft:simple_block", "config": {"to_place": {
                    "type": "minecraft:weighted_state_provider",
                    "entries": [{"data": {"Name": "minecraft:poppy"}, "weight": 2},
                        {"data": {"Name": "minecraft:dandelion"}, "weight": 1}]}}}),
            ),
            resource(
                "tags/worldgen/configured_feature",
                "can_spawn_from_bone_meal",
                json!({"values": ["minecraft:flower_test"]}),
            ),
            (
                "reports/blocks.json".to_string(),
                json!({
                    "minecraft:air": {"states": [{"id": 0, "default": true}]},
                    "minecraft:poppy": {"states": [{"id": 1, "default": true}]},
                    "minecraft:dandelion": {"states": [{"id": 2, "default": true}]},
                }),
            ),
        ]);
        let (wake, woken) = channel();
        let pool = Pool::with_threads(2, move || {
            let _ = wake.send(());
        });
        let flowers = |y, scale| TileKey {
            y,
            flowers: true,
            tile: Tile { scale, x: -1, z: 0 },
            ..key(0)
        };
        pool.want(vec![
            job(&session, JobKey::Biomes(flowers(64, 1))),
            job(&session, JobKey::Biomes(flowers(100, 16))),
        ]);
        for (key, result) in run(&pool, &woken, 2) {
            let (JobKey::Biomes(key), Ok(Output::Biomes(data))) = (key, result) else {
                panic!("a biome tile");
            };
            let area = key.tile.area(key.y).unwrap();
            let expected = session.flower_map(&area).unwrap();
            let Some(Ok(found)) = &data.flowers else {
                panic!("flowers on the tile");
            };
            assert_eq!(found.map, expected, "y {}", key.y);
            assert_eq!(found.key, expected.key());
            assert!(found.key.iter().any(|f| f.flower == "minecraft:poppy"));
        }
    }

    #[test]
    fn a_tile_whose_flowers_fail_keeps_its_biomes() {
        // The fixture has no bone meal tag.
        let session = session();
        let (wake, woken) = channel();
        let pool = Pool::with_threads(1, move || {
            let _ = wake.send(());
        });
        let flowers = TileKey {
            flowers: true,
            ..key(0)
        };
        pool.want(vec![job(&session, JobKey::Biomes(flowers))]);
        let (_, result) = run(&pool, &woken, 1).remove(0);
        let Ok(Output::Biomes(data)) = result else {
            panic!("a biome tile");
        };
        assert!(matches!(data.flowers, Some(Err(_))));
        let area = flowers.tile.area(flowers.y).unwrap();
        assert_eq!(data.biomes, session.biome_map(&area).unwrap());
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
    fn running_jobs_no_longer_wanted_are_stopped() {
        let session = session();
        let pool = Pool::with_threads(0, || ());
        // As a worker leaves them: taken, with a stop of their own.
        let running = |x| {
            (
                JobKey::Biomes(key(x)),
                Running {
                    stop: Stop::new(),
                    stopped_at: None,
                },
            )
        };
        let stops = {
            let mut queue = pool.queue.0.lock().unwrap();
            queue.running.extend([running(0), running(1)]);
            [0, 1].map(|x| queue.running[&JobKey::Biomes(key(x))].stop.clone())
        };
        // Still wanted, so not queued again and not stopped; no longer wanted, so stopped.
        pool.want(vec![job(&session, JobKey::Biomes(key(1)))]);
        assert!(stops[0].is_stopped());
        assert!(!stops[1].is_stopped());
        assert!(pool.waiting().is_empty());
        let queue = pool.queue.0.lock().unwrap();
        assert!(queue.running[&JobKey::Biomes(key(0))].stopped_at.is_some());
    }

    /// A worker on a features job, seconds of work, is free again soon after nothing wants it,
    /// and leaves nothing to cache.
    #[test]
    #[ignore = "needs bundles/<version> (SPYGLASS_ORACLE_VERSION, else the newest release)"]
    fn a_running_job_stops_within_half_a_second() {
        let world: &'static spyglass::World = Box::leak(Box::new(
            spyglass::World::open(spyglass_oracle::bundle_dir()).expect("a bundle"),
        ));
        let session = Arc::new(world.session(-4_893_098_694_207_956_070).unwrap());
        let features: Arc<[FeatureQuery]> = Arc::from(vec![
            world.feature_query("minecraft:monster_room").unwrap(),
            world.feature_query("minecraft:amethyst_geode").unwrap(),
        ]);
        // The template pools load once per world, and aren't stopped: load them first.
        session
            .features_in(&features[0], [40, 40], [43, 43], Default::default())
            .unwrap();
        let (wake, woken) = channel();
        let pool = Pool::with_threads(1, move || {
            let _ = wake.send(());
        });
        let tile = FeatureKey {
            source: 1,
            selection: 1,
            // A region's worth, still running when it's unwanted.
            region: Region {
                size: crate::view::FEATURE_REGION,
                x: 0,
                z: 0,
            },
        };
        pool.want(vec![Job {
            key: JobKey::Features(tile),
            session: session.clone(),
            structures: Arc::from(Vec::new()),
            features,
            loot: None,
            all_containers: false,
        }]);
        thread::sleep(std::time::Duration::from_millis(300));
        assert_eq!(pool.pending(), 1, "the job is still running");
        let unwanted = Instant::now();
        pool.want(Vec::new());
        while pool.pending() > 0 {
            woken
                .recv_timeout(std::time::Duration::from_secs(5))
                .expect("the worker to finish");
        }
        let took = unwanted.elapsed();
        assert!(took.as_millis() < 500, "stopped after {took:?}");
        assert!(pool.finished().is_empty(), "a stopped job sends nothing");
    }

    /// Every feature the library draws for an area is in the regions under it, and no two
    /// regions share one, so drawing by region shows what drawing by tile did, each once.
    #[test]
    #[ignore = "needs bundles/<version> (SPYGLASS_ORACLE_VERSION, else the newest release)"]
    fn regions_hold_every_feature_the_area_draws_once() {
        let world: &'static spyglass::World = Box::leak(Box::new(
            spyglass::World::open(spyglass_oracle::bundle_dir()).expect("a bundle"),
        ));
        let session = Arc::new(world.session(-4_893_098_694_207_956_070).unwrap());
        let features: Arc<[FeatureQuery]> = Arc::from(vec![
            world.feature_query("minecraft:monster_room").unwrap(),
            world.feature_query("minecraft:monster_room_deep").unwrap(),
            world.feature_query("minecraft:amethyst_geode").unwrap(),
        ]);
        let view = crate::view::View {
            centre: [300.0, -200.0],
            scale: 2,
            y: 64,
        };
        let tiles = view.tiles([256.0, 256.0]);
        let job = Job {
            key: JobKey::Features(FeatureKey {
                source: 1,
                selection: 1,
                region: Region {
                    size: 1,
                    x: 0,
                    z: 0,
                },
            }),
            session: session.clone(),
            structures: Arc::from(Vec::new()),
            features: features.clone(),
            loot: None,
            all_containers: false,
        };
        let never = Stop::new();
        let stopping = session.stopping(&never);
        let mut by_region: Vec<FoundFeature> = Vec::new();
        for region in view.regions(&tiles, crate::view::FEATURE_REGION) {
            by_region.extend(region_features(&job, &stopping, region).unwrap());
        }
        let layers = Layers {
            biomes: false,
            features: features.to_vec(),
            ..Layers::default()
        };
        let mut drawn = 0;
        for tile in &tiles {
            let area = tile.area(64).unwrap();
            for feature in session.map_features(&area, &layers).unwrap().features {
                assert!(by_region.contains(&feature), "{feature:?} in no region");
                drawn += 1;
            }
        }
        assert!(drawn > 0, "the area draws some features");
        for (i, feature) in by_region.iter().enumerate() {
            assert!(
                !by_region[i + 1..].contains(feature),
                "{feature:?} in two regions"
            );
        }
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
