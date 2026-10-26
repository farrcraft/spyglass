//! Writing a version bundle from the game jar and the data generator's reports. Reading is in
//! `spyglass_data::bundle`. See `docs/specs/bundle-format.md` and ADR 0006.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use spyglass_data::bundle::{FORMAT, MANIFEST, Manifest, sha1_hex};
use zip::ZipArchive;
use zip::result::ZipError;

/// Directories of the game jar copied into a bundle, with their paths kept.
pub const JAR_DIRS: &[&str] = &[
    "data/minecraft/worldgen/",
    "data/minecraft/tags/worldgen/",
    "data/minecraft/dimension_type/",
    "data/minecraft/structure/",
];

/// Data-generator reports copied into a bundle, relative to the generator's output directory and
/// kept at the same path in the bundle.
pub const REPORTS: &[&str] = &[
    "reports/biome_parameters/minecraft/overworld.json",
    "reports/biome_parameters/minecraft/nether.json",
];

/// Why a bundle couldn't be written.
#[derive(Debug, thiserror::Error)]
pub enum WriteError {
    /// Reading or writing a file failed.
    #[error("I/O error on {path}: {source}")]
    Io {
        /// The file, or for an entry inside a jar, its name.
        path: PathBuf,
        /// The underlying error.
        source: io::Error,
    },
    /// The game jar isn't a valid zip, or an entry is missing.
    #[error("not a readable game jar: {0}")]
    Zip(#[from] ZipError),
    /// The jar's `version.json` isn't the JSON expected.
    #[error("malformed {path}: {source}")]
    Json {
        /// The file.
        path: PathBuf,
        /// The underlying error.
        source: serde_json::Error,
    },
    /// The inputs don't look like the game's, e.g. a report is missing or the jar is a
    /// different version.
    #[error("{0}")]
    Layout(String),
    /// The destination already holds a bundle and replacing it wasn't asked for.
    #[error("{0} already exists")]
    Exists(PathBuf),
}

fn io_err(path: &Path) -> impl FnOnce(io::Error) -> WriteError + '_ {
    move |source| WriteError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// What a bundle is built from.
#[derive(Debug, Clone)]
pub struct Sources<'a> {
    /// The game jar unpacked from the server jar's bundler.
    pub game_jar: &'a Path,
    /// The data generator's `--output` directory.
    pub reports_dir: &'a Path,
    /// Game version id. The game jar's `version.json` must agree.
    pub version: &'a str,
    /// SHA-1 of the server jar.
    pub jar_sha1: &'a str,
    /// `JAVA_VERSION` of the runtime that ran the data generator.
    pub java_version: &'a str,
}

/// The parts of the game jar's `version.json` that a bundle records.
#[derive(Deserialize)]
struct GameVersion {
    id: String,
    world_version: u32,
}

/// Writes a bundle to `dest` and returns its manifest.
///
/// Everything is written to a sibling staging directory first and renamed into place at the end,
/// so `dest` is either a complete bundle or untouched. An existing `dest` is an error unless
/// `replace` is set.
pub fn write(dest: &Path, sources: &Sources<'_>, replace: bool) -> Result<Manifest, WriteError> {
    if dest.exists() && !replace {
        return Err(WriteError::Exists(dest.to_path_buf()));
    }
    let parent = match dest.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    fs::create_dir_all(parent).map_err(io_err(parent))?;
    let staging = tempfile::Builder::new()
        .prefix(".bundle-")
        .tempdir_in(parent)
        .map_err(io_err(parent))?;

    let mut files = BTreeMap::new();
    let data_version = copy_jar(sources, staging.path(), &mut files)?;
    for report in REPORTS {
        let from = sources.reports_dir.join(report);
        let bytes = fs::read(&from).map_err(|err| match err.kind() {
            io::ErrorKind::NotFound => WriteError::Layout(format!(
                "the data generator didn't write {}",
                from.display()
            )),
            _ => io_err(&from)(err),
        })?;
        put(staging.path(), report, &bytes, &mut files)?;
    }

    let manifest = Manifest {
        format: FORMAT,
        version: sources.version.to_string(),
        data_version,
        jar_sha1: sources.jar_sha1.to_ascii_lowercase(),
        java_version: sources.java_version.to_string(),
        extractor: env!("CARGO_PKG_VERSION").to_string(),
        files,
    };
    let mut json = serde_json::to_string_pretty(&manifest).expect("a manifest always serializes");
    json.push('\n');
    let path = staging.path().join(MANIFEST);
    fs::write(&path, json).map_err(io_err(&path))?;

    // Move an old bundle aside rather than deleting it, so a failed rename can put it back. It's
    // deleted when `old` drops.
    let old = tempfile::Builder::new()
        .prefix(".bundle-old-")
        .tempdir_in(parent)
        .map_err(io_err(parent))?;
    let previous = old.path().join("previous");
    let had_previous = dest.exists();
    if had_previous {
        fs::rename(dest, &previous).map_err(io_err(dest))?;
    }
    if let Err(source) = fs::rename(staging.path(), dest) {
        if had_previous {
            let _ = fs::rename(&previous, dest);
        }
        return Err(WriteError::Io {
            path: dest.to_path_buf(),
            source,
        });
    }
    // The staging directory is now `dest`; don't let its guard delete it.
    let _ = staging.keep();
    Ok(manifest)
}

/// Copies every file under [`JAR_DIRS`] from the game jar and returns its data version.
fn copy_jar(
    sources: &Sources<'_>,
    root: &Path,
    files: &mut BTreeMap<String, String>,
) -> Result<u32, WriteError> {
    let file = File::open(sources.game_jar).map_err(io_err(sources.game_jar))?;
    let mut jar = ZipArchive::new(file)?;

    let version: GameVersion = {
        let mut text = String::new();
        jar.by_name("version.json")?
            .read_to_string(&mut text)
            .map_err(io_err(Path::new("version.json")))?;
        serde_json::from_str(&text).map_err(|source| WriteError::Json {
            path: "version.json".into(),
            source,
        })?
    };
    if version.id != sources.version {
        return Err(WriteError::Layout(format!(
            "the game jar is version {:?}, not {:?}",
            version.id, sources.version
        )));
    }

    for i in 0..jar.len() {
        let mut entry = jar.by_index(i)?;
        let name = entry.name()?.into_owned();
        if entry.is_dir() || !JAR_DIRS.iter().any(|dir| name.starts_with(dir)) {
            continue;
        }
        if entry.enclosed_name().is_none() {
            return Err(WriteError::Layout(format!(
                "unsafe path in game jar: {name:?}"
            )));
        }
        let mut bytes = Vec::with_capacity(usize::try_from(entry.size()).unwrap_or(0));
        entry
            .read_to_end(&mut bytes)
            .map_err(io_err(Path::new(&name)))?;
        put(root, &name, &bytes, files)?;
    }
    for dir in JAR_DIRS {
        if !files.keys().any(|path| path.starts_with(dir)) {
            return Err(WriteError::Layout(format!("the game jar has no {dir}")));
        }
    }
    Ok(version.world_version)
}

/// Writes `bytes` to `root/<path>` and records its hash.
fn put(
    root: &Path,
    path: &str,
    bytes: &[u8],
    files: &mut BTreeMap<String, String>,
) -> Result<(), WriteError> {
    let out = root.join(path);
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent).map_err(io_err(parent))?;
    }
    fs::write(&out, bytes).map_err(io_err(&out))?;
    files.insert(path.to_string(), sha1_hex(bytes).map_err(io_err(&out))?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use spyglass_data::bundle::{Bundle, BundleError, FORMAT, MANIFEST, read_manifest, verify};
    use std::io::{Cursor, Write};
    use zip::write::SimpleFileOptions;

    const VERSION_JSON: &[u8] = br#"{"id": "26.3", "world_version": 5023, "stable": true}"#;

    fn jar(dir: &Path, files: &[(&str, &[u8])]) -> PathBuf {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, content) in files {
            if name.ends_with('/') {
                zip.add_directory(*name, SimpleFileOptions::default())
                    .unwrap();
            } else {
                zip.start_file(*name, SimpleFileOptions::default()).unwrap();
                zip.write_all(content).unwrap();
            }
        }
        let path = dir.join("server-26.3.jar");
        fs::write(&path, zip.finish().unwrap().into_inner()).unwrap();
        path
    }

    fn game_jar(dir: &Path) -> PathBuf {
        jar(
            dir,
            &[
                ("version.json", VERSION_JSON),
                ("data/minecraft/worldgen/", b""),
                ("data/minecraft/worldgen/noise/a.json", b"{\"a\": 1}\n"),
                ("data/minecraft/worldgen/biome/plains.json", b"{}"),
                ("data/minecraft/tags/worldgen/biome/is_ocean.json", b"[]"),
                ("data/minecraft/tags/block/logs.json", b"not copied"),
                ("data/minecraft/dimension_type/overworld.json", b"{}"),
                ("data/minecraft/structure/igloo/top.nbt", b"\x1f\x8b"),
                ("data/minecraft/recipe/bread.json", b"not copied"),
                ("net/minecraft/Main.class", b"not copied"),
            ],
        )
    }

    fn reports(dir: &Path) -> PathBuf {
        let out = dir.join("generated");
        for report in REPORTS {
            let path = out.join(report);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, format!("{{\"report\": \"{report}\"}}\n")).unwrap();
        }
        fs::write(out.join("reports/blocks.json"), "not copied").unwrap();
        out
    }

    fn sources<'a>(game_jar: &'a Path, reports_dir: &'a Path) -> Sources<'a> {
        Sources {
            game_jar,
            reports_dir,
            version: "26.3",
            jar_sha1: "ABCDEF",
            java_version: "25.0.3",
        }
    }

    #[test]
    fn writes_a_bundle_and_verifies_it() {
        let tmp = tempfile::tempdir().unwrap();
        let (game, reports) = (game_jar(tmp.path()), reports(tmp.path()));
        let dest = tmp.path().join("bundles/26.3");

        let manifest = write(&dest, &sources(&game, &reports), false).unwrap();
        assert_eq!(manifest.format, FORMAT);
        assert_eq!(manifest.version, "26.3");
        assert_eq!(manifest.data_version, 5023);
        assert_eq!(manifest.jar_sha1, "abcdef");
        assert_eq!(manifest.java_version, "25.0.3");
        assert_eq!(
            manifest.files.keys().collect::<Vec<_>>(),
            [
                "data/minecraft/dimension_type/overworld.json",
                "data/minecraft/structure/igloo/top.nbt",
                "data/minecraft/tags/worldgen/biome/is_ocean.json",
                "data/minecraft/worldgen/biome/plains.json",
                "data/minecraft/worldgen/noise/a.json",
                "reports/biome_parameters/minecraft/nether.json",
                "reports/biome_parameters/minecraft/overworld.json",
            ]
        );
        assert_eq!(
            fs::read(dest.join("data/minecraft/worldgen/noise/a.json")).unwrap(),
            b"{\"a\": 1}\n"
        );
        // Known answer: SHA-1("{}").
        assert_eq!(
            manifest.files["data/minecraft/worldgen/biome/plains.json"],
            "bf21a9e8fbc5a3846fb05b4fa0859e0917b2202f"
        );

        assert_eq!(verify(&dest).unwrap(), manifest);
        assert_eq!(read_manifest(&dest).unwrap(), manifest);
        // Nothing but the bundle is left beside it: the staging directory was renamed.
        assert_eq!(fs::read_dir(dest.parent().unwrap()).unwrap().count(), 1);
    }

    #[test]
    fn manifest_is_deterministic_and_ends_with_newline() {
        let tmp = tempfile::tempdir().unwrap();
        let (game, reports) = (game_jar(tmp.path()), reports(tmp.path()));
        let (a, b) = (tmp.path().join("a"), tmp.path().join("b"));
        write(&a, &sources(&game, &reports), false).unwrap();
        write(&b, &sources(&game, &reports), false).unwrap();
        let text = fs::read_to_string(a.join(MANIFEST)).unwrap();
        assert_eq!(text, fs::read_to_string(b.join(MANIFEST)).unwrap());
        assert!(text.starts_with("{\n  \"format\": 1,\n  \"version\": \"26.3\",\n"));
        assert!(text.ends_with("}\n"));
    }

    #[test]
    fn refuses_to_overwrite_unless_asked() {
        let tmp = tempfile::tempdir().unwrap();
        let (game, reports) = (game_jar(tmp.path()), reports(tmp.path()));
        let dest = tmp.path().join("26.3");
        fs::create_dir_all(&dest).unwrap();
        fs::write(dest.join("stale.json"), "old").unwrap();

        let err = write(&dest, &sources(&game, &reports), false).unwrap_err();
        assert!(matches!(err, WriteError::Exists(_)), "{err}");
        assert!(dest.join("stale.json").exists());

        write(&dest, &sources(&game, &reports), true).unwrap();
        assert!(!dest.join("stale.json").exists());
        verify(&dest).unwrap();
    }

    #[test]
    fn verify_reports_every_problem() {
        let tmp = tempfile::tempdir().unwrap();
        let (game, reports) = (game_jar(tmp.path()), reports(tmp.path()));
        let dest = tmp.path().join("26.3");
        write(&dest, &sources(&game, &reports), false).unwrap();

        fs::write(dest.join("data/minecraft/worldgen/noise/a.json"), "edited").unwrap();
        fs::remove_file(dest.join("data/minecraft/worldgen/biome/plains.json")).unwrap();
        fs::write(dest.join("data/minecraft/worldgen/extra.json"), "{}").unwrap();

        let BundleError::Invalid { problems, .. } = verify(&dest).unwrap_err() else {
            panic!("expected Invalid");
        };
        assert_eq!(problems.len(), 3, "{problems:#?}");
        assert!(problems[0].starts_with("data/minecraft/worldgen/biome/plains.json: missing"));
        assert!(problems[1].starts_with("data/minecraft/worldgen/noise/a.json: SHA-1 is"));
        assert!(problems[2].starts_with("data/minecraft/worldgen/extra.json: not listed"));
    }

    #[test]
    fn reads_resources_by_registry_and_id() {
        let tmp = tempfile::tempdir().unwrap();
        let (game, reports) = (game_jar(tmp.path()), reports(tmp.path()));
        let dest = tmp.path().join("26.3");
        write(&dest, &sources(&game, &reports), false).unwrap();

        let bundle = Bundle::open(&dest).unwrap();
        assert_eq!(bundle.manifest().version, "26.3");
        let noise = bundle.resource("worldgen/noise", "minecraft:a").unwrap();
        assert_eq!(noise, serde_json::json!({"a": 1}));
        // No namespace means minecraft.
        assert_eq!(bundle.resource("worldgen/noise", "a").unwrap(), noise);
        assert_eq!(
            bundle.resource("dimension_type", "overworld").unwrap(),
            serde_json::json!({})
        );
        let report = bundle
            .report("reports/biome_parameters/minecraft/nether.json")
            .unwrap();
        assert!(report["report"].is_string());

        assert!(matches!(
            bundle.resource("worldgen/noise", "minecraft:missing"),
            Err(BundleError::NotFound(_))
        ));
        // Recipes exist in the jar but aren't in bundles.
        assert!(matches!(
            bundle.resource("recipe", "bread"),
            Err(BundleError::NotFound(_))
        ));
    }

    #[test]
    fn rejects_a_jar_of_another_version() {
        let tmp = tempfile::tempdir().unwrap();
        let reports = reports(tmp.path());
        let game = jar(
            tmp.path(),
            &[("version.json", br#"{"id": "26.2", "world_version": 1}"#)],
        );
        let err = write(&tmp.path().join("b"), &sources(&game, &reports), false).unwrap_err();
        assert!(matches!(err, WriteError::Layout(_)), "{err}");
        assert!(!tmp.path().join("b").exists());
    }

    #[test]
    fn rejects_missing_reports_and_leaves_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let game = game_jar(tmp.path());
        let empty = tmp.path().join("empty");
        fs::create_dir_all(&empty).unwrap();
        let dest = tmp.path().join("out/26.3");
        let err = write(&dest, &sources(&game, &empty), false).unwrap_err();
        assert!(matches!(err, WriteError::Layout(_)), "{err}");
        assert_eq!(fs::read_dir(tmp.path().join("out")).unwrap().count(), 0);
    }
}
