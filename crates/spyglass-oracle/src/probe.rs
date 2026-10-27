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
    /// A volume's height in blocks, from `pos` up.
    pub height: Option<i32>,
    /// A density the case was evaluated with, as hex `f64` bits.
    pub density: Option<String>,
    /// A name hashed by `seedFromHashOf`, or a noise or density function id.
    pub name: Option<String>,
    /// The dimension, for density cases outside the overworld.
    pub dimension: Option<String>,
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

    /// The [`density`](Self::density) parameter as an `f64`, if there is one.
    ///
    /// # Panics
    ///
    /// If it isn't hex `f64` bits.
    pub fn density_f64(&self) -> Option<f64> {
        let bits = self.density.as_ref()?;
        Some(f64::from_bits(
            u64::from_str_radix(bits, 16).expect("hex bits"),
        ))
    }
}

/// The probe file with this name (`rng.json`, `noise.json`) for [`crate::version`], in
/// [`crate::out_dir`].
///
/// # Panics
///
/// If it hasn't been generated.
pub fn probe_file(name: &str) -> PathBuf {
    let path = crate::out_dir().join("probe").join(name);
    assert!(
        path.is_file(),
        "no {}; run cargo xtask oracle probe",
        path.display()
    );
    path
}
