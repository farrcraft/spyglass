//! spyglass: a local seed explorer for Minecraft: Java Edition.
//!
//! This is the public API that every front end uses (ADR 0014). Load one dimension of a version
//! bundle once as a [`World`], then bind it to seeds as [`Session`]s and ask them questions:
//!
//! ```no_run
//! let world = spyglass::World::open("bundles/26.3")?;
//! let session = world.session(spyglass::parse_seed("spyglass")?)?;
//! let village = world.structure_query("#minecraft:village")?;
//! let villages = session.locate_structure(&village, [0, 64, 0], &spyglass::LocateOptions::default())?;
//! if let Some(hit) = villages.hits.first() {
//!     println!("{:?} certain: {}", hit.start.position, hit.start.certain);
//! }
//! # Ok::<(), spyglass::Error>(())
//! ```
//!
//! Names (`#minecraft:village`, `minecraft:cherry_grove`) are resolved once against a world into
//! a [`StructureQuery`] or [`BiomeQuery`], which every session of that world can reuse. Worlds,
//! sessions and queries are `Send + Sync`, so one world can serve many threads.
//!
//! A [`World`] is one dimension: [`World::open`] the Overworld, [`World::open_dimension`] any of
//! the three. A [`Game`] is the whole bundle, opening each dimension's world the first time it's
//! asked for, for front ends that switch dimensions and questions that span them (ADR 0023).
//!
//! Answers say how sure they are. A biome cell can be tied, a structure start can depend on
//! terrain (unless it's confirmed, which computes it), and some results depend on the platform
//! the game runs on; each result type says which applies. See `docs/specs/locate.md`.

mod game;
mod render;
mod save;
mod search;

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

use spyglass_biome::{BiomeId, BiomeSource, Biomes, SearchVolume, SpawnSearch, SpawnTargets};
use spyglass_data::anchor::AnchorHeights;
use spyglass_data::bundle::{Bundle, BundleError};
use spyglass_data::codec::{LoadError, full_id};
use spyglass_data::versions::Checking;
use spyglass_structure::{Assembly, JigsawPools, Placements, Starts, StructureSets};
use spyglass_terrain::{Terrain, TerrainSettings};

pub use game::{Game, GameSession};
pub use render::{
    BiomeMap, DEFAULT_MAP_Y, HeightMap, Image, Layers, MARKER_REACH, MAX_PIXELS, MAX_SCALE,
    MAX_SIDE, MAX_SLIME_SCALE, MAX_TERRAIN_SCALE, MapArea, MapFeatures, biome_colour,
    structure_colour,
};
pub use save::{
    LauncherSave, PackConcern, SaveInfo, SaveInspector, SaveLocation, SaveReport, SaveWarning,
    Scan, find_save, inspect_save, launcher_saves,
};
pub use search::{
    ConstraintResult, Evaluation, Feature, Outcome, RankedSeed, SearchHit, SearchOptions,
    SearchOutcome, SearchProgress, SearchQuery, SearchStop, SeedSource,
};
pub use spyglass_data::bundle::{BUNDLES_ENV, Kind, default_bundles_dir};
pub use spyglass_data::dimension::Dimension;
pub use spyglass_data::versions::{CHECKED, Checked};
pub use spyglass_structure::LOCATE_RADIUS;
pub use spyglass_terrain::Heightmap;
/// Coarse timing of terrain, for finding where the time goes; not a stable interface.
#[cfg(feature = "timing")]
pub use spyglass_terrain::timing;

/// A search query's syntax tree, from [`SearchQuery::query`] (`docs/specs/search.md`).
pub mod query {
    pub use spyglass_search::{
        Anchor, Constraint, Count, DEFAULT_STEP, DEFAULT_Y, Importance, Kind as ConstraintKind,
        MAX_COORDINATE, MAX_PLACE_SIDE, MAX_SAMPLES, Place, Query, Region, Sampling,
    };
}

/// `/locate biome`'s search radius in blocks (`LocateCommand`).
pub const LOCATE_BIOME_RADIUS: i32 = 6400;
/// The most rings of placement cells [`Session::locate_structure`] searches: ten times
/// [`LOCATE_RADIUS`], a hundred times its cells (`docs/specs/locate.md#limits`).
pub const MAX_LOCATE_RADIUS: i32 = 1000;
/// The largest radius in blocks [`Session::locate_biome`] searches: four times
/// [`LOCATE_BIOME_RADIUS`], sixteen times its columns.
pub const MAX_LOCATE_BIOME_RADIUS: i32 = 25_600;
/// The largest radius in chunks [`Session::slime_chunks_near`] searches: about four million
/// chunks.
pub const MAX_SLIME_RADIUS: i32 = 1000;
/// The most columns [`Session::heights_in`] computes at once: about a second of work.
pub const MAX_HEIGHT_COLUMNS: i64 = 256 * 256;
/// `/locate biome`'s horizontal step in blocks.
const BIOME_HORIZONTAL_STEP: i32 = 32;
/// `/locate biome`'s vertical step in blocks.
const BIOME_VERTICAL_STEP: i32 = 64;

/// Why a request failed. More kinds may be added.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// A bundle couldn't be read, or its data couldn't be loaded. The message names the file
    /// and, for JSON, the path in it.
    #[error(transparent)]
    Data(Cause),
    /// A world save couldn't be read. The message names the file and the field.
    #[error(transparent)]
    Save(Cause),
    /// A file or folder couldn't be read.
    #[error("I/O error on {path}: {source}")]
    Io {
        /// The file or folder.
        path: PathBuf,
        /// The underlying error.
        source: std::io::Error,
    },
    /// A structure, structure tag, biome or world save that doesn't exist.
    #[error("unknown {what} {id:?}")]
    Unknown {
        /// What was looked up: `structure`, `structure tag`, `biome` or `world save`.
        what: &'static str,
        /// The id or name as given.
        id: String,
    },
    /// No bundle to use: none in the folder, or none for the version asked for.
    #[error(
        "no bundle{} in {}; extract one with spyglass-extract",
        .version.as_ref().map_or(String::new(), |v| format!(" for {v}")),
        .dir.display()
    )]
    NoBundle {
        /// The bundles folder.
        dir: PathBuf,
        /// The game version asked for, if any.
        version: Option<String>,
    },
    /// No version was asked for, and the bundles folder holds only snapshots, which are picked
    /// only by name (ADR 0028).
    #[error(
        "no release bundle in {}; name a snapshot to use one: {}",
        .dir.display(),
        .snapshots.join(", ")
    )]
    NoRelease {
        /// The bundles folder.
        dir: PathBuf,
        /// The snapshots there, newest first.
        snapshots: Vec<String>,
    },
    /// Several launcher saves have the folder name given.
    #[error(
        "{} saves are called {name:?}; give a path: {}",
        .matches.len(),
        .matches.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ")
    )]
    AmbiguousSave {
        /// The name given.
        name: String,
        /// Every save with that folder name.
        matches: Vec<PathBuf>,
    },
    /// Seed text the game would treat differently from spyglass, or none.
    #[error("{0}")]
    Seed(&'static str),
    /// spyglass can't answer for a world save; the reasons are listed.
    #[error("spyglass can't answer for this world: {0}")]
    Unsupported(String),
    /// A query was resolved against a different [`World`] than the session's.
    #[error("{0:?} was resolved against another world; resolve it with this session's world")]
    OtherWorld(String),
    /// A search query that doesn't parse, or names something this world doesn't have.
    #[error(
        "line {line}{}: {message}",
        .column.map_or(String::new(), |c| format!(", column {c}"))
    )]
    InvalidQuery {
        /// The line, from 1.
        line: usize,
        /// The column, from 1, where the text doesn't parse; `None` when it parses but
        /// doesn't resolve.
        column: Option<usize>,
        /// What's wrong.
        message: String,
    },
    /// A map area or layers that can't be drawn: the message says why.
    #[error("{0}")]
    InvalidMap(String),
    /// Two dimensions whose coordinates the game doesn't link, such as the End and the
    /// Overworld.
    #[error("{from} and {to} aren't linked by coordinates")]
    NotLinked {
        /// The dimension converted from.
        from: Dimension,
        /// The dimension converted to.
        to: Dimension,
    },
    /// A request past one of the API's limits, such as a locate radius (`docs/specs/locate.md`).
    #[error("{what} {asked} is more than {max}; ask for less")]
    TooLarge {
        /// What was asked for, e.g. `a structure locate radius of`.
        what: &'static str,
        /// How much.
        asked: i64,
        /// The most allowed.
        max: i64,
    },
    /// The search's worker threads couldn't be started, such as when asking for more than the
    /// system allows.
    #[error("couldn't start the search's threads: {0}")]
    Threads(Cause),
}

/// `value` as a [`Error::TooLarge`] if it's past `max`.
fn at_most(what: &'static str, value: i32, max: i32) -> Result<()> {
    if value > max {
        return Err(Error::TooLarge {
            what,
            asked: i64::from(value),
            max: i64::from(max),
        });
    }
    Ok(())
}

/// What went wrong below the API, for [`Error::Data`] and [`Error::Save`]. Its message is the
/// whole explanation; its type is deliberately opaque, so the layers below can change.
#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct Cause(Box<dyn std::error::Error + Send + Sync>);

impl From<BundleError> for Error {
    fn from(e: BundleError) -> Self {
        Error::Data(Cause(Box::new(e)))
    }
}

impl From<LoadError> for Error {
    fn from(e: LoadError) -> Self {
        Error::Data(Cause(Box::new(e)))
    }
}

impl From<spyglass_world::WorldError> for Error {
    fn from(e: spyglass_world::WorldError) -> Self {
        Error::Save(Cause(Box::new(e)))
    }
}

/// A result, or an [`Error`].
pub type Result<T> = std::result::Result<T, Error>;

/// The seed for `text`, as the world-creation screen reads it (`WorldOptions.parseSeed`):
/// trimmed of characters up to U+0020, then a decimal `long` (an optional sign, then digits),
/// else `String.hashCode` of the trimmed text, widened.
///
/// Empty text is an error (the game picks a random seed). So is text made only of non-ASCII
/// numerals: `Long.parseLong` reads some of them as digits, which spyglass doesn't implement, so
/// it refuses the whole class (any `char::is_numeric`) rather than guess.
pub fn parse_seed(text: &str) -> Result<i64> {
    let trimmed = text.trim_matches(|c: char| c <= ' ');
    if trimmed.is_empty() {
        return Err(Error::Seed("an empty seed means a random one"));
    }
    if let Ok(seed) = trimmed.parse::<i64>() {
        return Ok(seed);
    }
    let digits = trimmed.strip_prefix(['+', '-']).unwrap_or(trimmed);
    if !digits.is_empty() && digits.chars().all(char::is_numeric) && !digits.is_ascii() {
        return Err(Error::Seed(
            "seed text of non-ASCII numerals isn't supported",
        ));
    }
    // String.hashCode: s[0]·31^(n-1) + ... over UTF-16 code units, wrapping.
    let hash = trimmed
        .encode_utf16()
        .fold(0_i32, |h, c| h.wrapping_mul(31).wrapping_add(i32::from(c)));
    Ok(i64::from(hash))
}

/// A bundle found by [`bundles_in`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleInfo {
    /// The game version, e.g. `26.3`.
    pub version: String,
    /// The game's data version, which orders versions.
    pub data_version: u32,
    /// Release or snapshot (ADR 0028).
    pub kind: Kind,
    /// The bundle's directory.
    pub dir: PathBuf,
}

impl BundleInfo {
    /// Why this bundle's answers are unchecked, if they are (ADR 0039).
    pub fn unchecked_version(&self) -> Option<UncheckedVersion> {
        UncheckedVersion::of(&self.version, self.data_version)
    }
}

/// A bundle for a game version this build wasn't checked against: one not in [`CHECKED`]
/// (ADR 0039). It loads and answers as usual, but nothing compared its answers with the game's.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct UncheckedVersion {
    /// The game version, e.g. `26.5`.
    pub version: String,
    /// Its data version.
    pub data_version: u32,
    /// Whether it's newer than every checked version, so a later spyglass may support it.
    pub newer: bool,
}

impl UncheckedVersion {
    /// The warning for game version `version`, if its data version isn't checked.
    pub fn of(version: &str, data_version: u32) -> Option<Self> {
        let newer = match Checking::of(data_version) {
            Checking::Checked => return None,
            Checking::Newer => true,
            Checking::Unchecked => false,
        };
        Some(UncheckedVersion {
            version: version.to_string(),
            data_version,
            newer,
        })
    }

    /// A stable name for scripts: `unchecked_version`.
    pub fn kind(&self) -> &'static str {
        "unchecked_version"
    }
}

impl std::fmt::Display for UncheckedVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let newest = CHECKED.last().map_or("none", |c| c.id);
        if self.newer {
            write!(
                f,
                "{} is newer than every version this build of spyglass was checked against \
                 (the newest is {newest}), so its answers may differ from the game's; a newer \
                 spyglass may support it",
                self.version
            )
        } else {
            write!(
                f,
                "this build of spyglass wasn't checked against {}, so its answers may differ \
                 from the game's",
                self.version
            )
        }
    }
}

/// The bundles in `dir`'s subdirectories (e.g. `bundles/`), newest first by data version.
/// Subdirectories that aren't bundles are skipped. A missing `dir` has none.
pub fn bundles_in(dir: impl AsRef<Path>) -> Result<Vec<BundleInfo>> {
    let dir = dir.as_ref();
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => {
            return Err(Error::Io {
                path: dir.to_path_buf(),
                source,
            });
        }
    };
    let mut found: Vec<BundleInfo> = entries
        .flatten()
        .filter_map(|entry| {
            let manifest = spyglass_data::bundle::read_manifest(&entry.path()).ok()?;
            Some(BundleInfo {
                kind: manifest.kind(),
                version: manifest.version,
                data_version: manifest.data_version,
                dir: entry.path(),
            })
        })
        .collect();
    found.sort_by(|a, b| {
        b.data_version
            .cmp(&a.data_version)
            .then(a.version.cmp(&b.version))
    });
    Ok(found)
}

/// The bundle in `dir` (e.g. `bundles/`) for game version `version` (e.g. `26.3`), or the newest
/// release if `version` is `None`. Snapshots are as supported as releases, but are only picked by
/// name (ADR 0028).
pub fn find_bundle(dir: impl AsRef<Path>, version: Option<&str>) -> Result<BundleInfo> {
    let dir = dir.as_ref();
    let found = bundles_in(dir)?;
    if version.is_none() && !found.is_empty() && found.iter().all(|b| b.kind != Kind::Release) {
        return Err(Error::NoRelease {
            dir: dir.to_path_buf(),
            snapshots: found.into_iter().map(|b| b.version).collect(),
        });
    }
    found
        .into_iter()
        .find(|b| match version {
            Some(v) => b.version == v,
            None => b.kind == Kind::Release,
        })
        .ok_or_else(|| Error::NoBundle {
            dir: dir.to_path_buf(),
            version: version.map(str::to_string),
        })
}

/// Tells worlds apart, so a query resolved against one isn't used with another.
static WORLDS: AtomicU64 = AtomicU64::new(0);

/// The world preset every [`World`] is a dimension of: the Default preset.
pub(crate) const PRESET: &str = "minecraft:normal";

/// One dimension of a version bundle, loaded once: everything that doesn't depend on the seed.
pub struct World {
    id: u64,
    bundle: Bundle,
    dimension: Dimension,
    /// The dimension type's `coordinate_scale`.
    coordinate_scale: f64,
    /// The dimension type's `has_ceiling`.
    has_ceiling: bool,
    source: BiomeSource,
    sets: StructureSets,
    spawn: SpawnTargets,
    terrain: TerrainSettings,
    /// Loaded on first use: they read most of the bundle's structure templates.
    pools: OnceLock<JigsawPools>,
    /// Each biome's map colour, by [`BiomeId`].
    palette: Vec<[u8; 3]>,
}

impl World {
    /// The Overworld of the Default world preset, from the bundle in `dir` (e.g.
    /// `bundles/26.3`).
    pub fn open(dir: impl Into<PathBuf>) -> Result<World> {
        Self::open_dimension(dir, Dimension::Overworld)
    }

    /// `dimension` of the Default world preset, from the bundle in `dir`.
    pub fn open_dimension(dir: impl Into<PathBuf>, dimension: Dimension) -> Result<World> {
        let bundle = Bundle::open(dir)?;
        let id = dimension.id();
        // First, so that a preset without the dimension says so.
        let dimension_type = dimension_type(&bundle, id)?;
        let source = BiomeSource::load(&bundle, PRESET, id)?;
        let sets = StructureSets::load(&bundle, PRESET, id, &source)?;
        let terrain = TerrainSettings::load(&bundle, source.noise_settings(), &dimension_type)?;
        let spawn =
            SpawnTargets::load(&bundle, source.noise_settings())?.within_level(AnchorHeights {
                min_y: terrain.level_min_y,
                height: terrain.level_height,
            });
        // Binding fails the same way for every seed, so a world that opens binds for all.
        Terrain::new(&terrain, 0)?;
        let DimensionType {
            coordinate_scale,
            has_ceiling,
        } = DimensionType::load(&bundle, &dimension_type)?;
        let palette = source
            .biome_names()
            .names()
            .iter()
            .map(|n| biome_colour(n))
            .collect();
        Ok(World {
            id: WORLDS.fetch_add(1, Ordering::Relaxed),
            bundle,
            dimension,
            coordinate_scale,
            has_ceiling,
            source,
            sets,
            spawn,
            terrain,
            pools: OnceLock::new(),
            palette,
        })
    }

    /// The jigsaw structures' template pools, loaded the first time they're asked for.
    fn pools(&self) -> Result<&JigsawPools> {
        if let Some(pools) = self.pools.get() {
            return Ok(pools);
        }
        let pools = JigsawPools::load(&self.bundle, &self.sets)?;
        Ok(self.pools.get_or_init(|| pools))
    }

    /// The game version, e.g. `26.3`.
    pub fn version(&self) -> &str {
        &self.bundle.manifest().version
    }

    /// Why this world's answers are unchecked, if they are (ADR 0039).
    pub fn unchecked_version(&self) -> Option<UncheckedVersion> {
        let manifest = self.bundle.manifest();
        UncheckedVersion::of(&manifest.version, manifest.data_version)
    }

    /// The dimension's id, e.g. `minecraft:overworld`.
    pub fn dimension(&self) -> &str {
        self.dimension.id()
    }

    /// The dimension.
    pub fn dimension_kind(&self) -> Dimension {
        self.dimension
    }

    /// The dimension type's coordinate scale: 8 for the Nether, 1 for the others.
    pub fn coordinate_scale(&self) -> f64 {
        self.coordinate_scale
    }

    /// Whether the dimension type has a ceiling, as the Nether does. Its `WORLD_SURFACE_WG`
    /// heights are then the ceiling's, so maps don't draw terrain layers there.
    pub fn has_ceiling(&self) -> bool {
        self.has_ceiling
    }

    /// Every biome the dimension can have.
    pub fn biomes(&self) -> &[String] {
        self.source.biome_names().names()
    }

    /// Every structure the dimension's structure sets name, possible or not.
    pub fn structures(&self) -> impl Iterator<Item = &str> {
        self.sets.structures().iter().map(|s| s.id.as_str())
    }

    /// This world for `seed`.
    pub fn session(&self, seed: i64) -> Result<Session<'_>> {
        let biomes = self.source.with_seed(seed)?;
        let placements = self.sets.with_seed(seed, &biomes);
        Ok(Session {
            world: self,
            seed,
            spawn: self.spawn.with_seed(seed)?,
            biomes,
            placements,
            terrain: OnceLock::new(),
        })
    }

    fn exists(&self, registry: &str, id: &str) -> bool {
        Bundle::resource_path(registry, &full_id(id))
            .is_ok_and(|path| self.bundle.manifest().files.contains_key(&path))
    }

    /// Resolves a `/locate structure` argument, a structure id or a `#tag`, for this world's
    /// sessions. Reads the bundle's tags, so resolve once and reuse the query.
    pub fn structure_query(&self, argument: &str) -> Result<StructureQuery> {
        let (registry, what, id) = match argument.strip_prefix('#') {
            Some(tag) => ("tags/worldgen/structure", "structure tag", tag),
            None => ("worldgen/structure", "structure", argument),
        };
        if !self.exists(registry, id) {
            return Err(Error::Unknown {
                what,
                id: argument.to_string(),
            });
        }
        Ok(StructureQuery {
            world: self.id,
            argument: argument.to_string(),
            targets: self.sets.resolve(&self.bundle, argument)?,
        })
    }

    /// Resolves a biome id for this world's sessions. A biome that exists but that this
    /// dimension can't have resolves, and is never found.
    pub fn biome_query(&self, biome: &str) -> Result<BiomeQuery> {
        if !self.exists("worldgen/biome", biome) {
            return Err(Error::Unknown {
                what: "biome",
                id: biome.to_string(),
            });
        }
        Ok(BiomeQuery {
            world: self.id,
            id: biome.to_string(),
            biome: self.source.biome_names().id(&full_id(biome)),
        })
    }
}

/// The type of `dimension` in the Default preset, e.g. `minecraft:the_nether`.
fn dimension_type(bundle: &Bundle, dimension: &str) -> Result<String> {
    let preset = bundle.resource(
        "worldgen/world_preset",
        PRESET.trim_start_matches("minecraft:"),
    )?;
    preset
        .get("dimensions")
        .and_then(|d| d.get(dimension))
        .and_then(|d| d.get("type"))
        .and_then(serde_json::Value::as_str)
        .map(full_id)
        .ok_or_else(|| Error::Unknown {
            what: "dimension",
            id: dimension.to_string(),
        })
}

/// The fields of a dimension type that the API reads.
struct DimensionType {
    /// `coordinate_scale`, 1 if absent.
    coordinate_scale: f64,
    /// `has_ceiling`, false if absent.
    has_ceiling: bool,
}

impl DimensionType {
    /// Dimension type `id`, e.g. `minecraft:the_nether`.
    fn load(bundle: &Bundle, id: &str) -> Result<DimensionType> {
        let json = bundle.resource("dimension_type", id.trim_start_matches("minecraft:"))?;
        Ok(DimensionType {
            coordinate_scale: json
                .get("coordinate_scale")
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(1.0),
            has_ceiling: json
                .get("has_ceiling")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
        })
    }
}

/// A `/locate structure` argument resolved by [`World::structure_query`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructureQuery {
    world: u64,
    argument: String,
    targets: Vec<usize>,
}

impl StructureQuery {
    /// The argument as given, e.g. `#minecraft:village`.
    pub fn argument(&self) -> &str {
        &self.argument
    }
}

/// A biome resolved by [`World::biome_query`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BiomeQuery {
    world: u64,
    id: String,
    /// `None` if the dimension can't have the biome.
    biome: Option<BiomeId>,
}

impl BiomeQuery {
    /// The biome id as given, e.g. `minecraft:cherry_grove`.
    pub fn id(&self) -> &str {
        &self.id
    }
}

// Front ends share one world across threads: the viewer's tile renderers, and seed search.
const _: () = {
    const fn shareable<T: Send + Sync>() {}
    shareable::<World>();
    shareable::<Game>();
    shareable::<GameSession<'static>>();
    shareable::<Session<'static>>();
    shareable::<StructureQuery>();
    shareable::<BiomeQuery>();
    shareable::<SearchQuery>();
    shareable::<Error>();
};

/// A [`World`] bound to a seed. Cheap to create; per-seed data that takes a while (stronghold
/// rings) is computed on first use and kept.
pub struct Session<'w> {
    world: &'w World,
    seed: i64,
    biomes: Biomes<'w>,
    placements: Placements<'w>,
    spawn: SpawnSearch,
    /// Bound on first use: most queries never need terrain.
    terrain: OnceLock<Terrain>,
}

/// The biome at a position.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct BiomeAt {
    /// The biome, e.g. `minecraft:plains`. Where the cell is tied, the first of the tie.
    pub biome: String,
    /// Every biome the game may give here when the cell is tied (its answer depends on lookup
    /// history), [`biome`](Self::biome) first; empty when it's unique.
    pub tied: Vec<String>,
}

/// A structure start, or a chunk where one may be.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct StructureStart {
    /// The structure set, e.g. `minecraft:villages`.
    pub set: String,
    /// The chunk.
    pub chunk: [i32; 2],
    /// The position `/locate` reports: the chunk's minimum block plus the set's locate offset.
    pub position: [i32; 3],
    /// The structures that may start here, in the order the game tries them.
    pub structures: Vec<String>,
    /// Whether the game certainly starts one of them here. Otherwise it's a candidate: the
    /// start depends on terrain (unless confirmed), a tied biome cell, or a platform-dependent
    /// ring position.
    pub certain: bool,
}

impl StructureStart {
    /// The structure that certainly starts here, when that's known: a certain start with one
    /// structure. Its layout is [`Session::structure_layout`]'s.
    pub fn starting(&self) -> Option<&str> {
        match &self.structures[..] {
            [only] if self.certain => Some(only),
            _ => None,
        }
    }
}

/// How [`Session::locate_structure`] searches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocateOptions {
    /// How many rings of placement cells to search; the game's `/locate` searches
    /// [`LOCATE_RADIUS`].
    pub radius: i32,
    /// Decide each candidate with the seed's terrain, as the game does, so that only ties and
    /// platform-dependent results stay uncertain. Binds the seed's terrain.
    pub confirm: bool,
}

impl Default for LocateOptions {
    /// `/locate`'s radius, confirmed.
    fn default() -> Self {
        LocateOptions {
            radius: LOCATE_RADIUS,
            confirm: true,
        }
    }
}

/// A block-aligned box, corners included.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockBox {
    /// The minimum corner.
    pub min: [i32; 3],
    /// The maximum corner.
    pub max: [i32; 3],
}

impl From<spyglass_structure::BoundingBox> for BlockBox {
    fn from(b: spyglass_structure::BoundingBox) -> Self {
        BlockBox {
            min: b.min,
            max: b.max,
        }
    }
}

/// A jigsaw structure's pieces, from [`Session::structure_layout`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructureLayout {
    /// Each piece's box, the start piece first, before placement moves any.
    pub pieces: Vec<BlockBox>,
    /// The structure's bounds: the box around its pieces, grown by 12 on every side when the
    /// structure adapts the terrain around it.
    pub bounds: BlockBox,
}

/// A hit in [`Session::locate_structure`]'s answer.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct LocatedStructure {
    /// Where.
    pub start: StructureStart,
    /// `BlockPos.distSqr` from the search's origin.
    pub distance_sqr: f64,
    /// The distance `/locate` prints: horizontal, rounded down.
    pub blocks_away: i32,
}

/// The answer to [`Session::locate_structure`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct StructureLocate {
    /// Every place the game's answer may be, in its search order; each search path ends at
    /// its first certain hit.
    pub hits: Vec<LocatedStructure>,
    /// Whether the search ended where the game's does. If so, the game's answer is one of
    /// the hits, or nothing if every hit is a candidate the game rejects. If not, spyglass
    /// gave up after too many candidates.
    pub complete: bool,
    /// The game's answer if every candidate fails, as an index into
    /// [`hits`](Self::hits).
    pub certain_answer: Option<usize>,
}

/// A hit in [`Session::locate_biome`]'s answer.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct LocatedBiome {
    /// The block position the game would report.
    pub position: [i32; 3],
    /// The biome there.
    pub biome: BiomeAt,
    /// Whether the game certainly stops here; otherwise the cell is tied, and some of its
    /// biomes don't match.
    pub certain: bool,
    /// The distance `/locate` prints: in 3D, rounded down.
    pub blocks_away: i32,
}

/// The answer to [`Session::locate_biome`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct BiomeLocate {
    /// Every place the game's answer may be, in its search order, ending at the first
    /// certain one. Empty if nothing matches within the radius.
    pub hits: Vec<LocatedBiome>,
}

/// The world spawn's starting point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct SpawnOrigin {
    /// The chunk the game's spawn search starts from (`ChunkGenerator.getOrigin`). The final
    /// spawn is in the 11 × 11 chunks around it, decided by terrain spyglass doesn't compute
    /// yet.
    pub chunk: [i32; 2],
    /// The climate search's best block (y is 0), if the dimension has spawn targets.
    pub position: Option<[i32; 3]>,
    /// Whether the game's answer may differ by platform (ADR 0012).
    pub platform_dependent: bool,
}

/// `Mth.floor(Mth.sqrt(f))`: the square root in `float`, rounded down.
fn floor_sqrt(f: f32) -> i32 {
    (f64::from(f).sqrt() as f32).floor() as i32
}

impl<'w> Session<'w> {
    /// The world.
    pub fn world(&self) -> &'w World {
        self.world
    }

    /// The seed.
    pub fn seed(&self) -> i64 {
        self.seed
    }

    fn biome_names(&self, lookup: &spyglass_biome::Lookup) -> BiomeAt {
        let name = |b: &BiomeId| self.world.source.name(*b).to_string();
        BiomeAt {
            biome: name(&lookup.biome()),
            tied: if lookup.is_tied() {
                lookup.candidates().iter().map(name).collect()
            } else {
                Vec::new()
            },
        }
    }

    /// The seed's terrain, bound the first time it's asked for.
    fn terrain(&self) -> &Terrain {
        self.terrain.get_or_init(|| {
            Terrain::new(&self.world.terrain, self.seed)
                .expect("terrain binds for every seed once it binds for one, as World::open checks")
        })
    }

    /// The generator's height at block column `(x, z)` (`ChunkGenerator.getBaseHeight`): one
    /// above the highest block `heightmap` counts, before structures, carvers, features or
    /// surface blocks change the column. This is what structures are placed by. The first call
    /// binds the seed's terrain.
    pub fn height_at(&self, [x, z]: [i32; 2], heightmap: Heightmap) -> i32 {
        self.terrain().base_height(x, z, heightmap)
    }

    /// [`height_at`](Self::height_at) for every column from `min` to `max` inclusive, z outer
    /// and x inner. Empty if `max` is below `min` on either axis; more than
    /// [`MAX_HEIGHT_COLUMNS`] columns is an error.
    pub fn heights_in(
        &self,
        min: [i32; 2],
        max: [i32; 2],
        heightmap: Heightmap,
    ) -> Result<Vec<i32>> {
        let size = [0, 1].map(|d| i64::from(max[d]) - i64::from(min[d]) + 1);
        if size[0] <= 0 || size[1] <= 0 {
            return Ok(Vec::new());
        }
        let columns = size[0].saturating_mul(size[1]);
        if columns > MAX_HEIGHT_COLUMNS {
            return Err(Error::TooLarge {
                what: "a heights region of columns",
                asked: columns,
                max: MAX_HEIGHT_COLUMNS,
            });
        }
        // Both sides are at most MAX_HEIGHT_COLUMNS, so they fit an i32.
        let size = size.map(|s| s as i32);
        Ok(self.terrain().base_heights(min, size, heightmap))
    }

    /// The biome at block `(x, y, z)`: its quart's, as the game stores it. This isn't always the
    /// biome the F3 screen shows, which `BiomeManager` picks from the nearby quarts with a
    /// seed-dependent offset.
    pub fn biome_at(&self, [x, y, z]: [i32; 3]) -> BiomeAt {
        self.biome_names(&self.biomes.biome_at(x >> 2, y >> 2, z >> 2))
    }

    fn start(
        &self,
        set: usize,
        chunk: [i32; 2],
        position: [i32; 3],
        structures: &[usize],
        certain: bool,
    ) -> StructureStart {
        let sets = &self.world.sets;
        StructureStart {
            set: sets.sets()[set].id.clone(),
            chunk,
            position,
            structures: structures
                .iter()
                .map(|&s| sets.structures()[s].id.clone())
                .collect(),
            certain,
        }
    }

    /// Fails unless `world` is this session's world's id.
    fn check_world(&self, world: u64, name: &str) -> Result<()> {
        if world == self.world.id {
            Ok(())
        } else {
            Err(Error::OtherWorld(name.to_string()))
        }
    }

    /// The structure starts for this seed: from the climate alone, or confirmed with its
    /// terrain.
    fn starts(&self, confirm: bool) -> Starts<'_> {
        if confirm {
            Starts::with_terrain(&self.placements, &self.biomes, self.terrain())
        } else {
            Starts::new(&self.placements, &self.biomes)
        }
    }

    /// `/locate structure <argument>` run at `from`. Follows the game's search order and
    /// distance; see `docs/specs/locate.md`.
    pub fn locate_structure(
        &self,
        query: &StructureQuery,
        from: [i32; 3],
        options: &LocateOptions,
    ) -> Result<StructureLocate> {
        self.check_world(query.world, &query.argument)?;
        at_most(
            "a structure locate radius of",
            options.radius,
            MAX_LOCATE_RADIUS,
        )?;
        let starts = self.starts(options.confirm);
        let search = starts.locate(&query.targets, from, options.radius);
        let hits = search
            .hits
            .iter()
            .map(|h| {
                let [dx, dz] = [
                    h.position[0].wrapping_sub(from[0]),
                    h.position[2].wrapping_sub(from[2]),
                ];
                // LocateCommand.dist: int arithmetic, then a float square root.
                let horizontal = dx.wrapping_mul(dx).wrapping_add(dz.wrapping_mul(dz)) as f32;
                LocatedStructure {
                    start: self.start(h.set, h.chunk, h.position, &h.structures, h.certain),
                    distance_sqr: h.distance_sqr,
                    blocks_away: floor_sqrt(horizontal),
                }
            })
            .collect();
        Ok(StructureLocate {
            hits,
            complete: search.complete,
            certain_answer: search.certain_answer,
        })
    }

    /// Every chunk in `min..=max` (chunk coordinates, inclusive) where a structure that `query`
    /// names may start, by set, then in each set's chunk order. With `confirm`, each is decided
    /// with the seed's terrain, which it binds.
    pub fn structures_in(
        &self,
        query: &StructureQuery,
        min: [i32; 2],
        max: [i32; 2],
        confirm: bool,
    ) -> Result<Vec<StructureStart>> {
        self.check_world(query.world, &query.argument)?;
        Ok(self.starts_by_set(&query.targets, |_| (min, max), confirm))
    }

    /// Every chunk in each set's `chunks(locate offset)` range (inclusive) where one of
    /// `targets` may start, by set, then in each set's chunk order.
    fn starts_by_set(
        &self,
        targets: &[usize],
        chunks: impl Fn([i32; 3]) -> ([i32; 2], [i32; 2]),
        confirm: bool,
    ) -> Vec<StructureStart> {
        let starts = self.starts(confirm);
        let sets = &self.world.sets;
        let mut out = Vec::new();
        for (i, set) in sets.sets().iter().enumerate() {
            if !set.possible || !set.entries.iter().any(|e| targets.contains(&e.structure)) {
                continue;
            }
            let (min, max) = chunks(set.placement.locate_offset);
            for chunk in self.placements.structure_chunks_in(i, min, max) {
                if let Some((structures, certain)) = starts.targets_at(i, chunk, targets) {
                    let offset = set.placement.locate_offset;
                    let position = [
                        chunk[0].wrapping_shl(4).wrapping_add(offset[0]),
                        offset[1],
                        chunk[1].wrapping_shl(4).wrapping_add(offset[2]),
                    ];
                    out.push(self.start(i, chunk, position, &structures, certain));
                }
            }
        }
        out
    }

    /// The pieces of jigsaw structure `structure` (an id, e.g. `minecraft:village_plains`) if
    /// it starts in `chunk`, as the game assembles them, with its bounds. Whether it starts
    /// there is [`structures_in`](Self::structures_in)'s question; this one assumes it does.
    /// `None` if the structure isn't a jigsaw, or has no start piece there. Binds the seed's
    /// terrain, and the first call in a world loads its template pools, which takes a while
    /// (about a second for 26.3).
    pub fn structure_layout(
        &self,
        structure: &str,
        chunk: [i32; 2],
    ) -> Result<Option<StructureLayout>> {
        let sets = &self.world.sets;
        let index = sets
            .structure_index(structure)
            .ok_or_else(|| Error::Unknown {
                what: "structure",
                id: structure.to_string(),
            })?;
        if sets.structures()[index].jigsaw.is_none() {
            return Ok(None);
        }
        let pools = self.world.pools()?;
        let starts = self.starts(true).with_pools(pools);
        Ok(match starts.assemble(index, chunk) {
            Some(Assembly::Built(layout)) => Some(StructureLayout {
                pieces: layout.pieces.into_iter().map(BlockBox::from).collect(),
                bounds: layout.bounds.into(),
            }),
            Some(Assembly::NoStart) | None => None,
            Some(Assembly::NeedsTerrain) => {
                unreachable!("confirmed starts have the seed's terrain")
            }
        })
    }

    /// `/locate biome <biome>` run at `from`, searching `radius` blocks
    /// ([`LOCATE_BIOME_RADIUS`] in the game) in the game's order: a spiral of columns 32 blocks
    /// apart, and heights 64 apart from `from`'s outwards. A biome that exists but that this
    /// dimension can't have finds nothing.
    pub fn locate_biome(
        &self,
        query: &BiomeQuery,
        from: [i32; 3],
        radius: i32,
    ) -> Result<BiomeLocate> {
        self.check_world(query.world, &query.id)?;
        at_most("a biome locate radius of", radius, MAX_LOCATE_BIOME_RADIUS)?;
        let target = query.biome;
        let heights = self.world.sets.heights();
        let min_y = heights.level_min_y;
        let max_y = min_y.wrapping_add(heights.level_height).wrapping_sub(1);
        let volume = SearchVolume {
            from,
            radius,
            horizontal_step: BIOME_HORIZONTAL_STEP,
            vertical_step: BIOME_VERTICAL_STEP,
            min_y,
            max_y,
        };
        let search = self
            .biomes
            .find_closest_biome_3d(volume, |b| Some(b) == target);
        let hits = search
            .hits
            .iter()
            .map(|h| {
                let d = |i: usize| f64::from(h.position[i]) - f64::from(from[i]);
                let distance_sqr = d(0) * d(0) + d(1) * d(1) + d(2) * d(2);
                LocatedBiome {
                    position: h.position,
                    biome: self.biome_names(&h.biome),
                    certain: h.certain,
                    blocks_away: floor_sqrt(distance_sqr as f32),
                }
            })
            .collect();
        Ok(BiomeLocate { hits })
    }

    /// Whether chunk `(x, z)` is a slime chunk.
    pub fn is_slime_chunk(&self, [x, z]: [i32; 2]) -> bool {
        spyglass_structure::is_slime_chunk(self.seed, x, z)
    }

    /// Every slime chunk in `min..=max` (chunk coordinates, inclusive), x fastest.
    pub fn slime_chunks(&self, min: [i32; 2], max: [i32; 2]) -> Vec<[i32; 2]> {
        (min[1]..=max[1])
            .flat_map(|z| (min[0]..=max[0]).map(move |x| [x, z]))
            .filter(|&c| self.is_slime_chunk(c))
            .collect()
    }

    /// Every slime chunk within `radius` chunks of chunk `centre` on both axes (a square),
    /// nearest first by Euclidean distance in chunks, then by z, then x. A radius past
    /// [`MAX_SLIME_RADIUS`] is an error.
    pub fn slime_chunks_near(&self, centre: [i32; 2], radius: i32) -> Result<Vec<[i32; 2]>> {
        at_most("a slime chunk radius of", radius, MAX_SLIME_RADIUS)?;
        let around = |c: i32| [c.saturating_sub(radius), c.saturating_add(radius)];
        let ([x0, x1], [z0, z1]) = (around(centre[0]), around(centre[1]));
        let mut chunks = self.slime_chunks([x0, z0], [x1, z1]);
        let distance = |[x, z]: [i32; 2]| {
            let (dx, dz) = (
                i64::from(x) - i64::from(centre[0]),
                i64::from(z) - i64::from(centre[1]),
            );
            dx * dx + dz * dz
        };
        chunks.sort_by_key(|&c| (distance(c), c[1], c[0]));
        Ok(chunks)
    }

    /// Where the world spawn search starts.
    pub fn spawn_origin(&self) -> SpawnOrigin {
        let origin = self.spawn.origin();
        SpawnOrigin {
            chunk: origin.chunk,
            position: origin.found.map(|p| [p.position[0], 0, p.position[1]]),
            platform_dependent: origin.platform_dependent,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unchecked_versions_warn() {
        assert_eq!(UncheckedVersion::of("26.3", 5023), None);
        let newer = UncheckedVersion::of("27.1", 9000).unwrap();
        assert!(
            newer.newer && newer.to_string().contains("a newer spyglass"),
            "{newer}"
        );
        let between = UncheckedVersion::of("26.4-snapshot-1", 5100).unwrap();
        assert!(
            !between.newer
                && between
                    .to_string()
                    .contains("wasn't checked against 26.4-snapshot-1")
        );
        assert_eq!(between.kind(), "unchecked_version");
    }

    #[test]
    fn seeds_parse_like_the_game() {
        assert_eq!(parse_seed("12345").unwrap(), 12345);
        assert_eq!(parse_seed("  -7 \t").unwrap(), -7);
        assert_eq!(parse_seed("+7").unwrap(), 7);
        assert_eq!(parse_seed("-9223372036854775808").unwrap(), i64::MIN);
        // Out of range, or not a number: String.hashCode.
        assert_eq!(parse_seed("9223372036854775808").unwrap(), {
            let h = "9223372036854775808"
                .bytes()
                .fold(0_i32, |h, b| h.wrapping_mul(31).wrapping_add(i32::from(b)));
            i64::from(h)
        });
        // "spyglass".hashCode() is -1556512960 (the golden set's text seed).
        assert_eq!(parse_seed("spyglass").unwrap(), -1_556_512_960);
        assert_eq!(parse_seed("-").unwrap(), 45);
        // Characters above U+0020 aren't trimmed; supplementary characters hash as two units.
        assert_ne!(parse_seed("\u{a0}1").unwrap(), 1);
        assert_eq!(
            parse_seed("😀").unwrap(),
            i64::from(0xD83D_i32 * 31 + 0xDE00)
        );
        assert!(parse_seed("   ").is_err());
        assert!(parse_seed("１２３").is_err());
    }

    #[test]
    fn floor_sqrt_rounds_down_in_float() {
        assert_eq!(floor_sqrt(0.0), 0);
        assert_eq!(floor_sqrt(965_120.0), 982); // 272² + 944²
        assert_eq!(floor_sqrt(16.0), 4);
        assert_eq!(floor_sqrt(15.999), 3);
    }
}
