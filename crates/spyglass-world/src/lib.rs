//! Reading local Minecraft: Java Edition world saves: the seed, version, generators, data packs,
//! spawn and single player ([`Save::read`]), what its data packs contain ([`Save::packs`]),
//! how many chunks older versions generated ([`region`]), and where launchers keep saves
//! ([`launchers`]). It does no worldgen and needs
//! no game data. See `docs/specs/world-saves.md`.

pub mod launchers;
mod packs;
pub mod region;

pub use packs::PackContents;

use std::collections::BTreeMap;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use serde_json::Value;
use spyglass_data::nbt::{self, NbtError, Tag};

/// Why a save couldn't be read.
#[derive(Debug, thiserror::Error)]
pub enum WorldError {
    /// A file couldn't be read.
    #[error("I/O error on {path}: {source}")]
    Io {
        /// The file.
        path: PathBuf,
        /// The underlying error.
        source: std::io::Error,
    },
    /// A file isn't valid NBT.
    #[error("{path}: {source}")]
    Nbt {
        /// The file.
        path: PathBuf,
        /// What's wrong.
        source: NbtError,
    },
    /// A field is missing or has the wrong type.
    #[error("{path}: {field}: expected {expected}")]
    Field {
        /// The file.
        path: PathBuf,
        /// The field, e.g. `Data.DataVersion`.
        field: String,
        /// What was expected there.
        expected: &'static str,
    },
    /// A dimension id that isn't a valid identifier, so names no folder.
    #[error("not a dimension id: {0:?}")]
    DimensionId(String),
}

/// Which file layout a save uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// 26.x: generator settings in `data/minecraft/world_gen_settings.dat`, regions under
    /// `dimensions/<namespace>/<path>/`, players in `players/data/`.
    Current,
    /// Before 26.x: `Data.WorldGenSettings` in `level.dat`, regions in `region/`, `DIM-1/` and
    /// `DIM1/`, the single player in `Data.Player`.
    Legacy,
    /// Before 1.16: `Data.RandomSeed`, a generator named by `Data.generatorName` (in
    /// [`Save::generators`] as `{"legacy_generator": {"name", "options"}}`), and
    /// `Data.MapFeatures`. Regions as in [`Legacy`](Self::Legacy).
    Before116,
}

/// A position in a dimension.
#[derive(Debug, Clone, PartialEq)]
pub struct Location {
    /// The dimension, e.g. `minecraft:overworld`.
    pub dimension: String,
    /// The position, in blocks.
    pub pos: [f64; 3],
}

/// What a world save says about itself.
#[derive(Debug, Clone, PartialEq)]
pub struct Save {
    /// The save's directory.
    pub dir: PathBuf,
    /// Its layout.
    pub layout: Layout,
    /// `Data.LevelName`: the name shown in the world list.
    pub name: String,
    /// `Data.DataVersion`: the data version the world was last saved in. `None` before 1.9,
    /// which didn't record one.
    pub data_version: Option<i32>,
    /// `Data.Version.Name`, e.g. `26.3`, if present.
    pub version_name: Option<String>,
    /// The world seed.
    pub seed: i64,
    /// Whether structures generate (`generate_structures`, `generate_features` before 26.x).
    pub generate_structures: bool,
    /// Each dimension's generator (`dimensions.<id>.generator`), as JSON for comparing with a
    /// bundle's world presets. Bytes become numbers, as the game's codecs read them.
    pub generators: BTreeMap<String, Value>,
    /// `Data.DataPacks.Enabled`, in load order.
    pub enabled_packs: Vec<String>,
    /// `Data.DataPacks.Disabled`.
    pub disabled_packs: Vec<String>,
    /// `Data.WasModded`: whether a modded game has ever opened the world.
    pub was_modded: bool,
    /// The world spawn, if recorded.
    pub spawn: Option<Location>,
    /// The single player's position, in single-player saves.
    pub player: Option<Location>,
}

/// The most a save's NBT files (`level.dat`, player and settings files) may hold, compressed or
/// not, and the most memory the tree read from one may take. Real ones are kilobytes; the limit
/// keeps a crafted file from filling memory.
const MAX_NBT_FILE: u64 = 64 << 20;

/// A whole file, unless it's longer than `max` bytes.
pub(crate) fn read_capped(path: &Path, max: u64) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(max.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("longer than {max} bytes"),
        ));
    }
    Ok(bytes)
}

fn read_nbt(path: &Path) -> Result<Tag, WorldError> {
    let bytes = read_capped(path, MAX_NBT_FILE).map_err(|source| WorldError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    nbt::read_compressed_limited(&bytes, MAX_NBT_FILE)
        .map(|(_, tag)| tag)
        .map_err(|source| WorldError::Nbt {
            path: path.to_path_buf(),
            source,
        })
}

/// A field lookup in one file, with errors naming the file and the field.
struct Fields<'a> {
    path: &'a Path,
}

impl Fields<'_> {
    fn error(&self, field: &str, expected: &'static str) -> WorldError {
        WorldError::Field {
            path: self.path.to_path_buf(),
            field: field.to_string(),
            expected,
        }
    }

    /// The tag at a dotted path.
    fn get<'t>(&self, root: &'t Tag, field: &str) -> Option<&'t Tag> {
        field.split('.').try_fold(root, |t, name| t.get(name))
    }

    fn req<'t>(
        &self,
        root: &'t Tag,
        field: &str,
        expected: &'static str,
    ) -> Result<&'t Tag, WorldError> {
        self.get(root, field)
            .ok_or_else(|| self.error(field, expected))
    }

    fn int(&self, root: &Tag, field: &str) -> Result<i32, WorldError> {
        self.req(root, field, "an int")?
            .as_int()
            .ok_or_else(|| self.error(field, "an int"))
    }

    fn long(&self, root: &Tag, field: &str) -> Result<i64, WorldError> {
        self.req(root, field, "a long")?
            .as_long()
            .ok_or_else(|| self.error(field, "a long"))
    }

    fn strings(&self, root: &Tag, field: &str) -> Result<Vec<String>, WorldError> {
        let Some(tag) = self.get(root, field) else {
            return Ok(Vec::new());
        };
        tag.as_list()
            .and_then(|l| l.iter().map(|t| t.as_str().map(str::to_string)).collect())
            .ok_or_else(|| self.error(field, "a list of strings"))
    }

    /// A position: a list of three numbers (`Pos`, doubles) or an int array (`spawn.pos`).
    fn pos(&self, root: &Tag, field: &str) -> Result<[f64; 3], WorldError> {
        let tag = self.req(root, field, "a position")?;
        let values: Option<Vec<f64>> = match tag {
            Tag::IntArray(a) => Some(a.iter().map(|&v| f64::from(v)).collect()),
            Tag::List(l) => l
                .iter()
                .map(|t| t.as_double().or_else(|| t.as_int().map(f64::from)))
                .collect(),
            _ => None,
        };
        match values.as_deref() {
            Some(&[x, y, z]) => Ok([x, y, z]),
            _ => Err(self.error(field, "three coordinates")),
        }
    }
}

/// `UUIDUtil.uuidFromIntArray` as the hyphenated lower-case text player files are named by.
fn uuid_text(ints: &[i32]) -> Option<String> {
    let [a, b, c, d] = *ints else { return None };
    let (a, b, c, d) = (a as u32, b as u32, c as u32, d as u32);
    Some(format!(
        "{a:08x}-{:04x}-{:04x}-{:04x}-{:04x}{d:08x}",
        b >> 16,
        b & 0xffff,
        c >> 16,
        c & 0xffff
    ))
}

impl Save {
    /// Whether `dir` looks like a save: it has a `level.dat`.
    pub fn is_save(dir: &Path) -> bool {
        dir.join("level.dat").is_file()
    }

    /// Reads the save in `dir`: `level.dat`, then the generator settings and the single player
    /// from wherever its layout keeps them.
    pub fn read(dir: impl Into<PathBuf>) -> Result<Save, WorldError> {
        let dir = dir.into();
        let level_path = dir.join("level.dat");
        let level = read_nbt(&level_path)?;
        let lf = Fields { path: &level_path };

        let (layout, seed, generate_structures, generators) = Self::world_gen(&dir, &level, &lf)?;

        let spawn = match lf.get(&level, "Data.spawn") {
            // 26.x: Data.spawn {pos: [I; x, y, z], dimension}.
            Some(spawn) => Some(Location {
                dimension: spawn
                    .get("dimension")
                    .and_then(Tag::as_str)
                    .unwrap_or("minecraft:overworld")
                    .to_string(),
                pos: lf.pos(spawn, "pos")?,
            }),
            // Before: Data.SpawnX, SpawnY, SpawnZ, in the overworld.
            None => match (
                lf.get(&level, "Data.SpawnX").and_then(Tag::as_int),
                lf.get(&level, "Data.SpawnY").and_then(Tag::as_int),
                lf.get(&level, "Data.SpawnZ").and_then(Tag::as_int),
            ) {
                (Some(x), Some(y), Some(z)) => Some(Location {
                    dimension: "minecraft:overworld".into(),
                    pos: [f64::from(x), f64::from(y), f64::from(z)],
                }),
                _ => None,
            },
        };

        let player = Self::player(&dir, &level, &lf)?;
        Ok(Save {
            layout,
            name: lf
                .get(&level, "Data.LevelName")
                .and_then(Tag::as_str)
                .unwrap_or_default()
                .to_string(),
            data_version: match lf.get(&level, "Data.DataVersion") {
                None => None,
                Some(_) => Some(lf.int(&level, "Data.DataVersion")?),
            },
            version_name: lf
                .get(&level, "Data.Version.Name")
                .and_then(Tag::as_str)
                .map(str::to_string),
            seed,
            generate_structures,
            generators,
            enabled_packs: lf.strings(&level, "Data.DataPacks.Enabled")?,
            disabled_packs: lf.strings(&level, "Data.DataPacks.Disabled")?,
            was_modded: lf
                .get(&level, "Data.WasModded")
                .and_then(Tag::as_int)
                .is_some_and(|v| v != 0),
            spawn,
            player,
            dir,
        })
    }

    /// The layout, seed, whether structures generate, and each dimension's generator.
    fn world_gen(
        dir: &Path,
        level: &Tag,
        lf: &Fields<'_>,
    ) -> Result<(Layout, i64, bool, BTreeMap<String, Value>), WorldError> {
        let settings_path = dir
            .join("data")
            .join("minecraft")
            .join("world_gen_settings.dat");
        let (layout, settings_file, settings_root, settings) = if settings_path.is_file() {
            let tag = read_nbt(&settings_path)?;
            (Layout::Current, settings_path, tag, "data")
        } else if lf.get(level, "Data.WorldGenSettings").is_some() {
            (
                Layout::Legacy,
                lf.path.to_path_buf(),
                level.clone(),
                "Data.WorldGenSettings",
            )
        } else {
            // Before 1.16: a seed, a generator name and options, and MapFeatures.
            let mut generator = serde_json::Map::new();
            for (key, field) in [
                ("name", "Data.generatorName"),
                ("options", "Data.generatorOptions"),
            ] {
                if let Some(v) = lf.get(level, field) {
                    generator.insert(key.into(), v.to_json());
                }
            }
            let generators = BTreeMap::from([(
                "minecraft:overworld".to_string(),
                serde_json::json!({ "legacy_generator": generator }),
            )]);
            let structures = lf
                .get(level, "Data.MapFeatures")
                .and_then(Tag::as_int)
                .is_none_or(|v| v != 0);
            return Ok((
                Layout::Before116,
                lf.long(level, "Data.RandomSeed")?,
                structures,
                generators,
            ));
        };
        let sf = Fields {
            path: &settings_file,
        };
        let field = |name: &str| format!("{settings}.{name}");
        let generate = sf
            .get(&settings_root, &field("generate_structures"))
            .or_else(|| sf.get(&settings_root, &field("generate_features")));
        let generate_structures = match generate {
            None => true,
            Some(t) => {
                t.as_int()
                    .ok_or_else(|| sf.error(&field("generate_structures"), "a boolean"))?
                    != 0
            }
        };
        let dimensions = sf
            .req(
                &settings_root,
                &field("dimensions"),
                "a compound of dimensions",
            )?
            .as_compound()
            .ok_or_else(|| sf.error(&field("dimensions"), "a compound of dimensions"))?;
        let generators = dimensions
            .iter()
            .filter_map(|(id, dim)| Some((id.clone(), dim.get("generator")?.to_json())))
            .collect();
        Ok((
            layout,
            sf.long(&settings_root, &field("seed"))?,
            generate_structures,
            generators,
        ))
    }

    /// The single player: `Data.Player` before 26.x; in 26.x, `players/data/<uuid>.dat` for
    /// `Data.singleplayer_uuid`. `None` for server saves.
    fn player(dir: &Path, level: &Tag, lf: &Fields<'_>) -> Result<Option<Location>, WorldError> {
        let location = |fields: &Fields<'_>, tag: &Tag| -> Result<Location, WorldError> {
            // An id since 1.16; before, an int: 0, -1 or 1.
            let dimension = match tag.get("Dimension") {
                Some(Tag::String(id)) => id.clone(),
                Some(t) => match t.as_int() {
                    Some(-1) => "minecraft:the_nether".into(),
                    Some(1) => "minecraft:the_end".into(),
                    _ => "minecraft:overworld".into(),
                },
                None => "minecraft:overworld".into(),
            };
            Ok(Location {
                dimension,
                pos: fields.pos(tag, "Pos")?,
            })
        };
        if let Some(player) = lf.get(level, "Data.Player") {
            return location(lf, player).map(Some);
        }
        let Some(Tag::IntArray(ints)) = lf.get(level, "Data.singleplayer_uuid") else {
            return Ok(None);
        };
        let uuid =
            uuid_text(ints).ok_or_else(|| lf.error("Data.singleplayer_uuid", "four ints"))?;
        let path = dir.join("players").join("data").join(format!("{uuid}.dat"));
        if !path.is_file() {
            return Ok(None);
        }
        let tag = read_nbt(&path)?;
        location(&Fields { path: &path }, &tag).map(Some)
    }

    /// Where a dimension's region files are, e.g. `minecraft:overworld`. An id that isn't a
    /// valid identifier, such as one with a `..` segment, is an error rather than a path that
    /// could lead out of the save.
    pub fn region_dir(&self, dimension: &str) -> Result<PathBuf, WorldError> {
        spyglass_data::bundle::Bundle::resource_path("dimension", dimension)
            .map_err(|_| WorldError::DimensionId(dimension.to_string()))?;
        let (namespace, path) = dimension
            .split_once(':')
            .unwrap_or(("minecraft", dimension));
        let custom = || {
            self.dir
                .join("dimensions")
                .join(namespace)
                .join(path)
                .join("region")
        };
        Ok(match (self.layout, dimension) {
            (Layout::Current, _) => custom(),
            (_, "minecraft:overworld") => self.dir.join("region"),
            (_, "minecraft:the_nether") => self.dir.join("DIM-1").join("region"),
            (_, "minecraft:the_end") => self.dir.join("DIM1").join("region"),
            (_, _) => custom(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuids_are_hyphenated_hex() {
        let ints = [91_888_567, 345_196_078, -1_197_807_316, -1_277_292_734];
        assert_eq!(
            uuid_text(&ints).unwrap(),
            "057a1bb7-1493-462e-b89a-e92cb3de0f42"
        );
        assert_eq!(uuid_text(&[1, 2]), None);
    }
}
