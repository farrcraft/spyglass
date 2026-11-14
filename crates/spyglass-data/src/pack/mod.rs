//! Data packs: a folder or zip of `data/<namespace>/...` files over the bundle, as the game loads
//! them over its vanilla pack. See `docs/specs/data-packs.md` and ADR 0072.
//!
//! A pack is untrusted input, as a save is: reading is capped ([`MAX_ENTRIES`], [`MAX_FILE`],
//! [`MAX_TOTAL`]), only files under `data/` (at the root or in an overlay folder) are read, and
//! no path can leave the pack. This crate reads no archives (ADR 0016): [`PackReader`] takes a
//! pack's files one at a time, and `spyglass_world::open_pack` feeds it a folder's or a zip's.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::bundle::sha1_hex_of;

mod meta;
mod read;

use meta::parse_meta;
pub use read::PackReader;

/// The most entries a pack's zip may have, or files its folder may hold under the folders read.
pub const MAX_ENTRIES: usize = 200_000;

/// The largest file read from a pack, after decompression: 32 MiB, as for one chunk of a save.
pub const MAX_FILE: u64 = 32 << 20;

/// The most bytes read from one pack, after decompression.
pub const MAX_TOTAL: u64 = 512 << 20;

/// The metadata file at a pack's root.
pub const MCMETA: &str = "pack.mcmeta";

/// Why a pack couldn't be read.
#[derive(Debug, thiserror::Error)]
pub enum PackError {
    /// Reading the folder or zip failed.
    #[error("I/O error on {path}: {source}")]
    Io {
        /// The file or folder.
        path: PathBuf,
        /// The underlying error.
        source: io::Error,
    },
    /// The zip isn't one.
    #[error("{path} isn't a readable zip: {message}")]
    Zip {
        /// The zip.
        path: PathBuf,
        /// What the zip reader said.
        message: String,
    },
    /// The pack is larger than spyglass reads, or has a path that would leave it.
    #[error("{path}: {message}")]
    Limit {
        /// The pack.
        path: PathBuf,
        /// Which limit, or which path.
        message: String,
    },
    /// `pack.mcmeta` is missing or isn't what the game reads.
    #[error("{path}: pack.mcmeta {message}")]
    Meta {
        /// The pack.
        path: PathBuf,
        /// What's wrong with it.
        message: String,
    },
}

/// A data pack format: `major.minor`, as `PackFormat` has it. A plain number is `major.0`.
/// `bundle.json` writes it as `[major, minor]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(from = "[u32; 2]", into = "[u32; 2]")]
pub struct PackFormat {
    /// The major format, e.g. 101.
    pub major: u32,
    /// The minor format, 0 for every format before they had one.
    pub minor: u32,
}

impl From<[u32; 2]> for PackFormat {
    fn from([major, minor]: [u32; 2]) -> Self {
        PackFormat { major, minor }
    }
}

impl From<PackFormat> for [u32; 2] {
    fn from(format: PackFormat) -> Self {
        [format.major, format.minor]
    }
}

impl std::fmt::Display for PackFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

/// The formats a pack, or one of its overlays, says it supports: both ends included.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormatRange {
    /// The oldest format.
    pub min: PackFormat,
    /// The newest format.
    pub max: PackFormat,
}

impl FormatRange {
    /// Whether `format` is in the range, ends included (`InclusiveRange.isValueInRange`).
    pub fn contains(&self, format: PackFormat) -> bool {
        self.min <= format && format <= self.max
    }
}

/// One `overlays.entries` item: a folder applied over the pack's root for the formats it gives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Overlay {
    /// The folder, at the pack's root.
    pub directory: String,
    /// The formats it applies for.
    pub formats: FormatRange,
}

/// One `filter.block` item: a file of a lower pack is hidden if both patterns match its id
/// (`IdentifierPattern`); an absent pattern matches every id.
#[derive(Debug, Clone)]
pub struct BlockedPattern {
    /// The pattern on the namespace, e.g. `minecraft`.
    namespace: Option<regex_lite::Regex>,
    /// The pattern on the path in the namespace, e.g. `worldgen/biome/.*`.
    path: Option<regex_lite::Regex>,
}

impl BlockedPattern {
    /// Whether the file `data/<namespace>/<path>` is hidden.
    pub fn blocks(&self, namespace: &str, path: &str) -> bool {
        self.namespace
            .as_ref()
            .is_none_or(|re| re.is_match(namespace))
            && self.path.as_ref().is_none_or(|re| re.is_match(path))
    }
}

/// A pack's `pack.mcmeta`, as far as loading its data needs.
#[derive(Debug, Clone)]
pub struct PackMeta {
    /// The pack's description, as plain text if it's a string, else its JSON.
    pub description: String,
    /// The formats it supports, or `None` if its `pack` section doesn't say validly: the game
    /// loads such a pack anyway, of unknown compatibility (`Pack.readPackMetadata`'s fallback).
    pub formats: Option<FormatRange>,
    /// Why `formats` is `None`.
    pub formats_error: Option<String>,
    /// Its `filter.block` patterns.
    pub filter: Vec<BlockedPattern>,
    /// Its overlays, in the order listed.
    pub overlays: Vec<Overlay>,
}

/// A data pack, read: its metadata and every file the game could load from it.
#[derive(Debug, Clone)]
pub struct Pack {
    /// Where it was read from.
    path: PathBuf,
    /// A name for it: the folder's or zip's file name.
    name: String,
    /// Its `pack.mcmeta`.
    meta: PackMeta,
    /// Every file under `data/` at the root or in an overlay folder the metadata names, by its
    /// `/`-separated path in the pack.
    files: BTreeMap<String, Arc<[u8]>>,
    /// SHA-1 of every file read, `pack.mcmeta` included, for telling packs apart.
    hash: String,
}

/// A pack applied for one data pack format: its files with the overlays that apply laid over the
/// root's, by their path from `data/` on, and its filter.
#[derive(Debug, Clone)]
pub struct Layer {
    /// The pack's name.
    pub name: String,
    /// The pack's hash.
    pub hash: String,
    /// `data/<namespace>/<path>` → contents.
    pub files: BTreeMap<String, Arc<[u8]>>,
    /// What it hides of the packs below it.
    pub filter: Arc<[BlockedPattern]>,
}

impl Layer {
    /// Whether this layer hides the file at `path` (`data/<namespace>/<path>`) in lower layers.
    pub fn hides(&self, path: &str) -> bool {
        let Some((namespace, rest)) = path.strip_prefix("data/").and_then(|p| p.split_once('/'))
        else {
            return false;
        };
        self.filter.iter().any(|b| b.blocks(namespace, rest))
    }
}

impl Pack {
    /// A pack from files in memory, by their `/`-separated paths in the pack, `pack.mcmeta`
    /// included: for tests, and packs built by other means. The same limits apply.
    pub fn from_files(
        name: impl Into<String>,
        files: impl IntoIterator<Item = (String, Vec<u8>)>,
    ) -> Result<Pack, PackError> {
        let name = name.into();
        let mut reader = PackReader::new(&name);
        for (file, bytes) in files {
            reader.entry()?;
            reader.add(&file, bytes.len() as u64, &bytes[..])?;
        }
        reader.finish(name)
    }

    /// Parses the metadata, then keeps the files under `data/` at the root and in its overlays.
    fn from_read(path: PathBuf, name: String, mut read: Contents) -> Result<Pack, PackError> {
        let meta_bytes = read.mcmeta.take().ok_or_else(|| PackError::Meta {
            path: path.clone(),
            message: "is missing".into(),
        })?;
        let meta = parse_meta(&meta_bytes).map_err(|message| PackError::Meta {
            path: path.clone(),
            message,
        })?;
        let roots: Vec<String> = std::iter::once(String::new())
            .chain(meta.overlays.iter().map(|o| format!("{}/", o.directory)))
            .collect();
        let files: BTreeMap<String, Arc<[u8]>> = read
            .files
            .into_iter()
            .filter(|(file, _)| {
                roots.iter().any(|root| {
                    file.strip_prefix(root.as_str())
                        .is_some_and(|f| f.starts_with("data/"))
                })
            })
            .map(|(file, bytes)| (file, Arc::from(bytes)))
            .collect();
        let mut listing = format!("{MCMETA} {}\n", sha1_hex_of(&meta_bytes));
        for (file, bytes) in &files {
            listing.push_str(&format!("{file} {}\n", sha1_hex_of(bytes)));
        }
        let hash = sha1_hex_of(listing.as_bytes());
        Ok(Pack {
            path,
            name,
            meta,
            files,
            hash,
        })
    }

    /// Where the pack was read from.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The pack's name: its folder's or zip's file name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Renames the pack, e.g. to the id a save enables it by (`file/<name>`).
    pub fn with_name(mut self, name: impl Into<String>) -> Pack {
        self.name = name.into();
        self
    }

    /// Its `pack.mcmeta`.
    pub fn meta(&self) -> &PackMeta {
        &self.meta
    }

    /// SHA-1 over every file read, lower-case hex: two packs with the same files have the same
    /// hash.
    pub fn hash(&self) -> &str {
        &self.hash
    }

    /// Every file kept, by its path in the pack (overlay folders included).
    pub fn files(&self) -> impl Iterator<Item = &str> {
        self.files.keys().map(String::as_str)
    }

    /// How many of its files are worldgen data: under `data/<namespace>/worldgen/` or
    /// `data/<namespace>/tags/worldgen/`, at the root or in an overlay folder.
    pub fn worldgen_files(&self) -> usize {
        self.files().filter(|path| is_worldgen(path)).count()
    }

    /// The pack applied for `format` (`Pack.ResourcesSupplier.openFull` with the overlays
    /// `OverlayMetadataSection.overlaysForVersion` picks): the root's `data/` files, then each
    /// applying overlay's over them, later ones over earlier.
    pub fn layer(&self, format: PackFormat) -> Layer {
        let mut files: BTreeMap<String, Arc<[u8]>> = self
            .files
            .iter()
            .filter(|(path, _)| path.starts_with("data/"))
            .map(|(path, bytes)| (path.clone(), bytes.clone()))
            .collect();
        for overlay in self.overlays_for(format) {
            let prefix = format!("{}/", overlay.directory);
            for (path, bytes) in &self.files {
                if let Some(rest) = path.strip_prefix(prefix.as_str()) {
                    files.insert(rest.to_string(), bytes.clone());
                }
            }
        }
        Layer {
            name: self.name.clone(),
            hash: self.hash.clone(),
            files,
            filter: self.meta.filter.clone().into(),
        }
    }

    /// The overlays that apply for `format`, in the order they're laid on.
    pub fn overlays_for(&self, format: PackFormat) -> impl Iterator<Item = &Overlay> {
        self.meta
            .overlays
            .iter()
            .filter(move |o| o.formats.contains(format))
    }

    /// Whether the pack says it supports `format`. The game loads one that doesn't anyway when a
    /// world enables it, so this is for warnings only.
    pub fn supports(&self, format: PackFormat) -> bool {
        self.meta.formats.is_some_and(|f| f.contains(format))
    }
}

/// Packs are equal when they were read from the same place, have the same name and hold the
/// same files.
impl PartialEq for Pack {
    fn eq(&self, other: &Self) -> bool {
        (&self.path, &self.name, &self.hash) == (&other.path, &other.name, &other.hash)
    }
}

impl Eq for Pack {}

/// Whether a path in a pack is worldgen data: `data/<ns>/worldgen/...` or
/// `data/<ns>/tags/worldgen/...`, at the root or in an overlay folder.
pub fn is_worldgen(path: &str) -> bool {
    let parts: Vec<&str> = path.split('/').collect();
    parts.windows(3).enumerate().any(|(i, w)| {
        w[0] == "data"
            && (w[2] == "worldgen" || (w[2] == "tags" && parts.get(i + 3) == Some(&"worldgen")))
    })
}

/// Files read from a folder or zip: `pack.mcmeta` apart, and the rest by path.
#[derive(Default)]
pub(crate) struct Contents {
    mcmeta: Option<Vec<u8>>,
    files: BTreeMap<String, Vec<u8>>,
}

impl Contents {
    pub(crate) fn insert(&mut self, path: String, bytes: Vec<u8>) {
        if path == MCMETA {
            self.mcmeta = Some(bytes);
        } else {
            self.files.insert(path, bytes);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const V26_3: PackFormat = PackFormat {
        major: 121,
        minor: 0,
    };

    fn pack(files: &[(&str, &str)]) -> Pack {
        Pack::from_files(
            "test",
            files
                .iter()
                .map(|(p, text)| (p.to_string(), text.as_bytes().to_vec())),
        )
        .unwrap()
    }

    #[test]
    fn overlays_apply_by_format_the_last_listed_on_top() {
        let pack = pack(&[
            (
                "pack.mcmeta",
                r#"{"pack": {"description": {"text": "x"}, "min_format": 121, "max_format": 127},
                    "overlays": {"entries": [
                        {"directory": "old", "min_format": 100, "max_format": 120},
                        {"directory": "a", "min_format": 121, "max_format": 127},
                        {"directory": "b", "min_format": [121, 0], "max_format": [121, 0]},
                        {"directory": "skipped"}]}}"#,
            ),
            ("data/x/worldgen/noise/n.json", "root"),
            ("data/x/worldgen/noise/m.json", "root"),
            ("data/x/worldgen/noise/o.json", "root"),
            ("a/data/x/worldgen/noise/n.json", "a"),
            ("a/data/x/worldgen/noise/m.json", "a"),
            ("b/data/x/worldgen/noise/n.json", "b"),
            ("old/data/x/worldgen/noise/o.json", "old"),
            ("assets/x/textures/t.png", "not read"),
            ("c/data/x/worldgen/noise/n.json", "not an overlay"),
        ]);
        assert!(pack.supports(V26_3));
        assert_eq!(pack.meta().description, r#"{"text":"x"}"#);
        let layer = pack.layer(V26_3);
        let text = |p: &str| String::from_utf8(layer.files[p].to_vec()).unwrap();
        assert_eq!(text("data/x/worldgen/noise/n.json"), "b");
        assert_eq!(text("data/x/worldgen/noise/m.json"), "a");
        assert_eq!(text("data/x/worldgen/noise/o.json"), "root");
        assert_eq!(layer.files.len(), 3);
        let later = pack.layer(PackFormat::from([122, 0]));
        assert_eq!(&*later.files["data/x/worldgen/noise/n.json"], b"a");
        assert!(
            !pack
                .files()
                .any(|f| f.starts_with("assets/") || f.starts_with("c/"))
        );
    }

    #[test]
    fn filters_match_anywhere_in_the_id() {
        let pack = pack(&[(
            "pack.mcmeta",
            r#"{"pack": {"description": "", "min_format": 121, "max_format": 121},
                "filter": {"block": [{"namespace": "minecraft", "path": "worldgen/biome/"},
                                     {"path": "^tags/"}]}}"#,
        )]);
        let layer = pack.layer(V26_3);
        assert!(layer.hides("data/minecraft/worldgen/biome/plains.json"));
        // `asPredicate`: a find, not a whole match, so `minecraft` matches `notminecraft`.
        assert!(layer.hides("data/notminecraft/worldgen/biome/x.json"));
        assert!(!layer.hides("data/minecraft/worldgen/noise/x.json"));
        assert!(layer.hides("data/any/tags/block/logs.json"));
        assert!(!layer.hides("reports/blocks.json"));
    }
}
