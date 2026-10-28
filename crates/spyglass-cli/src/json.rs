//! The `--json` output, as types (`docs/specs/cli-json.md`). Every answer is a [`Header`] and the
//! command's own fields. Keys are printed sorted, so field order here doesn't matter.

use std::path::Path;

use anyhow::Result;
use serde::Serialize;
use spyglass::{Dimension, SaveLocation, SaveReport, SpawnOrigin, StructureLayout};

/// The `--json` format version.
pub const FORMAT: u32 = 1;

/// Prints `value` as pretty JSON with sorted keys.
pub fn print(value: &impl Serialize) -> Result<()> {
    // Through Value, whose maps are sorted, so the order never depends on field order.
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::to_value(value)?)?
    );
    Ok(())
}

/// A path as a JSON string. Text that isn't valid Unicode is replaced.
pub fn path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// A command's answer: the header, then its fields.
#[derive(Serialize)]
pub struct Answer<'a, T: Serialize> {
    #[serde(flatten)]
    pub header: Header<'a>,
    #[serde(flatten)]
    pub body: T,
}

/// The fields every locate answer starts with.
#[derive(Serialize)]
pub struct Header<'a> {
    pub format: u32,
    pub command: &'a str,
    pub version: &'a str,
    pub dimension: &'a str,
    pub seed: i64,
    /// With `--world`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub world: Option<WorldRef>,
    /// With `--world`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warnings: Option<Vec<Warning>>,
}

/// The save an answer is for.
#[derive(Serialize)]
pub struct WorldRef {
    pub name: String,
    pub dir: String,
}

#[derive(Serialize)]
pub struct Warning {
    pub kind: &'static str,
    pub message: String,
    /// For a warning about one dimension.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dimension: Option<&'static str>,
}

pub fn warnings(report: &SaveReport) -> Vec<Warning> {
    report
        .warnings
        .iter()
        .map(|w| Warning {
            kind: w.kind(),
            message: w.to_string(),
            dimension: w.dimension().map(Dimension::id),
        })
        .collect()
}

#[derive(Serialize)]
pub struct Xz {
    pub x: i32,
    pub z: i32,
}

#[derive(Serialize)]
pub struct Xyz {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

#[derive(Serialize)]
pub struct StructureAnswer<'a> {
    pub target: &'a str,
    pub from: [i32; 3],
    pub radius: i32,
    pub confirmed: bool,
    pub complete: bool,
    pub certain_answer: Option<usize>,
    pub hits: Vec<StructureHit<'a>>,
}

#[derive(Serialize)]
pub struct StructureHit<'a> {
    pub position: Xz,
    pub chunk: [i32; 2],
    pub set: &'a str,
    pub structures: &'a [String],
    pub certain: bool,
    pub blocks_away: i32,
    /// With `--bounds`, where the structure that starts is known and is a jigsaw.
    pub bounds: Option<Bounds>,
}

/// A jigsaw structure's bounds.
#[derive(Serialize)]
pub struct Bounds {
    pub min: [i32; 3],
    pub max: [i32; 3],
    pub pieces: usize,
}

impl Bounds {
    pub fn new(layout: &StructureLayout) -> Self {
        Bounds {
            min: layout.bounds.min,
            max: layout.bounds.max,
            pieces: layout.pieces.len(),
        }
    }
}

/// `heights`: rows of columns, z outer and x inner.
#[derive(Serialize)]
pub struct HeightsAnswer<'a> {
    pub heightmap: &'static str,
    pub min: [i32; 2],
    pub max: [i32; 2],
    pub heights: Vec<&'a [i32]>,
}

#[derive(Serialize)]
pub struct BiomeAnswer<'a> {
    pub target: &'a str,
    pub from: [i32; 3],
    pub radius: i32,
    pub hits: Vec<BiomeHit<'a>>,
}

#[derive(Serialize)]
pub struct BiomeHit<'a> {
    pub position: Xyz,
    pub biome: &'a str,
    pub tied: &'a [String],
    pub certain: bool,
    pub blocks_away: i32,
}

#[derive(Serialize)]
pub struct SlimeAnswer {
    pub from: [i32; 3],
    pub radius: i32,
    pub chunks: Vec<[i32; 2]>,
}

#[derive(Serialize)]
pub struct SpawnAnswer {
    pub chunk: [i32; 2],
    pub position: Option<Xz>,
    pub platform_dependent: bool,
}

impl SpawnAnswer {
    pub fn new(origin: &SpawnOrigin) -> Self {
        SpawnAnswer {
            chunk: origin.chunk,
            position: origin.position.map(|[x, _, z]| Xz { x, z }),
            platform_dependent: origin.platform_dependent,
        }
    }
}

/// `render`.
#[derive(Serialize)]
pub struct RenderAnswer<'a> {
    pub output: String,
    pub area: Area,
    pub layers: RenderLayers<'a>,
    pub structures: Vec<StructureMark<'a>>,
    pub slime_chunks: Vec<[i32; 2]>,
    pub spawn: Option<SpawnAnswer>,
}

#[derive(Serialize)]
pub struct Area {
    pub min: [i32; 2],
    pub size: [u32; 2],
    pub scale: u32,
    pub y: i32,
}

#[derive(Serialize)]
pub struct RenderLayers<'a> {
    pub biomes: bool,
    pub ties: bool,
    pub highlight: &'a [String],
    pub structures: &'a [String],
    pub slime_chunks: bool,
    pub spawn: bool,
    pub confirm: bool,
    pub relief: bool,
    pub surface: bool,
}

/// A structure marker on a map.
#[derive(Serialize)]
pub struct StructureMark<'a> {
    pub position: Xz,
    pub chunk: [i32; 2],
    pub set: &'a str,
    pub structures: &'a [String],
    pub certain: bool,
}

/// A save's summary: what it says, and what spyglass makes of it.
#[derive(Serialize)]
pub struct Save<'a> {
    pub name: &'a str,
    pub dir: String,
    pub data_version: Option<i32>,
    pub version_name: Option<&'a str>,
    pub seed: i64,
    pub generate_structures: bool,
    pub data_packs: &'a [String],
    pub spawn: Option<Location<'a>>,
    pub player: Option<Location<'a>>,
    pub bundle: Option<String>,
    pub answerable: bool,
    pub answerable_dimensions: Vec<&'static str>,
    pub warnings: Vec<Warning>,
}

#[derive(Serialize)]
pub struct Location<'a> {
    pub dimension: &'a str,
    pub position: [f64; 3],
}

impl<'a> Save<'a> {
    pub fn new(report: &'a SaveReport) -> Self {
        let save = &report.save;
        let location = |l: &'a SaveLocation| Location {
            dimension: &l.dimension,
            position: l.pos,
        };
        Save {
            name: &save.name,
            dir: path(&save.dir),
            data_version: save.data_version,
            version_name: save.version_name.as_deref(),
            seed: save.seed,
            generate_structures: save.generate_structures,
            data_packs: &save.enabled_packs,
            spawn: save.spawn.as_ref().map(location),
            player: save.player.as_ref().map(location),
            bundle: report.bundle.as_ref().map(|b| path(&b.dir)),
            answerable: report.answerable(),
            answerable_dimensions: Dimension::ALL
                .into_iter()
                .filter(|&d| report.bundle.is_some() && report.answerable_in(d))
                .map(Dimension::id)
                .collect(),
            warnings: warnings(report),
        }
    }
}

/// `world info`.
#[derive(Serialize)]
pub struct WorldInfo<'a> {
    pub format: u32,
    pub command: &'static str,
    #[serde(flatten)]
    pub save: Save<'a>,
}

/// `world list`.
#[derive(Serialize)]
pub struct WorldList<'a> {
    pub format: u32,
    pub command: &'static str,
    pub saves: Vec<ListedSave<'a>>,
}

/// One save in `world list`: its summary, or why it couldn't be read.
#[derive(Serialize)]
#[serde(untagged)]
pub enum ListedSave<'a> {
    Read {
        launcher: &'a str,
        #[serde(flatten)]
        save: Save<'a>,
    },
    Unreadable {
        launcher: &'a str,
        dir: String,
        error: String,
    },
}

/// `search`'s answer. Unlike the others, it has no `seed`: it's about many.
#[derive(Serialize)]
pub struct SearchAnswer<'a> {
    pub format: u32,
    pub command: &'a str,
    pub version: &'a str,
    pub dimension: &'a str,
    pub query: &'a str,
    pub source: Source,
    pub from: u64,
    pub certain: bool,
    pub confirm: bool,
    pub top: usize,
    pub next: Option<u64>,
    pub evaluated: u64,
    pub prefiltered: u64,
    pub checked: u64,
    pub reported: u64,
    pub elapsed_seconds: f64,
    pub hits: Vec<SearchHit<'a>>,
    pub partial: bool,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Source {
    All,
    Range { first: i64, last: i64 },
    List { count: usize },
}

#[derive(Serialize)]
pub struct SearchHit<'a> {
    pub index: u64,
    pub seed: i64,
    pub score: u64,
    pub verdict: &'static str,
    pub constraints: Vec<ConstraintAnswer<'a>>,
}

#[derive(Serialize)]
pub struct ConstraintAnswer<'a> {
    pub constraint: &'a str,
    pub dimension: &'static str,
    pub required: bool,
    pub outcome: &'static str,
    pub best: u32,
    pub worst: u32,
    pub features: Vec<FeatureAnswer<'a>>,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FeatureAnswer<'a> {
    Structure(StructureMark<'a>),
    SlimeChunk {
        chunk: [i32; 2],
    },
    Biome {
        position: Xyz,
        biome: &'a str,
        tied: &'a [String],
        certain: bool,
    },
    Spawn(SpawnAnswer),
    Other,
}
