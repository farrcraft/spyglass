//! `cargo xtask smoke <archive>`: a release archive, unpacked into an empty folder outside the
//! repo, run as someone without Rust would run it (docs/adr/0038-how-release-archives-are-built.md).
//! With `--extract`, it also builds a bundle with the packaged extractor and answers from it;
//! the bundle stays in a temporary folder and is deleted afterwards (ADR 0008).

use std::ffi::OsString;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use anyhow::{Context, Result, ensure};

#[derive(clap::Args)]
pub struct Smoke {
    /// The archive: `target/archives/spyglass-<version>-<target>.{zip,tar.gz}`.
    archive: PathBuf,
    /// Also extract this game version with the packaged extractor, then locate and render with
    /// it. Downloads the server jar and needs a matching Java.
    #[arg(long)]
    extract: Option<String>,
}

pub fn run(args: &Smoke) -> Result<()> {
    let bytes = std::fs::read(&args.archive)
        .with_context(|| format!("reading {}", args.archive.display()))?;
    let temp = tempfile::tempdir()?;
    let name = args
        .archive
        .file_name()
        .and_then(|n| n.to_str())
        .context("the archive's name isn't text")?;
    let root = if let Some(root) = name.strip_suffix(".zip") {
        zip::ZipArchive::new(Cursor::new(bytes))?.extract(temp.path())?;
        root
    } else {
        let root = name
            .strip_suffix(".tar.gz")
            .context("the archive is neither .zip nor .tar.gz")?;
        let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(bytes.as_slice()));
        tar.set_preserve_permissions(true);
        tar.unpack(temp.path())?;
        root
    };
    let dir = temp.path().join(root);
    let version = root
        .strip_prefix("spyglass-")
        .and_then(|rest| rest.split_once('-'))
        .map(|(version, _)| version)
        .context("the archive isn't named spyglass-<version>-<target>")?;
    let exe = if root.contains("windows") { ".exe" } else { "" };
    let bundles = temp.path().join("bundles");
    let run = |bin: &str, args: &[&str]| -> Result<Output> {
        let path = dir.join(format!("{bin}{exe}"));
        let out = Command::new(&path)
            .args(args)
            .current_dir(temp.path())
            .env_clear()
            .envs(user_env())
            .env("SPYGLASS_BUNDLES", &bundles)
            .output()
            .with_context(|| format!("running {}", path.display()))?;
        ensure!(
            out.status.success(),
            "{bin} {} failed:\n{}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr)
        );
        Ok(out)
    };

    for bin in ["spyglass", "spyglass-viewer", "spyglass-extract"] {
        let out = run(bin, &["--version"])?;
        let text = String::from_utf8_lossy(&out.stdout);
        ensure!(
            text.starts_with(&format!("{bin} {version} (")) && !text.contains("dev build"),
            "{bin} --version says {text:?}, not {version} with a commit"
        );
        println!("{}", text.lines().next().unwrap_or_default());
    }
    run("spyglass", &["--help"])?;
    run("spyglass-extract", &["--help"])?;

    if let Some(game) = &args.extract {
        run("spyglass-extract", &[game])?;
        let out = run(
            "spyglass",
            &[
                "locate",
                "spawn",
                "--seed",
                "1",
                "--game-version",
                game,
                "--json",
            ],
        )?;
        let answer: serde_json::Value = serde_json::from_slice(&out.stdout)?;
        ensure!(
            answer["version"] == game.as_str(),
            "locate answered for {}",
            answer["version"]
        );
        run(
            "spyglass",
            &[
                "locate",
                "structure",
                "#minecraft:village",
                "--seed",
                "1",
                "--game-version",
                game,
            ],
        )?;
        let map = temp.path().join("map.png");
        let map_arg = map.to_str().context("the temporary folder isn't text")?;
        run(
            "spyglass",
            &[
                "render",
                "--seed",
                "1",
                "--game-version",
                game,
                "--size",
                "64x64",
                "-o",
                map_arg,
            ],
        )?;
        ensure!(
            std::fs::read(&map)?.starts_with(b"\x89PNG"),
            "render didn't write a PNG"
        );
        println!("extracted {game}, located and rendered with it");
    }
    println!("{} works", args.archive.display());
    Ok(())
}

/// The environment a user's shell might have: this one, without Cargo's and rustup's variables,
/// and with their folders taken off PATH.
fn user_env() -> Vec<(OsString, OsString)> {
    std::env::vars_os()
        .filter(|(key, _)| {
            let key = key.to_string_lossy().to_uppercase();
            !(key.starts_with("CARGO") || key.starts_with("RUSTUP") || key.starts_with("RUST"))
        })
        .map(|(key, value)| {
            if key.to_string_lossy().eq_ignore_ascii_case("PATH") {
                let kept: Vec<PathBuf> = std::env::split_paths(&value)
                    .filter(|dir| !is_rust_dir(dir))
                    .collect();
                (key, std::env::join_paths(kept).unwrap_or(value))
            } else {
                (key, value)
            }
        })
        .collect()
}

fn is_rust_dir(dir: &Path) -> bool {
    dir.components().any(|c| {
        let c = c.as_os_str().to_string_lossy().to_lowercase();
        c == ".cargo" || c == ".rustup"
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_folders_are_found() {
        assert!(is_rust_dir(Path::new("/home/me/.cargo/bin")));
        assert!(is_rust_dir(Path::new("C:\\Users\\Me\\.Cargo\\bin")));
        assert!(!is_rust_dir(Path::new("/usr/bin")));
    }
}
