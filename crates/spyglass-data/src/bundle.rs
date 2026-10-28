//! Version bundles: the game's worldgen JSON and data-generator reports, copied byte for byte
//! with game paths, plus a `bundle.json` manifest of SHA-1 hashes. See
//! `docs/specs/bundle-format.md` and ADR 0006.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};

/// The bundle format this build reads, and `spyglass-extract` writes.
pub const FORMAT: u32 = 1;

/// The manifest's file name, at the bundle root.
pub const MANIFEST: &str = "bundle.json";

/// The environment variable that names the bundles folder, ahead of every other default.
pub const BUNDLES_ENV: &str = "SPYGLASS_BUNDLES";

/// The bundles folder to use when none is given (ADR 0039, `docs/specs/bundle-format.md`):
/// [`BUNDLES_ENV`] if it's set, else `bundles` in the working directory if that exists, else
/// `bundles` in the per-user data folder. Without a per-user folder (no home directory), it's
/// `bundles` in the working directory.
pub fn default_bundles_dir() -> PathBuf {
    choose_bundles_dir(
        std::env::var_os(BUNDLES_ENV),
        Path::new("bundles").is_dir(),
        directories::ProjectDirs::from("", "", "spyglass")
            .map(|d| d.data_local_dir().to_path_buf()),
    )
}

/// [`default_bundles_dir`]'s choice, given what it found.
fn choose_bundles_dir(
    env: Option<OsString>,
    local_exists: bool,
    data_dir: Option<PathBuf>,
) -> PathBuf {
    if let Some(dir) = env.filter(|dir| !dir.is_empty()) {
        return dir.into();
    }
    match data_dir {
        Some(data) if !local_exists => data.join("bundles"),
        _ => PathBuf::from("bundles"),
    }
}

/// Why a bundle couldn't be read.
#[derive(Debug, thiserror::Error)]
pub enum BundleError {
    /// Reading a file failed.
    #[error("I/O error on {path}: {source}")]
    Io {
        /// The file.
        path: PathBuf,
        /// The underlying error.
        source: io::Error,
    },
    /// `bundle.json` or a resource isn't the JSON expected.
    #[error("malformed {path}: {source}")]
    Json {
        /// The file.
        path: PathBuf,
        /// The underlying error.
        source: serde_json::Error,
    },
    /// The bundle isn't laid out as this build expects, e.g. another format or no templates.
    #[error("{0}")]
    Layout(String),
    /// The bundle doesn't match its manifest.
    #[error("bundle {dir} is invalid:\n  {}", .problems.join("\n  "))]
    Invalid {
        /// The bundle directory.
        dir: PathBuf,
        /// Every problem found.
        problems: Vec<String>,
    },
    /// A resource id isn't a valid `namespace:path`.
    #[error("invalid resource id {0:?}")]
    BadId(String),
    /// The bundle has no such resource or report.
    #[error("{0} isn't in the bundle")]
    NotFound(String),
}

impl Manifest {
    /// The version's kind: as recorded, or [`Kind::from_id`] for bundles that don't record it.
    pub fn kind(&self) -> Kind {
        self.kind.unwrap_or_else(|| Kind::from_id(&self.version))
    }
}

fn io_err(path: &Path) -> impl FnOnce(io::Error) -> BundleError + '_ {
    move |source| BundleError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// Whether a game version is a release or a snapshot (ADR 0028). Pre-releases and release
/// candidates are snapshots, as the version manifest says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// A release, e.g. `26.3`.
    Release,
    /// A snapshot, pre-release or release candidate, e.g. `26.4-snapshot-5`.
    Snapshot,
}

impl Kind {
    /// The version manifest's `type`: `release` is a release, anything else (`snapshot`, and the
    /// old `old_alpha` and `old_beta`) isn't.
    pub fn from_manifest_type(kind: &str) -> Self {
        if kind == "release" {
            Kind::Release
        } else {
            Kind::Snapshot
        }
    }

    /// A guess from the version id alone, for bundles that don't record their kind: ids with
    /// `-snapshot-`, `-pre` or `-rc` are snapshots, as are the old `25w14a` form.
    pub fn from_id(id: &str) -> Self {
        let weekly = id.len() == 6
            && id.is_ascii()
            && id[..2].bytes().all(|b| b.is_ascii_digit())
            && id.as_bytes()[2] == b'w'
            && id[3..5].bytes().all(|b| b.is_ascii_digit());
        if weekly || ["-snapshot-", "-pre", "-rc"].iter().any(|m| id.contains(m)) {
            Kind::Snapshot
        } else {
            Kind::Release
        }
    }
}

/// `bundle.json`. Field order is the order written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// Bundle format version; [`FORMAT`] for bundles this build reads.
    pub format: u32,
    /// Game version id, e.g. `26.3`.
    pub version: String,
    /// `world_version` from the game jar's `version.json`, e.g. 5023.
    pub data_version: u32,
    /// Whether the version is a release or a snapshot, from the version manifest's `type`.
    /// Bundles written before it was recorded have none; [`Manifest::kind`] then goes by the id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<Kind>,
    /// SHA-1 of the downloaded server jar, lower-case hex.
    pub jar_sha1: String,
    /// `JAVA_VERSION` of the runtime that ran the data generator.
    pub java_version: String,
    /// The version of `spyglass-extract` that wrote the bundle.
    pub extractor: String,
    /// Every other file in the bundle: `/`-separated path → lower-case hex SHA-1, sorted by path.
    pub files: BTreeMap<String, String>,
}

/// Lower-case hex SHA-1 of everything `reader` yields: a file, or bytes (`&[u8]` reads too).
pub fn sha1_hex(mut reader: impl Read) -> io::Result<String> {
    let mut hasher = Sha1::new();
    let mut buf = vec![0; 1 << 16];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// An opened bundle, for reading its resources. Opening reads `bundle.json` and checks its format,
/// but doesn't rehash the files; [`verify`] does that.
#[derive(Debug, Clone)]
pub struct Bundle {
    dir: PathBuf,
    manifest: Manifest,
    /// Whether the manifest lists structure templates; older bundles have none.
    has_templates: bool,
}

impl Bundle {
    /// Opens the bundle in `dir`.
    pub fn open(dir: impl Into<PathBuf>) -> Result<Self, BundleError> {
        let dir = dir.into();
        let manifest = read_manifest(&dir)?;
        let has_templates = manifest
            .files
            .keys()
            .any(|p| p.starts_with("data/minecraft/structure/"));
        Ok(Bundle {
            dir,
            manifest,
            has_templates,
        })
    }

    /// The bundle directory.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The bundle's `bundle.json`.
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// Where a registry entry lives in the bundle, as the game's data packs lay it out:
    /// `data/<namespace>/<registry>/<path>.json`. `registry` is the registry's path, such as
    /// `worldgen/density_function` or `dimension_type`. An id without a namespace is in
    /// `minecraft`, as `Identifier.parse` reads it.
    pub fn resource_path(registry: &str, id: &str) -> Result<String, BundleError> {
        let (namespace, path) = id.split_once(':').unwrap_or(("minecraft", id));
        let valid = |s: &str, extra: &[char]| {
            !s.is_empty()
                && s.chars().all(|c| {
                    c.is_ascii_lowercase()
                        || c.is_ascii_digit()
                        || "_-.".contains(c)
                        || extra.contains(&c)
                })
        };
        // Identifier's rules, plus no empty, `.` or `..` path segments.
        let segments_ok = path
            .split('/')
            .all(|seg| !seg.is_empty() && seg != "." && seg != "..");
        if !valid(namespace, &[]) || !valid(path, &['/']) || !segments_ok {
            return Err(BundleError::BadId(id.to_string()));
        }
        Ok(format!("data/{namespace}/{registry}/{path}.json"))
    }

    /// The ids of every entry in a registry (such as `worldgen/structure`), from the manifest,
    /// sorted.
    pub fn registry_ids(&self, registry: &str) -> Vec<String> {
        let mut ids: Vec<String> = self
            .manifest
            .files
            .keys()
            .filter_map(|path| {
                let rest = path.strip_prefix("data/")?;
                let (namespace, rest) = rest.split_once('/')?;
                let name = rest.strip_prefix(registry)?.strip_prefix('/')?;
                Some(format!("{namespace}:{}", name.strip_suffix(".json")?))
            })
            .collect();
        ids.sort();
        ids
    }

    /// A registry entry's JSON, e.g. `resource("worldgen/density_function",
    /// "minecraft:overworld/continents")`.
    pub fn resource(&self, registry: &str, id: &str) -> Result<serde_json::Value, BundleError> {
        self.json(&Self::resource_path(registry, id)?)
    }

    /// A data-generator report's JSON, by its path in the bundle, e.g.
    /// `reports/biome_parameters/minecraft/overworld.json`.
    pub fn report(&self, path: &str) -> Result<serde_json::Value, BundleError> {
        self.json(path)
    }

    /// A structure template's bytes (gzip-compressed NBT, as in the jar), by id: e.g.
    /// `minecraft:igloo/top` is `data/minecraft/structure/igloo/top.nbt`. Bundles extracted
    /// before templates were included have none, and say so.
    pub fn structure_template(&self, id: &str) -> Result<Vec<u8>, BundleError> {
        let path = Self::resource_path("structure", id)?;
        let path = format!("{}.nbt", path.strip_suffix(".json").unwrap_or(&path));
        if !self.has_templates {
            return Err(BundleError::Layout(format!(
                "{} has no structure templates; extract it again with this version of spyglass",
                self.dir.display()
            )));
        }
        self.bytes(&path)
    }

    /// Any file the manifest lists, as bytes. Unlisted paths are refused.
    fn bytes(&self, path: &str) -> Result<Vec<u8>, BundleError> {
        if !self.manifest.files.contains_key(path) {
            return Err(BundleError::NotFound(path.to_string()));
        }
        let file = self.dir.join(path);
        fs::read(&file).map_err(io_err(&file))
    }

    /// Any file the manifest lists, parsed as JSON. Unlisted paths are refused, so nothing
    /// outside the bundle can be read.
    fn json(&self, path: &str) -> Result<serde_json::Value, BundleError> {
        if !self.manifest.files.contains_key(path) {
            return Err(BundleError::NotFound(path.to_string()));
        }
        let file = self.dir.join(path);
        let bytes = fs::read(&file).map_err(io_err(&file))?;
        serde_json::from_slice(&bytes).map_err(|source| BundleError::Json { path: file, source })
    }
}

/// Reads and parses a bundle's `bundle.json`, without checking any files.
pub fn read_manifest(dir: &Path) -> Result<Manifest, BundleError> {
    let path = dir.join(MANIFEST);
    let text = fs::read_to_string(&path).map_err(io_err(&path))?;
    let manifest: Manifest =
        serde_json::from_str(&text).map_err(|source| BundleError::Json { path, source })?;
    if manifest.format != FORMAT {
        return Err(BundleError::Layout(format!(
            "{} has bundle format {}; this build reads format {FORMAT}",
            dir.display(),
            manifest.format
        )));
    }
    Ok(manifest)
}

/// Checks a bundle against its manifest: every listed file exists with the listed SHA-1, and no
/// other file exists. Reports every problem, not only the first.
pub fn verify(dir: &Path) -> Result<Manifest, BundleError> {
    let manifest = read_manifest(dir)?;
    let mut problems = Vec::new();
    for (path, expected) in &manifest.files {
        let contained = Path::new(path)
            .components()
            .all(|c| matches!(c, Component::Normal(_)));
        if !contained || path.contains('\\') {
            problems.push(format!("{path}: not a relative `/`-separated path"));
            continue;
        }
        match fs::File::open(dir.join(path)).and_then(sha1_hex) {
            Ok(actual) => {
                if actual != *expected {
                    problems.push(format!("{path}: SHA-1 is {actual}, expected {expected}"));
                }
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                problems.push(format!("{path}: missing"));
            }
            Err(err) => problems.push(format!("{path}: {err}")),
        }
    }
    let mut on_disk = Vec::new();
    walk(dir, "", &mut on_disk)?;
    on_disk.sort();
    for path in on_disk {
        if path != MANIFEST && !manifest.files.contains_key(&path) {
            problems.push(format!("{path}: not listed in {MANIFEST}"));
        }
    }
    if problems.is_empty() {
        Ok(manifest)
    } else {
        Err(BundleError::Invalid {
            dir: dir.to_path_buf(),
            problems,
        })
    }
}

/// Every file below `dir`, as `/`-separated paths relative to the walk's root.
fn walk(dir: &Path, prefix: &str, out: &mut Vec<String>) -> Result<(), BundleError> {
    for entry in fs::read_dir(dir).map_err(io_err(dir))? {
        let entry = entry.map_err(io_err(dir))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = format!("{prefix}{name}");
        if entry.file_type().map_err(io_err(&entry.path()))?.is_dir() {
            walk(&entry.path(), &format!("{path}/"), out)?;
        } else {
            out.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundles_dir_order() {
        let data = Some(PathBuf::from("/home/me/.local/share/spyglass"));
        let user = PathBuf::from("/home/me/.local/share/spyglass/bundles");
        let env = Some(OsString::from("/srv/bundles"));
        // The variable wins over both others.
        assert_eq!(
            choose_bundles_dir(env.clone(), true, data.clone()),
            PathBuf::from("/srv/bundles")
        );
        assert_eq!(
            choose_bundles_dir(env, false, None),
            PathBuf::from("/srv/bundles")
        );
        // An empty variable counts as unset.
        assert_eq!(
            choose_bundles_dir(Some(OsString::new()), false, data.clone()),
            user
        );
        // A clone's `bundles/` wins over the per-user folder.
        assert_eq!(
            choose_bundles_dir(None, true, data.clone()),
            PathBuf::from("bundles")
        );
        assert_eq!(choose_bundles_dir(None, false, data), user);
        // No per-user folder: the working directory's, even though it doesn't exist.
        assert_eq!(
            choose_bundles_dir(None, false, None),
            PathBuf::from("bundles")
        );
    }

    #[test]
    fn kinds() {
        assert_eq!(Kind::from_manifest_type("release"), Kind::Release);
        assert_eq!(Kind::from_manifest_type("snapshot"), Kind::Snapshot);
        for id in ["26.3", "26.2", "1.21.4", "test"] {
            assert_eq!(Kind::from_id(id), Kind::Release, "{id}");
        }
        for id in [
            "26.4-snapshot-5",
            "1.21.5-pre1",
            "1.21.5-rc2",
            "25w14a",
            "26.4-pre-1",
        ] {
            assert_eq!(Kind::from_id(id), Kind::Snapshot, "{id}");
        }
        assert_eq!(Kind::from_id("ü5w14a"), Kind::Release);
        let json = r#"{"format": 1, "version": "26.4-snapshot-5", "data_version": 5125,
            "jar_sha1": "", "java_version": "", "extractor": "", "files": {}}"#;
        let mut manifest: Manifest = serde_json::from_str(json).unwrap();
        assert_eq!((manifest.kind, manifest.kind()), (None, Kind::Snapshot));
        manifest.kind = Some(Kind::Release);
        let written = serde_json::to_string(&manifest).unwrap();
        assert!(
            written.contains(r#""data_version":5125,"kind":"release""#),
            "{written}"
        );
    }

    /// A bundle in `dir` holding `files`, with a manifest listing them, as the extractor writes.
    fn bundle(dir: &Path, files: &[(&str, &str)]) -> Bundle {
        let mut listed = BTreeMap::new();
        for (path, text) in files {
            let file = dir.join(path);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(&file, text).unwrap();
            listed.insert(path.to_string(), sha1_hex(text.as_bytes()).unwrap());
        }
        let manifest = Manifest {
            format: FORMAT,
            version: "26.3".into(),
            data_version: 5023,
            kind: None,
            jar_sha1: String::new(),
            java_version: String::new(),
            extractor: String::new(),
            files: listed,
        };
        fs::write(
            dir.join(MANIFEST),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();
        Bundle::open(dir).unwrap()
    }

    #[test]
    fn reads_listed_resources_only() {
        let tmp = tempfile::tempdir().unwrap();
        let bundle = bundle(
            tmp.path(),
            &[
                ("data/minecraft/worldgen/noise/a.json", "{\"a\": 1}"),
                ("reports/biome_parameters/minecraft/nether.json", "[]"),
            ],
        );
        fs::write(
            tmp.path().join("data/minecraft/worldgen/noise/b.json"),
            "{}",
        )
        .unwrap();
        let noise = bundle.resource("worldgen/noise", "minecraft:a").unwrap();
        assert_eq!(noise, serde_json::json!({"a": 1}));
        // No namespace means minecraft.
        assert_eq!(bundle.resource("worldgen/noise", "a").unwrap(), noise);
        assert_eq!(
            bundle
                .report("reports/biome_parameters/minecraft/nether.json")
                .unwrap(),
            serde_json::json!([])
        );
        // On disk but not in the manifest.
        assert!(matches!(
            bundle.resource("worldgen/noise", "b"),
            Err(BundleError::NotFound(_))
        ));
        assert!(matches!(
            bundle.structure_template("minecraft:igloo/top"),
            Err(BundleError::Layout(_))
        ));
        assert!(verify(tmp.path()).is_err());
    }

    #[test]
    fn sha1_of_bytes_and_files() {
        // Known answer: SHA-1("{}").
        let expected = "bf21a9e8fbc5a3846fb05b4fa0859e0917b2202f";
        assert_eq!(sha1_hex(&b"{}"[..]).unwrap(), expected);
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("x"), "{}").unwrap();
        assert_eq!(
            sha1_hex(fs::File::open(tmp.path().join("x")).unwrap()).unwrap(),
            expected
        );
    }

    #[test]
    fn verify_rejects_escaping_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let bundle = tmp.path().join("b");
        fs::create_dir_all(&bundle).unwrap();
        fs::write(tmp.path().join("outside.json"), "{}").unwrap();
        fs::write(
            bundle.join(MANIFEST),
            r#"{"format": 1, "version": "x", "data_version": 1, "jar_sha1": "", "java_version": "", "extractor": "",
                "files": {"../outside.json": "bf21a9e8fbc5a3846fb05b4fa0859e0917b2202f"}}"#,
        )
        .unwrap();
        let BundleError::Invalid { problems, .. } = verify(&bundle).unwrap_err() else {
            panic!("expected Invalid");
        };
        assert_eq!(problems.len(), 1, "{problems:#?}");
        assert!(problems[0].contains("not a relative"));
    }

    #[test]
    fn resource_paths_follow_the_data_pack_layout() {
        assert_eq!(
            Bundle::resource_path(
                "worldgen/density_function",
                "minecraft:overworld/continents"
            )
            .unwrap(),
            "data/minecraft/worldgen/density_function/overworld/continents.json"
        );
        for bad in [
            "",
            "Minecraft:x",
            "minecraft:",
            "minecraft:a//b",
            "minecraft:../x",
            "a:b:c",
            "minecraft:x y",
        ] {
            assert!(
                matches!(
                    Bundle::resource_path("worldgen/noise", bad),
                    Err(BundleError::BadId(_))
                ),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn rejects_an_unknown_format() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(
            tmp.path().join(MANIFEST),
            r#"{"format": 2, "version": "x", "data_version": 1, "jar_sha1": "", "java_version": "", "extractor": "", "files": {}}"#,
        )
        .unwrap();
        assert!(matches!(
            read_manifest(tmp.path()),
            Err(BundleError::Layout(_))
        ));
    }
}
