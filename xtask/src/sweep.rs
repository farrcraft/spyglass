//! `cargo xtask sweep`: delete stale build outputs from `target/` (docs/adr/0061-keeping-target-small.md).
//!
//! Cargo names each output after its crate and a hash of how it was built (profile, features,
//! dependencies' versions), and never deletes an old hash when a new one replaces it. A dependency
//! bump, a feature change or clippy's check build each leave a full copy behind, so `target/`
//! grows without bound. This keeps, for each name in each output directory, the newest copy and
//! any copy written within the last few days, and deletes the rest. Deleting an output a build
//! still uses is safe: cargo's freshness check sees it missing and rebuilds it.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};

use crate::repo_root;

/// The directories of a profile's outputs (`target/<profile>/` or `target/<triple>/<profile>/`)
/// whose entries are named `<name>-<hash>`.
const OUTPUT_DIRS: &[&str] = &["deps", "incremental", "build", ".fingerprint", "examples"];

/// How deep below `target/` profile directories are looked for: `target/<triple>/<profile>/`.
const PROFILE_DEPTH: usize = 2;

#[derive(clap::Args)]
pub struct Sweep {
    /// Keep every copy written within this many days, as well as each name's newest.
    #[arg(long, default_value_t = DEFAULT_DAYS)]
    days: u64,
    /// Only report what would be deleted.
    #[arg(long)]
    dry_run: bool,
}

/// The default for `--days`: a week, so a build variant used now and then isn't rebuilt every
/// time `cargo xtask check` sweeps.
pub const DEFAULT_DAYS: u64 = 7;

impl Default for Sweep {
    fn default() -> Self {
        Sweep {
            days: DEFAULT_DAYS,
            dry_run: false,
        }
    }
}

pub fn run(args: &Sweep) -> Result<()> {
    let target = target_dir();
    let stale = stale(&target, args.days)?;
    let bytes: u64 = stale.iter().map(|s| s.bytes).sum();
    if args.dry_run {
        println!(
            "{} stale build outputs, {}, in {}",
            stale.len(),
            human(bytes),
            target.display()
        );
        return Ok(());
    }
    for output in &stale {
        let removed = if output.path.is_dir() {
            fs::remove_dir_all(&output.path)
        } else {
            fs::remove_file(&output.path)
        };
        removed.with_context(|| format!("failed to delete {}", output.path.display()))?;
    }
    println!(
        "swept {} stale build outputs, {}, from {}",
        stale.len(),
        human(bytes),
        target.display()
    );
    Ok(())
}

/// The target directory cargo uses here: `CARGO_TARGET_DIR`, else the repo's `target/`.
pub fn target_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root().join("target"))
}

/// A build output to delete, with its size.
pub struct Stale {
    pub path: PathBuf,
    pub bytes: u64,
}

/// The stale outputs under `target`: in every profile directory's output directories, each
/// entry named `<name>-<hash>` that is neither its name's newest nor written within `days`.
/// Entries without a hash (the uplifted binaries, `.cargo-lock`) are always kept.
pub fn stale(target: &Path, days: u64) -> Result<Vec<Stale>> {
    let cutoff = SystemTime::now()
        .checked_sub(Duration::from_secs(days * 24 * 60 * 60))
        .unwrap_or(SystemTime::UNIX_EPOCH);
    let mut found = Vec::new();
    for profile in profile_dirs(target, PROFILE_DEPTH) {
        for dir in OUTPUT_DIRS {
            let dir = profile.join(dir);
            if dir.is_dir() {
                stale_in(&dir, cutoff, &mut found)?;
            }
        }
    }
    Ok(found)
}

/// Directories under `dir`, down to `depth` levels below it, that hold a profile's outputs:
/// those with a `.fingerprint` directory.
fn profile_dirs(dir: &Path, depth: usize) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if path.join(".fingerprint").is_dir() {
            found.push(path);
        } else if depth > 1 {
            found.extend(profile_dirs(&path, depth - 1));
        }
    }
    found
}

/// One output: a hash's file or directory under a name.
struct Output {
    path: PathBuf,
    hash: String,
    modified: SystemTime,
}

/// Adds the stale entries of one output directory to `found`. All the files of one build of a
/// name share its hash (`foo-<hash>.exe`, `.pdb`, `.d`), so they're kept or deleted together.
fn stale_in(dir: &Path, cutoff: SystemTime, found: &mut Vec<Stale>) -> Result<()> {
    let mut by_name: HashMap<String, Vec<Output>> = HashMap::new();
    for entry in fs::read_dir(dir).with_context(|| format!("failed to read {}", dir.display()))? {
        let entry = entry?;
        let file_name = entry.file_name();
        let Some((name, hash)) = file_name.to_str().and_then(split_hash) else {
            continue;
        };
        let path = entry.path();
        by_name.entry(name.to_string()).or_default().push(Output {
            modified: modified(&path),
            hash: hash.to_string(),
            path,
        });
    }
    for outputs in by_name.values() {
        // The newest build of each hash, and the newest hash of all.
        let mut newest_of: HashMap<&str, SystemTime> = HashMap::new();
        for output in outputs {
            let newest = newest_of
                .entry(&output.hash)
                .or_insert(SystemTime::UNIX_EPOCH);
            *newest = (*newest).max(output.modified);
        }
        let newest_hash = newest_of
            .iter()
            .max_by_key(|(_, modified)| **modified)
            .map(|(hash, _)| *hash);
        for output in outputs {
            let keep = Some(output.hash.as_str()) == newest_hash
                || newest_of[output.hash.as_str()] >= cutoff;
            if !keep {
                found.push(Stale {
                    bytes: size(&output.path),
                    path: output.path.clone(),
                });
            }
        }
    }
    Ok(())
}

/// `foo_bar-0123456789abcdef.exe` as (`foo_bar.exe`, `0123456789abcdef`): the name with its
/// extension, and the hash after the last `-`. Cargo's hashes are 16 hex digits; incremental
/// directories' are base 36. `None` for a name without one.
fn split_hash(file_name: &str) -> Option<(String, &str)> {
    let (stem, rest) = file_name.rsplit_once('-')?;
    let (hash, extension) = match rest.find('.') {
        Some(dot) => rest.split_at(dot),
        None => (rest, ""),
    };
    let is_hash = hash.len() >= 10
        && hash
            .bytes()
            .all(|b| b.is_ascii_digit() || b.is_ascii_lowercase());
    is_hash.then(|| (format!("{stem}{extension}"), hash))
}

/// When `path` was last written: for a directory, the newest of it and its entries, since cargo
/// writes inside an incremental or build directory without touching the directory itself.
fn modified(path: &Path) -> SystemTime {
    let own = |p: &Path| {
        fs::metadata(p)
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH)
    };
    let mut newest = own(path);
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            newest = newest.max(own(&entry.path()));
        }
    }
    newest
}

/// The bytes in `path`, a file or a whole directory.
fn size(path: &Path) -> u64 {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return 0;
    };
    if !meta.is_dir() {
        return meta.len();
    }
    fs::read_dir(path)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| size(&entry.path()))
        .sum()
}

/// `bytes` for people: `1.4 GB`, `350 MB`.
pub fn human(bytes: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    let mb = bytes as f64 / MB;
    if mb >= 1024.0 {
        format!("{:.1} GB", mb / 1024.0)
    } else {
        format!("{mb:.0} MB")
    }
}

#[cfg(test)]
mod tests {
    use std::fs::File;

    use super::*;

    const OLD: Duration = Duration::from_secs(30 * 24 * 60 * 60);

    fn write(path: &Path, age: Duration) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"x").unwrap();
        let file = File::options().write(true).open(path).unwrap();
        file.set_modified(SystemTime::now() - age).unwrap();
    }

    #[test]
    fn hashes_split_from_names() {
        assert_eq!(
            split_hash("libspyglass_data-0123456789abcdef.rlib"),
            Some(("libspyglass_data.rlib".into(), "0123456789abcdef"))
        );
        assert_eq!(
            split_hash("spyglass-data-0123456789abcdef"),
            Some(("spyglass-data".into(), "0123456789abcdef"))
        );
        assert_eq!(
            split_hash("spyglass_data-2x9k3abcd0q1z"),
            Some(("spyglass_data".into(), "2x9k3abcd0q1z"))
        );
        assert_eq!(split_hash("spyglass_viewer.pdb"), None);
        assert_eq!(split_hash(".cargo-lock"), None);
        assert_eq!(split_hash("spyglass-viewer"), None);
    }

    #[test]
    fn keeps_the_newest_and_the_recent_and_deletes_the_rest() {
        let tmp = tempfile::tempdir().unwrap();
        let debug = tmp.path().join("debug");
        fs::create_dir_all(debug.join(".fingerprint")).unwrap();
        let deps = debug.join("deps");
        // Three builds of one test binary, two files each: one recent, two old.
        let hour = Duration::from_secs(3600);
        for (hash, age) in [
            ("aaaaaaaaaaaaaaaa", OLD + hour),
            ("bbbbbbbbbbbbbbbb", OLD),
            ("cccccccccccccccc", hour),
        ] {
            write(&deps.join(format!("terrain-{hash}.exe")), age);
            write(&deps.join(format!("terrain-{hash}.pdb")), age);
        }
        // Only old builds of another name: its newest stays.
        write(&deps.join("libnoise-dddddddddddddddd.rlib"), OLD + hour);
        write(&deps.join("libnoise-eeeeeeeeeeeeeeee.rlib"), OLD);
        // No hash: always kept.
        write(&debug.join("spyglass.exe"), OLD);

        let mut stale: Vec<String> = stale(tmp.path(), DEFAULT_DAYS)
            .unwrap()
            .iter()
            .map(|s| s.path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        stale.sort();
        assert_eq!(
            stale,
            [
                "libnoise-dddddddddddddddd.rlib",
                "terrain-aaaaaaaaaaaaaaaa.exe",
                "terrain-aaaaaaaaaaaaaaaa.pdb",
                "terrain-bbbbbbbbbbbbbbbb.exe",
                "terrain-bbbbbbbbbbbbbbbb.pdb",
            ]
        );
    }

    #[test]
    fn finds_profiles_below_a_target_triple_and_leaves_other_folders() {
        let tmp = tempfile::tempdir().unwrap();
        let release = tmp.path().join("x86_64-pc-windows-msvc/release");
        fs::create_dir_all(release.join(".fingerprint")).unwrap();
        write(&release.join("deps/a-aaaaaaaaaaaaaaaa.exe"), OLD + OLD);
        write(&release.join("deps/a-bbbbbbbbbbbbbbbb.exe"), OLD);
        // The oracle's outputs have no .fingerprint: never touched.
        write(
            &tmp.path().join("oracle/26.3/a-aaaaaaaaaaaaaaaa.json"),
            OLD + OLD,
        );
        write(&tmp.path().join("oracle/26.3/a-bbbbbbbbbbbbbbbb.json"), OLD);

        let stale = stale(tmp.path(), DEFAULT_DAYS).unwrap();
        assert_eq!(stale.len(), 1);
        assert!(stale[0].path.ends_with("deps/a-aaaaaaaaaaaaaaaa.exe"));
    }
}
