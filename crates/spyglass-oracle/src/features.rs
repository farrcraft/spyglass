//! Feature golden files (`docs/specs/oracle.md#feature-golden-files`): each chunk decorated on
//! its own, every call to a target feature with where and whether it placed, and each step's
//! feature order.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

/// One seed and dimension's feature file.
#[derive(Debug, Clone)]
pub struct FeatureFile {
    /// The game version.
    pub version: String,
    /// The seed.
    pub seed: i64,
    /// The dimension's id.
    pub dimension: String,
    /// Each step's placed features in decoration order: ids, or `None` for an inline one.
    pub order: Vec<Vec<Option<String>>>,
    /// The chunks decorated.
    pub chunks: Vec<FeatureChunk>,
}

/// One decorated chunk.
#[derive(Debug, Clone)]
pub struct FeatureChunk {
    /// Chunk x and z.
    pub chunk: [i32; 2],
    /// The calls to target features, in the order they happened.
    pub tries: Vec<GoldenTry>,
    /// The same, with the chunk's 8 neighbours decorated first; `None` in files made before
    /// the harness ran it.
    pub after_neighbours: Option<Vec<GoldenTry>>,
}

/// A call to a target feature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoldenTry {
    /// The placed feature running.
    pub feature: String,
    /// The origin the feature was called with.
    pub pos: [i32; 3],
    /// Whether it placed.
    pub placed: bool,
}

impl FeatureFile {
    /// Reads a feature file.
    pub fn read(path: &Path) -> Result<Self, String> {
        let bytes = fs::read(path).map_err(|e| format!("reading {}: {e}", path.display()))?;
        Self::parse(&bytes).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Parses a feature file's bytes.
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let file: Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        if file["format"] != 1 {
            return Err(format!("format {} isn't 1", file["format"]));
        }
        let text = |key: &str| {
            file[key]
                .as_str()
                .map(str::to_string)
                .ok_or(format!("no {key}"))
        };
        let int = |v: &Value, what: &str| v.as_i64().ok_or(format!("{what} isn't an integer"));
        let order = file["order"]
            .as_array()
            .ok_or("no order")?
            .iter()
            .map(|step| {
                step.as_array().ok_or("a step isn't a list").map(|ids| {
                    ids.iter()
                        .map(|id| id.as_str().map(str::to_string))
                        .collect()
                })
            })
            .collect::<Result<_, _>>()?;
        let mut chunks = Vec::new();
        for chunk in file["chunks"].as_array().ok_or("no chunks")? {
            let pos = [
                int(&chunk["chunk"][0], "chunk")? as i32,
                int(&chunk["chunk"][1], "chunk")? as i32,
            ];
            let calls = |list: &Value| -> Result<Vec<GoldenTry>, String> {
                let mut tries = Vec::new();
                for t in list.as_array().ok_or("no tries")? {
                    tries.push(GoldenTry {
                        feature: t["feature"]
                            .as_str()
                            .ok_or("a try has no feature")?
                            .to_string(),
                        pos: [
                            int(&t["pos"][0], "pos")? as i32,
                            int(&t["pos"][1], "pos")? as i32,
                            int(&t["pos"][2], "pos")? as i32,
                        ],
                        placed: t["placed"].as_bool().ok_or("placed isn't a bool")?,
                    });
                }
                Ok(tries)
            };
            let tries = calls(&chunk["tries"])?;
            let after_neighbours = match chunk.get("after_neighbours") {
                None => None,
                Some(list) => Some(calls(list)?),
            };
            chunks.push(FeatureChunk {
                chunk: pos,
                tries,
                after_neighbours,
            });
        }
        Ok(FeatureFile {
            version: text("version")?,
            seed: int(&file["seed"], "seed")?,
            dimension: text("dimension")?,
            order,
            chunks,
        })
    }
}

/// The feature files of [`crate::version`], sorted: every dimension's. Empty if none have been
/// generated.
pub fn files() -> Vec<PathBuf> {
    let dir = crate::default_dir().join(crate::version()).join("features");
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
        let text = r#"{"format": 1, "harness": 1, "version": "test", "data_version": 1, "seed": -5,
            "dimension": "minecraft:overworld",
            "order": [[], ["minecraft:a", null]],
            "chunks": [{"chunk": [-1, 2], "tries": [
                {"feature": "minecraft:a", "pos": [-12, 40, 33], "placed": true}],
                "after_neighbours": []}]}"#;
        let file = FeatureFile::parse(text.as_bytes()).unwrap();
        assert_eq!(
            (file.seed, file.dimension.as_str()),
            (-5, "minecraft:overworld")
        );
        assert_eq!(
            file.order,
            [vec![], vec![Some("minecraft:a".to_string()), None]]
        );
        assert_eq!(file.chunks[0].chunk, [-1, 2]);
        assert_eq!(
            file.chunks[0].tries,
            [GoldenTry {
                feature: "minecraft:a".into(),
                pos: [-12, 40, 33],
                placed: true
            }]
        );
        assert_eq!(file.chunks[0].after_neighbours, Some(vec![]));
        assert!(FeatureFile::parse(br#"{"format": 2}"#).is_err());
    }
}
