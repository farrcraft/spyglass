//! `cargo xtask dist`: one target's release archive (docs/adr/0038-how-release-archives-are-built.md).
//!
//! The three binaries are built with the `dist` profile from the commit checked out, with build
//! machine paths remapped and the commit named in `--version`. The archive holds them, the docs
//! and the third-party licenses, in a fixed order with fixed times and permissions, so the same
//! commit gives the same bytes. Before it's written, every name is checked against an allowlist
//! and every file is scanned for the build machine's paths.

use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail, ensure};

use crate::{command, repo_root, toml_string};

#[derive(clap::Args)]
pub struct Dist {
    /// The target triple to build for [default: this machine's].
    #[arg(long)]
    target: Option<String>,
}

/// The binaries shipped: package, then binary name.
const BINARIES: [(&str, &str); 3] = [
    ("spyglass-cli", "spyglass"),
    ("spyglass-viewer", "spyglass-viewer"),
    ("spyglass-extract", "spyglass-extract"),
];

/// Files copied from the repo root into every archive.
const DOCS: [&str; 3] = ["README.md", "LICENSE", "CHANGELOG.md"];

/// The first-run guide, for someone with no Rust and no clone.
const GUIDE: &str = "xtask/dist/GETTING-STARTED.txt";

/// The third-party licenses' file in the archive, made by cargo-about.
const LICENSES: &str = "THIRD-PARTY-LICENSES.html";

/// Every name an archive may hold, below its top folder. Anything else, a bundle, a jar or an
/// oracle output above all, fails the build (ADR 0008).
const ALLOWED: [&str; 8] = [
    "spyglass",
    "spyglass-viewer",
    "spyglass-extract",
    "README.md",
    "LICENSE",
    "CHANGELOG.md",
    "GETTING-STARTED.txt",
    LICENSES,
];

/// One file in an archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Its name below the archive's top folder.
    pub name: String,
    pub bytes: Vec<u8>,
    pub executable: bool,
}

/// The commit an archive is built from.
struct Build {
    /// Abbreviated hash, with `-dirty` if the tree has changes.
    commit: String,
    /// Commit date, `YYYY-MM-DD`.
    date: String,
    /// Commit time, seconds since the epoch: every file's time in the archive.
    epoch: u64,
}

pub fn run(args: &Dist) -> Result<()> {
    let host = host_target()?;
    let target = args.target.clone().unwrap_or_else(|| host.clone());
    let version = toml_string("Cargo.toml", &["workspace", "package", "version"])?;
    let build = build_info()?;
    if build.commit.ends_with("-dirty") {
        eprintln!("warning: the tree has uncommitted changes; the build is marked -dirty");
    }

    build_binaries(&target, &build)?;
    let windows = target.contains("windows");
    let exe = if windows { ".exe" } else { "" };
    let out = repo_root().join("target").join(&target).join("dist");
    let mut entries = Vec::new();
    for (_, bin) in BINARIES {
        let name = format!("{bin}{exe}");
        entries.push(Entry {
            bytes: read(&out.join(&name))?,
            name,
            executable: true,
        });
    }
    for doc in DOCS {
        entries.push(file(doc, doc)?);
    }
    entries.push(file(GUIDE, "GETTING-STARTED.txt")?);
    entries.push(Entry {
        name: LICENSES.to_string(),
        bytes: third_party_licenses()?,
        executable: false,
    });

    for entry in &entries {
        ensure!(
            allowed(&entry.name),
            "{} isn't allowed in an archive",
            entry.name
        );
    }
    let leaks = leak_needles();
    for entry in &entries {
        if let Some(needle) = leak(&entry.bytes, &leaks) {
            bail!(
                "{} contains a path from this machine ({needle}); is it remapped?",
                entry.name
            );
        }
    }

    let root = format!("spyglass-{version}-{target}");
    let (bytes, ext) = if windows {
        (zip(&root, &entries, build.epoch)?, "zip")
    } else {
        (tar_gz(&root, &entries, build.epoch)?, "tar.gz")
    };
    let dir = repo_root().join("target").join("archives");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{root}.{ext}"));
    std::fs::write(&path, &bytes).with_context(|| format!("writing {}", path.display()))?;

    if target == host {
        check_version(&out.join(format!("spyglass{exe}")), &build)?;
    }
    println!("{} ({} KiB)", path.display(), bytes.len() / 1024);
    Ok(())
}

/// This machine's target triple, from `rustc -vV`.
fn host_target() -> Result<String> {
    let out = command("rustc").arg("-vV").output()?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .map(str::to_string)
        .context("rustc -vV didn't name the host")
}

fn build_info() -> Result<Build> {
    let git = |args: &[&str]| -> Result<String> {
        let out = command("git")
            .args(args)
            .output()
            .context("failed to run git")?;
        ensure!(out.status.success(), "git {} failed", args.join(" "));
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    };
    let mut commit = git(&["rev-parse", "--short=12", "HEAD"])?;
    if !git(&["status", "--porcelain"])?.is_empty() {
        commit.push_str("-dirty");
    }
    Ok(Build {
        commit,
        date: git(&["log", "-1", "--format=%cs"])?,
        epoch: git(&["log", "-1", "--format=%ct"])?
            .parse()
            .context("git's commit time isn't a number")?,
    })
}

/// `cargo build --profile dist` for `target`, with this build's commit and remapped paths.
fn build_binaries(target: &str, build: &Build) -> Result<()> {
    let mut flags: Vec<String> = remaps()
        .into_iter()
        .map(|(from, to)| format!("--remap-path-prefix={}={to}", from.display()))
        .collect();
    if target.ends_with("windows-msvc") {
        // No Visual C++ runtime needed on the user's machine; the PDB named by file name only,
        // not by the path it was linked at; and the header's timestamp and the PDB's GUID made
        // from the output's hash instead of the clock, so builds repeat byte for byte.
        flags.push("-Ctarget-feature=+crt-static".into());
        flags.push("-Clink-arg=/PDBALTPATH:%_PDB%".into());
        flags.push("-Clink-arg=/Brepro".into());
    }
    let mut cmd = command("cargo");
    cmd.args(["build", "--profile", "dist", "--locked", "--target", target]);
    for (package, _) in BINARIES {
        cmd.args(["-p", package]);
    }
    // With --target, these reach only the target's crates, not build scripts or proc macros.
    cmd.env("CARGO_ENCODED_RUSTFLAGS", flags.join("\x1f"))
        .env_remove("RUSTFLAGS")
        .env("SPYGLASS_BUILD_COMMIT", &build.commit)
        .env("SPYGLASS_BUILD_DATE", &build.date)
        .env("SOURCE_DATE_EPOCH", build.epoch.to_string());
    ensure!(
        cmd.status().context("failed to run cargo")?.success(),
        "the dist build failed"
    );
    Ok(())
}

/// Build machine folders and what each becomes in the binaries.
fn remaps() -> Vec<(PathBuf, &'static str)> {
    let home = home_dir();
    let mut remaps = vec![(repo_root().to_path_buf(), "spyglass")];
    let cargo = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| home.as_ref().map(|h| h.join(".cargo")));
    let rustup = std::env::var_os("RUSTUP_HOME")
        .map(PathBuf::from)
        .or_else(|| home.as_ref().map(|h| h.join(".rustup")));
    remaps.extend(cargo.map(|dir| (dir, "cargo")));
    remaps.extend(rustup.map(|dir| (dir, "rustup")));
    remaps
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

/// What the archive's files must not contain: this machine's home and repo folders, each with
/// either separator, lowercased.
fn leak_needles() -> Vec<String> {
    let mut dirs: Vec<PathBuf> = remaps().into_iter().map(|(dir, _)| dir).collect();
    dirs.extend(home_dir());
    let mut needles: Vec<String> = dirs
        .iter()
        .flat_map(|dir| {
            let text = dir.to_string_lossy().to_lowercase();
            [text.replace('\\', "/"), text.replace('/', "\\")]
        })
        // `/` alone, or a home that's the root, would match everything.
        .filter(|needle| needle.len() > 3)
        .collect();
    needles.sort();
    needles.dedup();
    needles
}

/// The first of `needles` in `bytes`, ignoring ASCII case.
fn leak<'a>(bytes: &[u8], needles: &'a [String]) -> Option<&'a str> {
    let lower = bytes.to_ascii_lowercase();
    needles
        .iter()
        .find(|needle| {
            let needle = needle.as_bytes();
            lower.windows(needle.len()).any(|window| window == needle)
        })
        .map(String::as_str)
}

/// Whether `name` may be in an archive. Windows binaries' `.exe` doesn't count.
fn allowed(name: &str) -> bool {
    ALLOWED.contains(&name.strip_suffix(".exe").unwrap_or(name))
}

fn read(path: &Path) -> Result<Vec<u8>> {
    std::fs::read(path).with_context(|| format!("reading {}", path.display()))
}

/// `from` in the repo, as `name` in the archive.
fn file(from: &str, name: &str) -> Result<Entry> {
    Ok(Entry {
        name: name.to_string(),
        bytes: read(&repo_root().join(from))?,
        executable: false,
    })
}

/// The third-party licenses page, from cargo-about.
fn third_party_licenses() -> Result<Vec<u8>> {
    // To a file: cargo-about refuses to write to a pipe from PowerShell, for fear of its encoding.
    // Not in target/archives/, whose `spyglass-*` files are the release's.
    let page = repo_root().join("target").join("about").join(LICENSES);
    std::fs::create_dir_all(page.parent().expect("a file in a folder"))?;
    let out = command("cargo")
        .args(["about", "generate", "--locked", "--workspace", "--fail"])
        .args(["-c", "xtask/dist/about.toml", "xtask/dist/about.hbs", "-o"])
        .arg(&page)
        .output()
        .context("failed to run cargo about")?;
    if !out.status.success() {
        bail!(
            "cargo about failed (install it with scripts/setup.ps1 -ReleaseTools or \
             scripts/setup.sh --release-tools):\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    read(&page)
}

/// Runs the built `spyglass --version`, which must name the commit.
fn check_version(binary: &Path, build: &Build) -> Result<()> {
    let out = Command::new(binary).arg("--version").output()?;
    let text = String::from_utf8_lossy(&out.stdout);
    ensure!(
        out.status.success() && text.contains(&build.commit),
        "{} --version doesn't name {}: {text}",
        binary.display(),
        build.commit
    );
    Ok(())
}

/// Unix permissions for an entry.
fn mode(entry: &Entry) -> u32 {
    if entry.executable { 0o755 } else { 0o644 }
}

/// Entries in name order, so the archive doesn't depend on the order they were gathered in.
fn sorted(entries: &[Entry]) -> Vec<&Entry> {
    let mut sorted: Vec<&Entry> = entries.iter().collect();
    sorted.sort_by(|a, b| a.name.cmp(&b.name));
    sorted
}

/// A gzipped tar of `entries` below `root/`, every file at time `epoch`, owned by root.
pub fn tar_gz(root: &str, entries: &[Entry], epoch: u64) -> Result<Vec<u8>> {
    // GzEncoder's header has no file name and time 0, so it adds nothing that varies.
    let gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    let mut tar = tar::Builder::new(gz);
    tar.mode(tar::HeaderMode::Deterministic);
    for entry in sorted(entries) {
        let mut header = tar::Header::new_gnu();
        header.set_size(entry.bytes.len() as u64);
        header.set_mode(mode(entry));
        header.set_mtime(epoch);
        header.set_uid(0);
        header.set_gid(0);
        header.set_entry_type(tar::EntryType::Regular);
        tar.append_data(
            &mut header,
            format!("{root}/{}", entry.name),
            entry.bytes.as_slice(),
        )?;
    }
    Ok(tar.into_inner()?.finish()?)
}

/// A zip of `entries` below `root/`, every file at time `epoch` (UTC).
pub fn zip(root: &str, entries: &[Entry], epoch: u64) -> Result<Vec<u8>> {
    let (year, month, day) = civil_date(epoch / 86_400);
    let seconds = epoch % 86_400;
    let time = zip::DateTime::from_date_and_time(
        u16::try_from(year)?,
        month,
        day,
        u8::try_from(seconds / 3600)?,
        u8::try_from(seconds / 60 % 60)?,
        u8::try_from(seconds % 60)?,
    )?;
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for entry in sorted(entries) {
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .compression_level(Some(9))
            .last_modified_time(time)
            // Made on Unix whatever the build machine, so the bytes don't depend on its OS and
            // readers take the permissions.
            .system(zip::System::Unix)
            .unix_permissions(mode(entry));
        zip.start_file(format!("{root}/{}", entry.name), options)?;
        zip.write_all(&entry.bytes)?;
    }
    Ok(zip.finish()?.into_inner())
}

/// The UTC date `days` days after 1970-01-01, as (year, month, day). Howard Hinnant's
/// `civil_from_days`.
pub fn civil_date(days: u64) -> (u64, u8, u8) {
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z % 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + u64::from(month <= 2);
    // Both fit: day is 1 to 31, month 1 to 12.
    (year, month as u8, day as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries() -> Vec<Entry> {
        vec![
            Entry {
                name: "spyglass".into(),
                bytes: b"\x7fELF binary".to_vec(),
                executable: true,
            },
            Entry {
                name: "README.md".into(),
                bytes: b"# spyglass\n".to_vec(),
                executable: false,
            },
        ]
    }

    #[test]
    fn archives_are_deterministic() {
        let mut reversed = entries();
        reversed.reverse();
        for write in [tar_gz, zip] {
            let a = write("spyglass-0.1.0-x", &entries(), 1_793_000_000).unwrap();
            let b = write("spyglass-0.1.0-x", &reversed, 1_793_000_000).unwrap();
            assert_eq!(a, b, "the order entries are given in changes nothing");
            let later = write("spyglass-0.1.0-x", &entries(), 1_793_000_100).unwrap();
            assert_ne!(a, later, "the commit time is the files' time");
        }
    }

    #[test]
    fn archives_hold_what_was_given() {
        let bytes = tar_gz("root", &entries(), 0).unwrap();
        let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(bytes.as_slice()));
        let mut seen = Vec::new();
        for file in tar.entries().unwrap() {
            let file = file.unwrap();
            let header = file.header();
            seen.push((
                file.path().unwrap().to_string_lossy().into_owned(),
                header.mode().unwrap(),
            ));
        }
        assert_eq!(
            seen,
            [
                ("root/README.md".to_string(), 0o644),
                ("root/spyglass".to_string(), 0o755)
            ]
        );
        let bytes = zip("root", &entries(), 1_793_000_000).unwrap();
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let file = zip.by_name("root/spyglass").unwrap();
        assert_eq!(file.unix_mode(), Some(0o100_755));
        let time = file.last_modified().unwrap();
        // 1_793_000_000 is 2026-10-26 07:33:20 UTC.
        assert_eq!(
            (
                time.year(),
                time.month(),
                time.day(),
                time.hour(),
                time.minute()
            ),
            (2026, 10, 26, 7, 33)
        );
    }

    #[test]
    fn dates() {
        assert_eq!(civil_date(0), (1970, 1, 1));
        assert_eq!(civil_date(11_016), (2000, 2, 29));
        assert_eq!(civil_date(20_754), (2026, 10, 28));
    }

    #[test]
    fn only_allowed_names() {
        for name in ["spyglass", "spyglass.exe", "LICENSE", LICENSES] {
            assert!(allowed(name), "{name}");
        }
        for name in [
            "",
            ".exe",
            "26.3/bundle.json",
            "server.jar",
            "bundles",
            "spyglass.pdb",
        ] {
            assert!(!allowed(name), "{name}");
        }
    }

    /// The strings in the array at `keys` in the repo's TOML file `file`, sorted.
    fn toml_strings(file: &str, keys: &[&str]) -> Vec<String> {
        let text = std::fs::read_to_string(repo_root().join(file)).unwrap();
        let document: toml_edit::Document<String> = toml_edit::Document::parse(text).unwrap();
        let mut item = document.as_item();
        for key in keys {
            item = &item[key];
        }
        let mut strings: Vec<String> = item
            .as_array()
            .unwrap_or_else(|| panic!("no {} in {file}", keys.join(".")))
            .iter()
            .map(|value| value.as_str().unwrap().to_string())
            .collect();
        strings.sort();
        strings
    }

    /// The setup scripts and the release workflow install the same cargo-about.
    #[test]
    fn one_cargo_about_version() {
        let pins: Vec<(&str, String)> = [
            "scripts/setup.sh",
            "scripts/setup.ps1",
            ".github/workflows/release.yml",
        ]
        .into_iter()
        .map(|file| {
            let text = std::fs::read_to_string(repo_root().join(file)).unwrap();
            let (_, rest) = text
                .split_once("cargo-about@")
                .unwrap_or_else(|| panic!("{file} doesn't install cargo-about@<version>"));
            let version: String = rest
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == '.')
                .collect();
            (file, version)
        })
        .collect();
        assert!(pins.iter().all(|(_, v)| *v == pins[0].1), "{pins:?}");
    }

    #[test]
    fn about_accepts_what_deny_allows() {
        assert_eq!(
            toml_strings("xtask/dist/about.toml", &["accepted"]),
            toml_strings("deny.toml", &["licenses", "allow"]),
            "xtask/dist/about.toml's accepted licenses must equal deny.toml's"
        );
    }

    #[test]
    fn leaks_are_found_in_any_case() {
        let needles = vec!["c:\\users\\me".to_string(), "/home/me".to_string()];
        assert_eq!(
            leak(b"panicked at C:\\Users\\Me\\x.rs", &needles),
            Some("c:\\users\\me")
        );
        assert_eq!(
            leak(b"at /home/me/.cargo/registry", &needles),
            Some("/home/me")
        );
        assert_eq!(leak(b"at cargo/registry/src/x.rs", &needles), None);
    }
}
