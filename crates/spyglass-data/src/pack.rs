//! Data packs: a folder or zip of `data/<namespace>/...` files over the bundle, as the game loads
//! them over its vanilla pack. See `docs/specs/data-packs.md` and ADR 0072.
//!
//! A pack is untrusted input, as a save is: reading is capped ([`MAX_ENTRIES`], [`MAX_FILE`],
//! [`MAX_TOTAL`]), only files under `data/` (at the root or in an overlay folder) are read, and
//! no path can leave the pack. This crate reads no archives (ADR 0016): [`PackReader`] takes a
//! pack's files one at a time, and `spyglass_world::open_pack` feeds it a folder's or a zip's.

use std::collections::BTreeMap;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::bundle::sha1_hex;

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
        let mut listing = format!(
            "{MCMETA} {}\n",
            sha1_hex(&meta_bytes[..]).unwrap_or_default()
        );
        for (file, bytes) in &files {
            listing.push_str(&format!(
                "{file} {}\n",
                sha1_hex(&bytes[..]).unwrap_or_default()
            ));
        }
        let hash = sha1_hex(listing.as_bytes()).unwrap_or_default();
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
struct Contents {
    mcmeta: Option<Vec<u8>>,
    files: BTreeMap<String, Vec<u8>>,
}

impl Contents {
    fn insert(&mut self, path: String, bytes: Vec<u8>) {
        if path == MCMETA {
            self.mcmeta = Some(bytes);
        } else {
            self.files.insert(path, bytes);
        }
    }
}

/// Reads a pack's files one at a time, under the limits, into a [`Pack`]: for readers of
/// folders and zips, which call [`entry`](Self::entry) for each entry they see and
/// [`add`](Self::add) for each [`wanted`](Self::wanted) file.
pub struct PackReader {
    /// The limits, and what's been read so far.
    budget: Budget,
    /// What's been read.
    contents: Contents,
}

impl PackReader {
    /// A reader for the pack at `path`, which names it in errors.
    pub fn new(path: impl AsRef<Path>) -> Self {
        PackReader {
            budget: Budget::new(path.as_ref()),
            contents: Contents::default(),
        }
    }

    /// The pack's path, as given.
    pub fn path(&self) -> &Path {
        &self.budget.pack
    }

    /// Whether a path in a pack is one spyglass reads: `pack.mcmeta`, or a file in a `data/`
    /// folder at the root or one level down (where overlay folders are). Readers skip the rest
    /// unread.
    pub fn wanted(path: &str) -> bool {
        wanted(path)
    }

    /// Counts one entry seen, read or not: more than [`MAX_ENTRIES`] is an error.
    pub fn entry(&mut self) -> Result<(), PackError> {
        self.budget.entry()
    }

    /// Reads the file at `path` in the pack, whose size is said to be `size`: a path that would
    /// leave the pack, a file larger than [`MAX_FILE`] (as said or as read) or a pack past
    /// [`MAX_TOTAL`] is an error.
    pub fn add(&mut self, path: &str, size: u64, reader: impl Read) -> Result<(), PackError> {
        check_path(&self.budget.pack, path)?;
        if size > MAX_FILE {
            return Err(self
                .budget
                .limit(format!("{path} is larger than {MAX_FILE} bytes")));
        }
        let bytes = self.budget.read(path, reader)?;
        self.contents.insert(path.to_string(), bytes);
        Ok(())
    }

    /// An error about the pack: an I/O error on `file`, or its archive.
    pub fn io_error(&self, file: impl AsRef<Path>, source: io::Error) -> PackError {
        PackError::Io {
            path: self.budget.pack.join(file),
            source,
        }
    }

    /// An error saying the pack is past a limit, e.g. has too many entries.
    pub fn limit(&self, message: impl Into<String>) -> PackError {
        self.budget.limit(message.into())
    }

    /// An error saying the pack's archive can't be read.
    pub fn archive_error(&self, message: impl Into<String>) -> PackError {
        PackError::Zip {
            path: self.budget.pack.clone(),
            message: message.into(),
        }
    }

    /// The pack read, named `name`: its metadata parsed, and its files under `data/` at the root
    /// and in its overlays kept.
    pub fn finish(self, name: impl Into<String>) -> Result<Pack, PackError> {
        Pack::from_read(self.budget.pack, name.into(), self.contents)
    }
}

/// Whether a path in a pack is one spyglass reads ([`PackReader::wanted`]).
fn wanted(path: &str) -> bool {
    if path == MCMETA {
        return true;
    }
    let mut parts = path.split('/');
    match (parts.next(), parts.next()) {
        (Some("data"), Some(_)) => true,
        (Some(_), Some("data")) => parts.next().is_some(),
        _ => false,
    }
}

/// Refuses a path that's empty, absolute, has a `\`, or has an empty, `.` or `..` segment.
fn check_path(pack: &Path, path: &str) -> Result<(), PackError> {
    let ok = !path.is_empty()
        && !path.contains('\\')
        && !path.contains(':')
        && path
            .split('/')
            .all(|seg| !seg.is_empty() && seg != "." && seg != "..");
    if ok {
        Ok(())
    } else {
        Err(PackError::Limit {
            path: pack.to_path_buf(),
            message: format!("{path:?} isn't a path inside the pack"),
        })
    }
}

/// The limits on one pack's reading, and what's been read so far.
struct Budget {
    pack: PathBuf,
    entries: usize,
    total: u64,
}

impl Budget {
    fn new(pack: &Path) -> Self {
        Budget {
            pack: pack.to_path_buf(),
            entries: 0,
            total: 0,
        }
    }

    fn limit(&self, message: String) -> PackError {
        PackError::Limit {
            path: self.pack.clone(),
            message,
        }
    }

    /// Counts one entry seen, read or not.
    fn entry(&mut self) -> Result<(), PackError> {
        self.entries += 1;
        if self.entries > MAX_ENTRIES {
            return Err(self.limit(format!("more than {MAX_ENTRIES} entries")));
        }
        Ok(())
    }

    /// Counts `len` bytes read for `file`.
    fn take(&mut self, file: &str, len: u64) -> Result<(), PackError> {
        if len > MAX_FILE {
            return Err(self.limit(format!("{file} is larger than {MAX_FILE} bytes")));
        }
        self.total += len;
        if self.total > MAX_TOTAL {
            return Err(self.limit(format!("more than {MAX_TOTAL} bytes of data")));
        }
        Ok(())
    }

    /// Reads at most [`MAX_FILE`] bytes of `reader`, counting them.
    fn read(&mut self, file: &str, reader: impl Read) -> Result<Vec<u8>, PackError> {
        let mut bytes = Vec::new();
        reader
            .take(MAX_FILE + 1)
            .read_to_end(&mut bytes)
            .map_err(|source| PackError::Io {
                path: self.pack.join(file),
                source,
            })?;
        self.take(file, bytes.len() as u64)?;
        Ok(bytes)
    }
}

/// The newest data pack format without a minor part (`PackFormat.lastPreMinorVersion` for
/// `SERVER_DATA`): packs and overlays that reach it or older must also give the old fields.
const LAST_PRE_MINOR: u32 = 81;

/// Parses `pack.mcmeta` as `Pack.readPackMetadata` reads it. An error is a pack the game skips:
/// not JSON, no `pack` section or description, or broken `overlays` or `filter`. A `pack`
/// section whose formats don't validate isn't an error: the pack loads, of unknown format.
fn parse_meta(bytes: &[u8]) -> Result<PackMeta, String> {
    let json: Value = serde_json::from_slice(bytes).map_err(|e| format!("isn't JSON: {e}"))?;
    let pack = json
        .get("pack")
        .and_then(Value::as_object)
        .ok_or("has no \"pack\" section")?;
    let description = match pack.get("description") {
        Some(Value::String(text)) => text.clone(),
        Some(other) => other.to_string(),
        None => return Err("has no pack description".into()),
    };
    let (formats, formats_error) = match FormatFields::read(pack, "supported_formats")
        .and_then(|f| f.validate("Pack", "supported_formats", true, false))
    {
        Ok(range) => (Some(range), None),
        Err(e) => (None, Some(e)),
    };
    let overlays = match json.get("overlays") {
        None => Vec::new(),
        Some(section) => overlays(section).map_err(|e| format!("overlays: {e}"))?,
    };
    let filter = match json.get("filter") {
        None => Vec::new(),
        Some(section) => filter(section).map_err(|e| format!("filter: {e}"))?,
    };
    Ok(PackMeta {
        description,
        formats,
        formats_error,
        filter,
        overlays,
    })
}

/// The format fields of a pack section or an overlay entry (`IntermediaryFormat`), decoded but
/// not yet validated.
struct FormatFields {
    /// `min_format`.
    min: Option<PackFormat>,
    /// `max_format`.
    max: Option<PackFormat>,
    /// `pack_format`, read only for a pack section.
    pack_format: Option<u32>,
    /// `supported_formats`, or an overlay's `formats`: the range in major formats only.
    old: Option<(u32, u32)>,
}

impl FormatFields {
    fn read(object: &serde_json::Map<String, Value>, old_key: &str) -> Result<Self, String> {
        Ok(FormatFields {
            min: object
                .get("min_format")
                .map(|v| format_value(v, 0))
                .transpose()?,
            max: object
                .get("max_format")
                .map(|v| format_value(v, i32::MAX.unsigned_abs()))
                .transpose()?,
            pack_format: object
                .get("pack_format")
                .map(|v| non_negative(v).ok_or("pack_format isn't a non-negative int"))
                .transpose()?,
            old: object.get(old_key).map(int_range).transpose()?,
        })
    }

    fn is_empty(&self) -> bool {
        self.min.is_none() && self.max.is_none() && self.old.is_none()
    }

    /// `IntermediaryFormat.validate`: which fields a pack or overlay must give, and the range
    /// they make. `has_pack_format` says whether `pack_format` is read (packs, not overlays);
    /// `require_old` whether the old range field must be present (overlays, when any entry
    /// reaches format 81 or older).
    fn validate(
        &self,
        what: &str,
        old_key: &str,
        has_pack_format: bool,
        require_old: bool,
    ) -> Result<FormatRange, String> {
        let check_pack_format = |min: u32, max: u32| -> Result<(), String> {
            if !has_pack_format {
                return Ok(());
            }
            let Some(main) = self.pack_format else {
                return Err(format!(
                    "{what} declares formats {min} to {max} and so requires a pack_format field"
                ));
            };
            if main < min || main > max {
                return Err(format!(
                    "{what} declared support for versions {min} to {max} but declared main format is {main}"
                ));
            }
            if min < 15 && min != max {
                return Err(format!(
                    "Multi-version packs cannot support minimum version of less than 15 ({what} declares {min})"
                ));
            }
            Ok(())
        };
        let newer_without_fields = || {
            format!(
                "{what} declares support for version newer than {LAST_PRE_MINOR}, but is missing mandatory fields min_format and max_format"
            )
        };
        match (self.min, self.max) {
            (Some(_), None) | (None, Some(_)) => Err(format!(
                "{what} missing field, must declare both min_format and max_format"
            )),
            (Some(min), Some(max)) => {
                if min > max {
                    return Err(format!(
                        "{what} min_format ({min}) is greater than max_format ({max})"
                    ));
                }
                if min.major > LAST_PRE_MINOR {
                    if require_old && self.old.is_none() {
                        return Err(format!("{what} missing required field {old_key}"));
                    }
                    if self.old.is_some() && !require_old {
                        return Err(format!(
                            "{what} key {old_key} is deprecated starting from pack format {}",
                            LAST_PRE_MINOR + 1
                        ));
                    }
                    if self.pack_format.is_some() {
                        check_pack_format(min.major, max.major)?;
                    }
                } else {
                    let Some((old_min, old_max)) = self.old else {
                        return Err(format!(
                            "{what} declares format {} and so requires a {old_key} field",
                            min.major
                        ));
                    };
                    if old_min != min.major || (old_max != max.major && old_max != LAST_PRE_MINOR) {
                        return Err(format!(
                            "{what} version declaration mismatch between {old_key} and min_format/max_format"
                        ));
                    }
                    check_pack_format(min.major, max.major)?;
                }
                Ok(FormatRange { min, max })
            }
            (None, None) => match (self.old, has_pack_format, self.pack_format) {
                (Some((_, max)), ..) if max > LAST_PRE_MINOR => Err(newer_without_fields()),
                (Some((min, max)), ..) => {
                    check_pack_format(min, max)?;
                    Ok(FormatRange {
                        min: PackFormat::from([min, 0]),
                        max: PackFormat::from([max, 0]),
                    })
                }
                (None, true, Some(f)) if f > LAST_PRE_MINOR => Err(newer_without_fields()),
                (None, true, Some(f)) => Ok(FormatRange {
                    min: PackFormat::from([f, 0]),
                    max: PackFormat::from([f, 0]),
                }),
                _ => Err(format!(
                    "{what} could not be parsed, missing format version information"
                )),
            },
        }
    }
}

/// `min_format` or `max_format` (`PackFormat.fullCodec`): `N` or `[N]` is `N.<minor>`, with
/// `minor` 0 for a minimum and the largest for a maximum; `[N, M]` is `N.M`, and later elements
/// are ignored.
fn format_value(value: &Value, minor: u32) -> Result<PackFormat, String> {
    let bad = || format!("{value} isn't a pack format");
    match value {
        Value::Array(parts) => {
            if parts.is_empty() || parts.len() > 256 {
                return Err(bad());
            }
            let parts = parts
                .iter()
                .map(|v| non_negative(v).ok_or_else(bad))
                .collect::<Result<Vec<u32>, _>>()?;
            Ok(PackFormat {
                major: parts[0],
                minor: parts.get(1).copied().unwrap_or(minor),
            })
        }
        other => Ok(PackFormat {
            major: non_negative(other).ok_or_else(bad)?,
            minor,
        }),
    }
}

/// An int range as `InclusiveRange.codec(INT)` reads it: `N`, `[min, max]` or
/// `{"min_inclusive": min, "max_inclusive": max}`, with `min <= max`. Negative ends are clamped
/// to 0, below every format.
fn int_range(value: &Value) -> Result<(u32, u32), String> {
    let (min, max) = match value {
        Value::Array(ends) if ends.len() == 2 => (int(&ends[0]), int(&ends[1])),
        Value::Object(ends) => (
            ends.get("min_inclusive").and_then(int),
            ends.get("max_inclusive").and_then(int),
        ),
        other => (int(other), int(other)),
    };
    let (Some(min), Some(max)) = (min, max) else {
        return Err(format!("{value} isn't a format range"));
    };
    if min > max {
        return Err(format!(
            "range {min} to {max} has its ends the wrong way round"
        ));
    }
    Ok((min.max(0).unsigned_abs(), max.max(0).unsigned_abs()))
}

/// A JSON int in `i32`, as the game's `Codec.INT` reads it.
fn int(value: &Value) -> Option<i32> {
    value.as_i64().and_then(|v| i32::try_from(v).ok())
}

/// A JSON int in `0..=i32::MAX` (`ExtraCodecs.NON_NEGATIVE_INT`).
fn non_negative(value: &Value) -> Option<u32> {
    int(value).and_then(|v| u32::try_from(v).ok())
}

/// `overlays.entries` (`OverlayMetadataSection`): each a `directory` matching
/// `[-_a-zA-Z0-9.]+` and its formats, validated together (`PackFormat.validateHolderList`). An
/// entry without any format field is skipped, as the game skips it; any other bad entry fails
/// the list, and so the pack.
fn overlays(section: &Value) -> Result<Vec<Overlay>, String> {
    let entries = section
        .get("entries")
        .and_then(Value::as_array)
        .ok_or("has no \"entries\" list")?;
    let mut read = Vec::new();
    for entry in entries {
        let object = entry.as_object().ok_or("an entry isn't an object")?;
        let directory = object
            .get("directory")
            .and_then(Value::as_str)
            .ok_or("an entry has no directory")?;
        let valid = !directory.is_empty()
            && directory
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c));
        if !valid {
            return Err(format!("{directory} is not accepted directory name"));
        }
        let fields = FormatFields::read(object, "formats")?;
        if !fields.is_empty() {
            read.push((directory.to_string(), fields));
        }
    }
    // The oldest major format any entry reaches: at 81 or below, every entry needs `formats`.
    let min_major = read
        .iter()
        .filter_map(|(_, f)| f.min.map(|m| m.major).or(f.old.map(|o| o.0)))
        .min();
    let require_old = min_major.is_some_and(|m| m <= LAST_PRE_MINOR);
    read.into_iter()
        .map(|(directory, fields)| {
            let what = format!("Overlay \"{directory}\"");
            let formats = fields.validate(&what, "formats", false, require_old)?;
            Ok(Overlay { directory, formats })
        })
        .collect()
}

/// `filter.block` (`ResourceFilterSection`): a list of `{"namespace": regex, "path": regex}`,
/// either optional. The game's patterns are Java's; one `regex_lite` can't read is an error,
/// since spyglass couldn't apply the pack as the game does.
fn filter(section: &Value) -> Result<Vec<BlockedPattern>, String> {
    let block = section
        .get("block")
        .and_then(Value::as_array)
        .ok_or("has no \"block\" list")?;
    let pattern = |entry: &Value, key: &str| -> Result<Option<regex_lite::Regex>, String> {
        match entry.get(key) {
            None => Ok(None),
            Some(Value::String(re)) => regex_lite::Regex::new(re)
                .map(Some)
                .map_err(|e| format!("pattern {re:?} can't be read: {e}")),
            Some(_) => Err(format!("{key} isn't a string")),
        }
    };
    block
        .iter()
        .map(|entry| {
            if !entry.is_object() {
                return Err("an entry isn't an object".to_string());
            }
            Ok(BlockedPattern {
                namespace: pattern(entry, "namespace")?,
                path: pattern(entry, "path")?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {

    use super::*;

    const V26_3: PackFormat = PackFormat {
        major: 121,
        minor: 0,
    };

    fn meta(json: &str) -> Result<PackMeta, String> {
        parse_meta(json.as_bytes())
    }

    fn formats(pack: &str) -> Option<FormatRange> {
        meta(&format!(r#"{{"pack": {{"description": "", {pack}}}}}"#))
            .unwrap()
            .formats
    }

    fn range(min: [u32; 2], max: [u32; 2]) -> Option<FormatRange> {
        Some(FormatRange {
            min: min.into(),
            max: max.into(),
        })
    }

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
    fn format_fields_as_the_game_reads_them() {
        let max = i32::MAX.unsigned_abs();
        assert_eq!(
            formats(r#""min_format": 121, "max_format": 121"#),
            range([121, 0], [121, max])
        );
        assert_eq!(
            formats(r#""min_format": [120, 3], "max_format": [121, 1, 9]"#),
            range([120, 3], [121, 1])
        );
        assert_eq!(formats(r#""pack_format": 48"#), range([48, 0], [48, 0]));
        assert_eq!(
            formats(r#""pack_format": 48, "supported_formats": [48, 57]"#),
            range([48, 0], [57, 0])
        );
        assert_eq!(
            formats(
                r#""pack_format": 71, "supported_formats": {"min_inclusive": 71, "max_inclusive": 81},
                "min_format": 71, "max_format": 121"#
            ),
            range([71, 0], [121, max])
        );
        // Invalid: the pack still loads, of unknown format.
        for bad in [
            r#""min_format": 121"#,
            r#""min_format": 122, "max_format": 121"#,
            r#""pack_format": 121"#,
            r#""supported_formats": [48, 121], "pack_format": 48"#,
            r#""min_format": 121, "max_format": 121, "supported_formats": [121, 121]"#,
            r#""min_format": 71, "max_format": 121"#,
            r#""pack_format": 60, "supported_formats": [48, 57]"#,
            r#""pack_format": 4, "supported_formats": [4, 10]"#,
            r#""min_format": -1, "max_format": 121"#,
            r#""note": 1"#,
        ] {
            let read = meta(&format!(r#"{{"pack": {{"description": "", {bad}}}}}"#)).unwrap();
            assert!(read.formats.is_none(), "{bad}");
            assert!(read.formats_error.is_some(), "{bad}");
        }
    }

    #[test]
    fn packs_the_game_skips() {
        for bad in [
            "not json",
            "{}",
            r#"{"pack": {"min_format": 121, "max_format": 121}}"#,
            r#"{"pack": {"description": ""}, "overlays": {}}"#,
            r#"{"pack": {"description": ""}, "overlays": {"entries": [{"directory": "a/b", "formats": 1}]}}"#,
            r#"{"pack": {"description": ""}, "overlays": {"entries": [{"directory": "a", "min_format": 121}]}}"#,
            r#"{"pack": {"description": ""}, "filter": {"block": [{"path": "("}]}}"#,
        ] {
            assert!(meta(bad).is_err(), "{bad}");
        }
        assert!(matches!(
            Pack::from_files("x", [("data/a/b.json".to_string(), b"{}".to_vec())]),
            Err(PackError::Meta { .. })
        ));
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
