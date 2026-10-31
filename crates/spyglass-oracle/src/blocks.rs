//! Block golden files (`docs/specs/oracle.md#block-golden-files`): the game's blocks for whole
//! chunks as generation leaves them before features, with and without carvers.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

/// One block file: a seed and dimension's chunks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockFile {
    /// The game version, `+no-carvers` for the runs without carvers.
    pub version: String,
    /// The world seed.
    pub seed: i64,
    /// The dimension's id, e.g. `minecraft:overworld`.
    pub dimension: String,
    /// The level's lowest y.
    pub min_y: i32,
    /// The level's height in blocks.
    pub height: u32,
    /// The chunks, in the harness's order.
    pub chunks: Vec<GoldenChunk>,
}

/// One chunk's blocks: every position's state, as an index into a palette of state names
/// (`id[property=value,…]`, non-default properties only, by name).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoldenChunk {
    /// Chunk x and z.
    pub chunk: [i32; 2],
    /// The level's lowest y.
    min_y: i32,
    /// The level's height in blocks, so each column's length.
    height: u32,
    /// The chunk's states, in the order first met.
    palette: Vec<String>,
    /// By column (x fastest within z), then y from the bottom.
    blocks: Vec<u16>,
}

impl GoldenChunk {
    /// The state at block `(x, y, z)`, which must be in this chunk and the level.
    ///
    /// # Panics
    ///
    /// If it isn't.
    pub fn block(&self, [x, y, z]: [i32; 3]) -> &str {
        let [cx, cz] = self.chunk;
        assert!(
            x >> 4 == cx && z >> 4 == cz,
            "block ({x}, {z}) isn't in chunk {cx}, {cz}"
        );
        let dy = y - self.min_y;
        assert!(
            dy >= 0 && (dy as u32) < self.height,
            "y {y} is outside the level"
        );
        let column = ((z & 15) * 16 + (x & 15)) as usize;
        &self.palette[usize::from(self.blocks[column * self.height as usize + dy as usize])]
    }

    /// The chunk's blocks, each with its position, column by column from the bottom up.
    pub fn blocks(&self) -> impl Iterator<Item = ([i32; 3], &str)> + '_ {
        let [west, north] = self.chunk.map(|c| c * 16);
        let height = self.height as usize;
        self.blocks.iter().enumerate().map(move |(i, &state)| {
            let (column, dy) = (i / height, i % height);
            let pos = [
                west + (column % 16) as i32,
                self.min_y + dy as i32,
                north + (column / 16) as i32,
            ];
            (pos, self.palette[usize::from(state)].as_str())
        })
    }

    /// The first position, column by column from the bottom up, where `found` differs from the
    /// game, with both states.
    pub fn first_difference<S: AsRef<str>>(
        &self,
        mut found: impl FnMut([i32; 3]) -> S,
    ) -> Option<Difference> {
        self.blocks().find_map(|(pos, want)| {
            let got = found(pos);
            (got.as_ref() != want).then(|| Difference {
                pos,
                expected: want.to_string(),
                found: got.as_ref().to_string(),
            })
        })
    }
}

/// Where a chunk's blocks first differ from the game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Difference {
    /// The block x, y and z.
    pub pos: [i32; 3],
    /// The game's state there.
    pub expected: String,
    /// The state found instead.
    pub found: String,
}

impl std::fmt::Display for Difference {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let [x, y, z] = self.pos;
        write!(
            f,
            "at {x} {y} {z}: the game has {}, found {}",
            self.expected, self.found
        )
    }
}

impl BlockFile {
    /// Reads a block file.
    pub fn read(path: &Path) -> Result<Self, String> {
        let bytes = fs::read(path).map_err(|e| format!("reading {}: {e}", path.display()))?;
        Self::parse(&bytes).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Parses a block file's bytes.
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
        let min_y = int(&file["min_y"], "min_y")? as i32;
        let height = int(&file["height"], "height")? as u32;
        // Each column is a list of runs, `[state, count]` from the bottom up; they're expanded
        // here into one palette index per block.
        let mut chunks = Vec::new();
        for chunk in file["chunks"].as_array().ok_or("no chunks")? {
            let pos = [
                int(&chunk["chunk"][0], "chunk")? as i32,
                int(&chunk["chunk"][1], "chunk")? as i32,
            ];
            let columns = chunk["columns"].as_array().ok_or("no columns")?;
            if columns.len() != 256 {
                return Err(format!("chunk {pos:?} has {} columns", columns.len()));
            }
            let mut palette: Vec<String> = Vec::new();
            let mut blocks = Vec::with_capacity(256 * height as usize);
            for column in columns {
                let start = blocks.len();
                for run in column.as_array().ok_or("a column isn't a list")? {
                    let state = run[0].as_str().ok_or("a run has no state")?;
                    let count = int(&run[1], "a run's count")? as usize;
                    let index = match palette.iter().position(|s| s == state) {
                        Some(i) => i,
                        None => {
                            palette.push(state.to_string());
                            palette.len() - 1
                        }
                    };
                    let index = u16::try_from(index).map_err(|_| "too many states")?;
                    blocks.extend(std::iter::repeat_n(index, count));
                }
                if blocks.len() - start != height as usize {
                    return Err(format!(
                        "a column of chunk {pos:?} covers {} blocks, not {height}",
                        blocks.len() - start
                    ));
                }
            }
            chunks.push(GoldenChunk {
                chunk: pos,
                min_y,
                height,
                palette,
                blocks,
            });
        }
        Ok(BlockFile {
            version: text("version")?,
            seed: int(&file["seed"], "seed")?,
            dimension: text("dimension")?,
            min_y,
            height,
            chunks,
        })
    }
}

/// The block files of [`crate::version`], with carvers or without, sorted: every dimension's.
/// Empty if none have been generated.
pub fn files(carvers: bool) -> Vec<PathBuf> {
    let mut version = crate::version();
    if !carvers {
        version.push_str("+no-carvers");
    }
    let dir = crate::default_dir().join(version).join("blocks");
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

    /// A chunk of a level 4 high: bedrock under stone, except column (1, 0) of chunk (-1, 2),
    /// which has a grass block on top.
    fn file() -> String {
        let mut columns = vec![r#"[["minecraft:bedrock", 1], ["minecraft:stone", 3]]"#; 256];
        columns[1] = r#"[["minecraft:bedrock", 1], ["minecraft:stone", 2], ["minecraft:grass_block[snowy=true]", 1]]"#;
        format!(
            r#"{{"format": 1, "harness": 1, "version": "26.3", "data_version": 5023, "seed": -7,
            "dimension": "minecraft:overworld", "min_y": -2, "height": 4,
            "chunks": [{{"chunk": [-1, 2], "columns": [{}]}}]}}"#,
            columns.join(",")
        )
    }

    #[test]
    fn runs_expand_to_positions() {
        let file = BlockFile::parse(file().as_bytes()).unwrap();
        assert_eq!((file.seed, file.min_y, file.height), (-7, -2, 4));
        let chunk = &file.chunks[0];
        assert_eq!(chunk.block([-16, -2, 32]), "minecraft:bedrock");
        assert_eq!(
            chunk.block([-15, 1, 32]),
            "minecraft:grass_block[snowy=true]"
        );
        assert_eq!(chunk.block([-1, 1, 47]), "minecraft:stone");
        assert_eq!(chunk.blocks().count(), 256 * 4);
        let (pos, state) = chunk.blocks().nth(7).unwrap();
        assert_eq!(
            (pos, state),
            ([-15, 1, 32], "minecraft:grass_block[snowy=true]")
        );
    }

    #[test]
    fn the_first_difference_is_named() {
        let file = BlockFile::parse(file().as_bytes()).unwrap();
        let chunk = &file.chunks[0];
        assert_eq!(chunk.first_difference(|p| chunk.block(p).to_string()), None);
        let diff = chunk
            .first_difference(|p| {
                if p[1] == 1 {
                    "minecraft:stone"
                } else {
                    chunk.block(p)
                }
            })
            .unwrap();
        assert_eq!(diff.pos, [-15, 1, 32]);
        assert_eq!(
            diff.to_string(),
            "at -15 1 32: the game has minecraft:grass_block[snowy=true], found minecraft:stone"
        );
    }

    #[test]
    fn bad_files_say_why() {
        let short = file().replace(r#"["minecraft:stone", 3]"#, r#"["minecraft:stone", 2]"#);
        assert!(
            BlockFile::parse(short.as_bytes())
                .unwrap_err()
                .contains("covers 3")
        );
        let format = file().replace(r#""format": 1"#, r#""format": 2"#);
        assert!(
            BlockFile::parse(format.as_bytes())
                .unwrap_err()
                .contains("format")
        );
    }
}
