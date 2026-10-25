//! Reads the oracle's golden files. Test support: use it only as a dev-dependency.
//!
//! Golden files are generated locally by `cargo xtask oracle golden` into `oracle-out/` and never
//! committed. The format and the comparison rule are specified in `docs/specs/oracle.md`.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const MAGIC: &[u8; 8] = b"SPYBIOME";
/// The golden biome format version this reader understands.
pub const FORMAT: u32 = 1;

#[derive(Debug, thiserror::Error)]
pub enum GoldenError {
    #[error("I/O error on {path}: {source}")]
    Io { path: PathBuf, source: io::Error },
    #[error("not a golden biome file")]
    BadMagic,
    #[error("golden format {0}, but this reader understands {FORMAT}; regenerate the golden files")]
    UnsupportedFormat(u32),
    #[error("malformed header: {0}")]
    Header(#[from] serde_json::Error),
    #[error("malformed file: {0}")]
    Malformed(&'static str),
}

/// The JSON header. Field order matches the file, so [`BiomeGrid::to_bytes`] reproduces it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Header {
    pub format: u32,
    /// Harness version; changes when the harness's output for a game version changes.
    pub harness: u32,
    /// Game version id, e.g. `26.3`.
    pub version: String,
    pub data_version: i32,
    pub seed: i64,
    /// e.g. `minecraft:overworld`.
    pub dimension: String,
    /// Minimum quart x, y, z.
    pub min: [i32; 3],
    /// Size in quarts along x, y, z.
    pub size: [u32; 3],
    /// Biome ids, sorted.
    pub palette: Vec<String>,
    /// Number of tied cells.
    pub ties: u32,
}

/// What the game gives for one cell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cell<'a> {
    /// A unique minimum: spyglass must return exactly this biome.
    Unique(&'a str),
    /// Several biomes tie at the minimum: the game may return any of them, so spyglass may too.
    Tied(Vec<&'a str>),
}

impl Cell<'_> {
    /// Whether `biome` is a correct answer for this cell.
    pub fn accepts(&self, biome: &str) -> bool {
        match self {
            Cell::Unique(b) => *b == biome,
            Cell::Tied(set) => set.contains(&biome),
        }
    }
}

/// A golden biome grid: a box of quart cells for one seed and dimension.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BiomeGrid {
    pub header: Header,
    /// Palette index per cell, x fastest, then z, then y. A tied cell holds its tie set's lowest
    /// index.
    cells: Vec<u16>,
    /// Tie sets by cell index, each sorted ascending.
    ties: BTreeMap<u32, Vec<u16>>,
}

impl BiomeGrid {
    pub fn read(path: &Path) -> Result<Self, GoldenError> {
        let bytes = fs::read(path).map_err(|source| GoldenError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        Self::parse(&bytes)
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, GoldenError> {
        let mut r = Reader(bytes);
        if r.take(8)? != MAGIC {
            return Err(GoldenError::BadMagic);
        }
        let header_len = r.u32()? as usize;
        let header_bytes = r.take(header_len)?;
        // Check the format before the rest of the header, so a newer file gets a clear error.
        #[derive(Deserialize)]
        struct Format {
            format: u32,
        }
        let Format { format } = serde_json::from_slice(header_bytes)?;
        if format != FORMAT {
            return Err(GoldenError::UnsupportedFormat(format));
        }
        let header: Header = serde_json::from_slice(header_bytes)?;

        let palette_len = header.palette.len();
        if palette_len > usize::from(u16::MAX) + 1 {
            return Err(GoldenError::Malformed("palette too large"));
        }
        if !header.palette.is_sorted_by(|a, b| a < b) {
            return Err(GoldenError::Malformed("palette not sorted and unique"));
        }
        let count = header
            .size
            .iter()
            .try_fold(1usize, |n, &s| n.checked_mul(s as usize))
            .ok_or(GoldenError::Malformed("grid too large"))?;
        let in_palette = |i: u16| usize::from(i) < palette_len;

        let cells = (0..count).map(|_| r.u16()).collect::<Result<Vec<_>, _>>()?;
        if !cells.iter().all(|&c| in_palette(c)) {
            return Err(GoldenError::Malformed("cell outside the palette"));
        }

        let mut ties = BTreeMap::new();
        let mut last = None;
        for _ in 0..header.ties {
            let index = r.u32()?;
            if usize::try_from(index).map_or(true, |i| i >= count) || last >= Some(index) {
                return Err(GoldenError::Malformed(
                    "tie index out of range or out of order",
                ));
            }
            last = Some(index);
            let n = r.u16()?;
            let set = (0..n).map(|_| r.u16()).collect::<Result<Vec<_>, _>>()?;
            if set.len() < 2
                || !set.is_sorted_by(|a, b| a < b)
                || !set.iter().all(|&i| in_palette(i))
            {
                return Err(GoldenError::Malformed("bad tie set"));
            }
            if cells[index as usize] != set[0] {
                return Err(GoldenError::Malformed(
                    "tied cell doesn't hold its lowest tied index",
                ));
            }
            ties.insert(index, set);
        }
        if !r.0.is_empty() {
            return Err(GoldenError::Malformed("trailing bytes"));
        }
        Ok(BiomeGrid {
            header,
            cells,
            ties,
        })
    }

    /// The file's bytes. Parsing a file and writing it back gives the same bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let header = serde_json::to_vec(&self.header).expect("headers always serialize");
        let mut out = Vec::with_capacity(12 + header.len() + 2 * self.cells.len());
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&(header.len() as u32).to_le_bytes());
        out.extend_from_slice(&header);
        for c in &self.cells {
            out.extend_from_slice(&c.to_le_bytes());
        }
        for (index, set) in &self.ties {
            out.extend_from_slice(&index.to_le_bytes());
            out.extend_from_slice(&(set.len() as u16).to_le_bytes());
            for i in set {
                out.extend_from_slice(&i.to_le_bytes());
            }
        }
        out
    }

    /// The cell index of a quart position, or `None` outside the grid.
    fn index(&self, qx: i32, qy: i32, qz: i32) -> Option<usize> {
        let [x0, y0, z0] = self.header.min;
        let [sx, sy, sz] = self.header.size.map(i64::from);
        let (x, y, z) = (
            i64::from(qx) - i64::from(x0),
            i64::from(qy) - i64::from(y0),
            i64::from(qz) - i64::from(z0),
        );
        if !(0..sx).contains(&x) || !(0..sy).contains(&y) || !(0..sz).contains(&z) {
            return None;
        }
        Some(((y * sz + z) * sx + x) as usize)
    }

    fn cell_at(&self, index: usize) -> Cell<'_> {
        let name = |i: &u16| self.header.palette[usize::from(*i)].as_str();
        match self.ties.get(&(index as u32)) {
            Some(set) => Cell::Tied(set.iter().map(name).collect()),
            None => Cell::Unique(name(&self.cells[index])),
        }
    }

    /// The cell at a quart position, or `None` outside the grid.
    pub fn get(&self, qx: i32, qy: i32, qz: i32) -> Option<Cell<'_>> {
        self.index(qx, qy, qz).map(|i| self.cell_at(i))
    }

    /// Every cell with its quart position, in file order.
    pub fn cells(&self) -> impl Iterator<Item = ([i32; 3], Cell<'_>)> {
        let [x0, y0, z0] = self.header.min;
        let [sx, _, sz] = self.header.size.map(|s| s as usize);
        (0..self.cells.len()).map(move |i| {
            let (x, z, y) = (i % sx, (i / sx) % sz, i / (sx * sz));
            (
                [x0 + x as i32, y0 + y as i32, z0 + z as i32],
                self.cell_at(i),
            )
        })
    }

    /// Number of tied cells.
    pub fn tie_count(&self) -> usize {
        self.ties.len()
    }
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], GoldenError> {
        if self.0.len() < n {
            return Err(GoldenError::Malformed("truncated"));
        }
        let (head, rest) = self.0.split_at(n);
        self.0 = rest;
        Ok(head)
    }

    fn u16(&mut self) -> Result<u16, GoldenError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }

    fn u32(&mut self) -> Result<u32, GoldenError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
}

/// `oracle-out/` at the repo root, where `cargo xtask oracle golden` writes.
pub fn default_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../oracle-out")
}

/// Every golden biome file for a game version and dimension (e.g. `overworld`), sorted. Empty if
/// none have been generated.
pub fn biome_files(root: &Path, version: &str, dimension: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let dir = root.join(version).join("biomes").join(dimension);
    let Ok(seeds) = fs::read_dir(dir) else {
        return out;
    };
    for seed in seeds.flatten() {
        if let Ok(files) = fs::read_dir(seed.path()) {
            out.extend(
                files
                    .flatten()
                    .map(|f| f.path())
                    .filter(|p| p.extension().is_some_and(|e| e == "bin")),
            );
        }
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 2×2×1 grid at quart (-1, 5, -1): plains, desert, a plains/savanna tie, and forest.
    fn sample() -> Vec<u8> {
        let header = r#"{"format":1,"harness":1,"version":"26.3","dataVersion":5023,"seed":-1,"dimension":"minecraft:overworld","min":[-1,5,-1],"size":[2,1,2],"palette":["minecraft:desert","minecraft:forest","minecraft:plains","minecraft:savanna"],"ties":1}"#;
        let mut b = MAGIC.to_vec();
        b.extend_from_slice(&(header.len() as u32).to_le_bytes());
        b.extend_from_slice(header.as_bytes());
        for c in [2u16, 0, 2, 1] {
            b.extend_from_slice(&c.to_le_bytes());
        }
        b.extend_from_slice(&2u32.to_le_bytes());
        b.extend_from_slice(&2u16.to_le_bytes());
        b.extend_from_slice(&2u16.to_le_bytes());
        b.extend_from_slice(&3u16.to_le_bytes());
        b
    }

    #[test]
    fn reads_cells_and_ties() {
        let grid = BiomeGrid::parse(&sample()).unwrap();
        assert_eq!(grid.header.seed, -1);
        assert_eq!(grid.get(-1, 5, -1), Some(Cell::Unique("minecraft:plains")));
        assert_eq!(grid.get(0, 5, -1), Some(Cell::Unique("minecraft:desert")));
        let tied = grid.get(-1, 5, 0).unwrap();
        assert_eq!(
            tied,
            Cell::Tied(vec!["minecraft:plains", "minecraft:savanna"])
        );
        assert!(tied.accepts("minecraft:savanna") && !tied.accepts("minecraft:forest"));
        assert_eq!(grid.get(0, 5, 0), Some(Cell::Unique("minecraft:forest")));
        assert_eq!(grid.get(1, 5, 0), None);
        assert_eq!(grid.get(0, 6, 0), None);
        assert_eq!(grid.tie_count(), 1);

        let positions: Vec<_> = grid.cells().map(|(p, _)| p).collect();
        assert_eq!(positions, [[-1, 5, -1], [0, 5, -1], [-1, 5, 0], [0, 5, 0]]);
    }

    #[test]
    fn round_trips() {
        let bytes = sample();
        assert_eq!(BiomeGrid::parse(&bytes).unwrap().to_bytes(), bytes);
    }

    #[test]
    fn rejects_damage() {
        let good = sample();
        let mut bad_magic = good.clone();
        bad_magic[0] = b'X';
        assert!(matches!(
            BiomeGrid::parse(&bad_magic),
            Err(GoldenError::BadMagic)
        ));
        assert!(BiomeGrid::parse(&good[..good.len() - 1]).is_err());
        let mut trailing = good.clone();
        trailing.push(0);
        assert!(BiomeGrid::parse(&trailing).is_err());
        // The tied cell must hold the tie set's lowest index.
        let mut wrong_tied_cell = good.clone();
        let cells_at = good.len() - 10 - 8;
        wrong_tied_cell[cells_at + 4] = 3;
        assert!(BiomeGrid::parse(&wrong_tied_cell).is_err());
    }

    #[test]
    fn rejects_newer_formats() {
        let mut bytes = sample();
        let at = bytes
            .windows(10)
            .position(|w| w == br#""format":1"#)
            .unwrap();
        bytes[at + 9] = b'2';
        assert!(matches!(
            BiomeGrid::parse(&bytes),
            Err(GoldenError::UnsupportedFormat(2))
        ));
    }

    /// Checks every generated golden file: it parses, and writing it back gives the same bytes.
    /// Needs `cargo xtask oracle golden` first; run with `cargo test -- --ignored`.
    #[test]
    #[ignore = "needs golden files from cargo xtask oracle golden"]
    fn golden_files_round_trip() {
        let root = default_dir();
        let versions: Vec<_> = fs::read_dir(&root)
            .expect("no oracle-out/; run cargo xtask oracle golden")
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        let mut checked = 0;
        for version in versions {
            for path in biome_files(&root, &version, "overworld") {
                let bytes = fs::read(&path).unwrap();
                let grid = BiomeGrid::parse(&bytes).unwrap_or_else(|e| panic!("{path:?}: {e}"));
                assert_eq!(grid.to_bytes(), bytes, "{path:?}");
                assert_eq!(grid.header.version, version);
                checked += 1;
            }
        }
        assert!(checked > 0, "no golden files found under {root:?}");
    }
}
