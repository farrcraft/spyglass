//! The `--json` output, as types (`docs/specs/cli-json.md`). Every answer is a [`Header`] and the
//! command's own fields. Keys are printed sorted, so field order here doesn't matter.

use std::path::Path;

use anyhow::Result;
use serde::Serialize;
use spyglass::{SaveLocation, SaveReport};

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
}

pub fn warnings(report: &SaveReport) -> Vec<Warning> {
    report
        .warnings
        .iter()
        .map(|w| Warning {
            kind: w.kind(),
            message: w.to_string(),
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
