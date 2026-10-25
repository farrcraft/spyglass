//! Probe files: raw values from the game's random sources and noises, written by
//! `cargo xtask oracle probe` into `oracle-out/<version>/probe/`. The format is specified in
//! `docs/specs/oracle.md` ("Probe files").

use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

use crate::GoldenError;

/// The probe file format this reader understands.
pub const FORMAT: u32 = 1;

/// One probe file.
#[derive(Debug, Clone, Deserialize)]
pub struct ProbeFile {
    /// Format version; see [`FORMAT`].
    pub format: u32,
    /// Harness version.
    pub harness: u32,
    /// Game version id.
    pub version: String,
    /// The game's data version.
    pub data_version: u32,
    /// Every case, in the order the harness wrote them.
    pub cases: Vec<Case>,
}

impl ProbeFile {
    /// Reads and parses a probe file, rejecting formats this reader doesn't know.
    pub fn read(path: &Path) -> Result<Self, GoldenError> {
        let bytes = fs::read(path).map_err(|source| GoldenError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let file: ProbeFile = serde_json::from_slice(&bytes)?;
        if file.format != FORMAT {
            return Err(GoldenError::UnsupportedFormat(file.format));
        }
        Ok(file)
    }
}

/// One case: what was called, its parameters, and the game's results.
#[derive(Debug, Clone, Deserialize)]
pub struct Case {
    /// What was tested, e.g. `next_int_bounded` or `normal_noise`.
    pub test: String,
    /// For tests with several methods, which one, e.g. `get_3d`.
    pub method: Option<String>,
    /// `xoroshiro` or `legacy`, for random-source cases.
    pub kind: Option<String>,
    /// The seed.
    pub seed: Option<i64>,
    /// The bound passed to `nextInt(int)`.
    pub bound: Option<i32>,
    /// A block position.
    pub pos: Option<[i32; 3]>,
    /// A name hashed by `seedFromHashOf`, or a noise id.
    pub name: Option<String>,
    /// A sample point, as hex `f64` bits.
    #[serde(default)]
    pub point: Vec<String>,
    /// How to read [`values`](Self::values): `i64`, `f32`, `f64` or `text`.
    #[serde(rename = "type")]
    pub kind_of_values: String,
    /// The results.
    pub values: Vec<Value>,
}

impl Case {
    /// A short description for assertion messages.
    pub fn label(&self) -> String {
        let mut s = self.test.clone();
        for (key, value) in [
            ("method", self.method.clone()),
            ("kind", self.kind.clone()),
            ("seed", self.seed.map(|v| v.to_string())),
            ("bound", self.bound.map(|v| v.to_string())),
            ("pos", self.pos.map(|p| format!("{p:?}"))),
            ("name", self.name.clone()),
        ] {
            if let Some(value) = value {
                s.push_str(&format!(" {key}={value}"));
            }
        }
        if !self.point.is_empty() {
            s.push_str(&format!(" point={:?}", self.point_f64()));
        }
        s
    }

    fn expect_type(&self, wanted: &str) {
        assert_eq!(
            self.kind_of_values,
            wanted,
            "{}: values are {}, not {wanted}",
            self.label(),
            self.kind_of_values
        );
    }

    /// The values as integers.
    ///
    /// # Panics
    ///
    /// If the case's values aren't `i64`.
    pub fn i64s(&self) -> Vec<i64> {
        self.expect_type("i64");
        self.values
            .iter()
            .map(|v| v.as_i64().expect("an integer"))
            .collect()
    }

    /// The values as raw `f32` bits.
    ///
    /// # Panics
    ///
    /// If the case's values aren't `f32`.
    pub fn f32_bits(&self) -> Vec<u32> {
        self.expect_type("f32");
        self.values
            .iter()
            .map(|v| u32::from_str_radix(v.as_str().expect("a string"), 16).expect("hex bits"))
            .collect()
    }

    /// The values as raw `f64` bits.
    ///
    /// # Panics
    ///
    /// If the case's values aren't `f64`.
    pub fn f64_bits(&self) -> Vec<u64> {
        self.expect_type("f64");
        self.values
            .iter()
            .map(|v| u64::from_str_radix(v.as_str().expect("a string"), 16).expect("hex bits"))
            .collect()
    }

    /// The single text value.
    ///
    /// # Panics
    ///
    /// If the case's values aren't `text`.
    pub fn text(&self) -> &str {
        self.expect_type("text");
        self.values[0].as_str().expect("a string")
    }

    /// The sample point as `f64`s.
    ///
    /// # Panics
    ///
    /// If a coordinate isn't hex `f64` bits.
    pub fn point_f64(&self) -> Vec<f64> {
        self.point
            .iter()
            .map(|h| f64::from_bits(u64::from_str_radix(h, 16).expect("hex bits")))
            .collect()
    }
}

/// Every probe file with this name (`rng.json`, `noise.json`) under `root`, one per game version,
/// sorted.
pub fn probe_files(root: &Path, name: &str) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path().join("probe").join(name))
        .filter(|path| path.is_file())
        .collect();
    out.sort();
    out
}
