//! Container golden files (`docs/specs/oracle.md#container-golden-files`): the nearest start of
//! each structure, and every chunk those starts reach decorated on its own with structures, with
//! each container with a loot table decoration made and what made it.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

/// One seed and dimension's container file.
#[derive(Debug, Clone)]
pub struct ContainerFile {
    /// The game version.
    pub version: String,
    /// The seed.
    pub seed: i64,
    /// The dimension's id.
    pub dimension: String,
    /// The nearest start of each structure, by structure id.
    pub starts: Vec<GoldenStart>,
    /// The chunks decorated.
    pub chunks: Vec<ContainerChunk>,
}

/// A structure's start, as assembly left it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoldenStart {
    /// The structure's id.
    pub structure: String,
    /// Its start chunk.
    pub chunk: [i32; 2],
    /// The box around its pieces: least x, y, z, then greatest.
    pub bounds: [i32; 6],
    /// Each piece's box, in the start's order.
    pub pieces: Vec<[i32; 6]>,
}

/// One decorated chunk and the containers its decoration made, in the order it made them.
#[derive(Debug, Clone)]
pub struct ContainerChunk {
    /// Chunk x and z.
    pub chunk: [i32; 2],
    /// The containers.
    pub containers: Vec<GoldenContainer>,
}

/// A container with a loot table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoldenContainer {
    /// What placed it: a structure's id, or a placed feature's.
    pub by: String,
    /// Its block, or for an entity the block it's in.
    pub pos: [i32; 3],
    /// The block, e.g. `minecraft:chest`, or the entity, e.g. `minecraft:chest_minecart`.
    pub what: Holder,
    /// The loot table's id.
    pub table: String,
    /// The loot seed.
    pub seed: i64,
}

/// What holds a container's items.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Holder {
    /// A block, by id.
    Block(String),
    /// An entity, by id.
    Entity(String),
}

impl ContainerFile {
    /// Reads a container file.
    pub fn read(path: &Path) -> Result<Self, String> {
        let bytes = fs::read(path).map_err(|e| format!("reading {}: {e}", path.display()))?;
        Self::parse(&bytes).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Parses a container file's bytes.
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let file: Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        if file["format"] != 1 {
            return Err(format!("format {} isn't 1", file["format"]));
        }
        let text =
            |v: &Value, what: &str| v.as_str().map(str::to_string).ok_or(format!("no {what}"));
        let int = |v: &Value, what: &str| -> Result<i32, String> {
            v.as_i64()
                .and_then(|i| i32::try_from(i).ok())
                .ok_or(format!("{what} isn't an int"))
        };
        let ints = |v: &Value, what: &str, out: &mut [i32]| -> Result<(), String> {
            let list = v.as_array().ok_or(format!("{what} isn't a list"))?;
            if list.len() != out.len() {
                return Err(format!("{what} doesn't have {} ints", out.len()));
            }
            for (o, v) in out.iter_mut().zip(list) {
                *o = int(v, what)?;
            }
            Ok(())
        };
        let mut starts = Vec::new();
        for s in file["starts"].as_array().ok_or("no starts")? {
            let mut start = GoldenStart {
                structure: text(&s["structure"], "structure")?,
                chunk: [0; 2],
                bounds: [0; 6],
                pieces: Vec::new(),
            };
            ints(&s["chunk"], "chunk", &mut start.chunk)?;
            ints(&s["bounds"], "bounds", &mut start.bounds)?;
            for p in s["pieces"].as_array().ok_or("no pieces")? {
                let mut piece = [0; 6];
                ints(p, "piece", &mut piece)?;
                start.pieces.push(piece);
            }
            starts.push(start);
        }
        let mut chunks = Vec::new();
        for c in file["chunks"].as_array().ok_or("no chunks")? {
            let mut chunk = ContainerChunk {
                chunk: [0; 2],
                containers: Vec::new(),
            };
            ints(&c["chunk"], "chunk", &mut chunk.chunk)?;
            for k in c["containers"].as_array().ok_or("no containers")? {
                let mut pos = [0; 3];
                ints(&k["pos"], "pos", &mut pos)?;
                let what = match (k.get("block"), k.get("entity")) {
                    (Some(b), None) => Holder::Block(text(b, "block")?),
                    (None, Some(e)) => Holder::Entity(text(e, "entity")?),
                    _ => return Err("a container isn't one block or one entity".into()),
                };
                chunk.containers.push(GoldenContainer {
                    by: text(&k["by"], "by")?,
                    pos,
                    what,
                    table: text(&k["table"], "table")?,
                    seed: k["seed"].as_i64().ok_or("seed isn't an integer")?,
                });
            }
            chunks.push(chunk);
        }
        Ok(ContainerFile {
            version: text(&file["version"], "version")?,
            seed: file["seed"].as_i64().ok_or("seed isn't an integer")?,
            dimension: text(&file["dimension"], "dimension")?,
            starts,
            chunks,
        })
    }
}

/// The container files of [`crate::version`], sorted: every dimension's. Empty if none have been
/// generated.
pub fn files() -> Vec<PathBuf> {
    let dir = crate::default_dir()
        .join(crate::version())
        .join("containers");
    let mut out: Vec<PathBuf> = ["overworld", "the_nether", "the_end"]
        .into_iter()
        .flat_map(|d| fs::read_dir(dir.join(d)))
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_parses() {
        let text = r#"{"format": 1, "harness": 1, "version": "test", "data_version": 1, "seed": 3,
            "dimension": "minecraft:overworld",
            "starts": [{"structure": "minecraft:village_plains", "chunk": [1, -2],
                "bounds": [0, 60, -40, 40, 80, 0], "pieces": [[1, 60, -30, 9, 70, -20]]}],
            "chunks": [{"chunk": [1, -2], "containers": [
                {"by": "minecraft:village_plains", "pos": [20, 64, -25], "block": "minecraft:chest",
                 "table": "minecraft:chests/village/village_plains_house", "seed": -4},
                {"by": "minecraft:mineshaft", "pos": [21, 30, -25], "entity": "minecraft:chest_minecart",
                 "table": "minecraft:chests/abandoned_mineshaft", "seed": 9007199254740993}]}]}"#;
        let file = ContainerFile::parse(text.as_bytes()).unwrap();
        assert_eq!(file.seed, 3);
        assert_eq!(file.starts[0].pieces, [[1, 60, -30, 9, 70, -20]]);
        let containers = &file.chunks[0].containers;
        assert_eq!(containers[0].what, Holder::Block("minecraft:chest".into()));
        assert_eq!(
            containers[1].what,
            Holder::Entity("minecraft:chest_minecart".into())
        );
        assert_eq!(containers[1].seed, 9007199254740993);
        assert!(ContainerFile::parse(br#"{"format": 2}"#).is_err());
    }
}
