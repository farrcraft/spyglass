//! `cargo xtask fuzz <target>`: fuzz a reader of untrusted files with cargo-fuzz on nightly
//! (docs/adr/0036-fuzzing-with-cargo-fuzz.md). The targets are in `fuzz/`, a workspace of their
//! own; what fuzzing makes (the corpus, crashing inputs) stays in its gitignored folders.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use spyglass_data::nbt::{self, Compound, Tag};

use crate::{command, repo_root};

#[derive(clap::Args)]
pub struct Fuzz {
    /// The target: `nbt` (the NBT reader) or `region` (a region file's chunk scan).
    target: String,
    /// How long to fuzz, in seconds.
    #[arg(long, default_value_t = 60)]
    seconds: u64,
}

pub fn run(args: &Fuzz) -> Result<()> {
    let seeds: fn() -> Vec<(&'static str, Vec<u8>)> = match args.target.as_str() {
        "nbt" => nbt_seeds,
        "region" => region_seeds,
        other => bail!("no fuzz target {other:?}: use nbt or region"),
    };
    let dir = repo_root().join("fuzz");
    let corpus = dir.join("corpus").join(&args.target);
    fs::create_dir_all(&corpus)?;
    // Valid inputs to start from, so the fuzzer explores past the first length check.
    for (name, bytes) in seeds() {
        let path = corpus.join(name);
        if !path.exists() {
            fs::write(&path, bytes)?;
        }
    }
    let mut cmd = command("cargo");
    cmd.current_dir(&dir)
        .args(["+nightly", "fuzz", "run", &args.target]);
    cmd.arg("--")
        .arg(format!("-max_total_time={}", args.seconds));
    if cfg!(windows) {
        // The sanitizer's runtime ships with Visual Studio's MSVC tools, not on PATH.
        let runtime = asan_runtime_dir().context(
            "no clang_rt.asan_dynamic-x86_64.dll found; install Visual Studio's MSVC build tools",
        )?;
        let mut path = OsString::from(runtime);
        path.push(";");
        path.push(std::env::var_os("PATH").unwrap_or_default());
        cmd.env("PATH", path);
    }
    let status = cmd
        .status()
        .context("failed to run cargo +nightly fuzz; install it with rustup toolchain install nightly and cargo install cargo-fuzz")?;
    if !status.success() {
        bail!(
            "fuzzing {} failed; a crashing input is in fuzz/artifacts/{}/",
            args.target,
            args.target
        );
    }
    Ok(())
}

/// Where Visual Studio keeps the 64-bit AddressSanitizer runtime: the newest MSVC tools'
/// `bin/Hostx64/x64` under any edition of any version.
fn asan_runtime_dir() -> Option<PathBuf> {
    let roots = ["ProgramFiles", "ProgramFiles(x86)"]
        .iter()
        .filter_map(std::env::var_os)
        .map(|p| PathBuf::from(p).join("Microsoft Visual Studio"));
    let mut found: Vec<PathBuf> = Vec::new();
    for root in roots {
        for version in children(&root) {
            for edition in children(&version) {
                for tools in children(&edition.join("VC/Tools/MSVC")) {
                    let bin = tools.join("bin/Hostx64/x64");
                    if bin.join("clang_rt.asan_dynamic-x86_64.dll").is_file() {
                        found.push(bin);
                    }
                }
            }
        }
    }
    found.sort();
    found.pop()
}

fn children(dir: &Path) -> Vec<PathBuf> {
    fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .collect()
}

/// A small `level.dat`-like compound, plain and gzipped.
fn nbt_seeds() -> Vec<(&'static str, Vec<u8>)> {
    let root = sample_chunk(3955);
    vec![
        ("seed-plain.nbt", nbt::write("", &root)),
        ("seed-gzip.nbt", nbt::write_compressed("", &root)),
    ]
}

/// A region file with two uncompressed chunks, one external chunk with no `.mcc` file, and
/// empty locations.
fn region_seeds() -> Vec<(&'static str, Vec<u8>)> {
    let mut file = vec![0_u8; 8192];
    let mut sector = 2_u32;
    for (index, compression, body) in [
        (0_usize, 3_u8, nbt::write("", &sample_chunk(3955))),
        (1, 3, nbt::write("", &sample_chunk(4189))),
        (2, 3 | 128, Vec::new()),
    ] {
        let mut chunk = u32::try_from(body.len() + 1)
            .expect("a small chunk")
            .to_be_bytes()
            .to_vec();
        chunk.push(compression);
        chunk.extend(body);
        let sectors = u32::try_from(chunk.len().div_ceil(4096)).expect("a small chunk");
        let entry = (sector << 8) | sectors;
        file[index * 4..index * 4 + 4].copy_from_slice(&entry.to_be_bytes());
        chunk.resize(sectors as usize * 4096, 0);
        file.extend(chunk);
        sector += sectors;
    }
    vec![("seed.mca", file)]
}

/// A compound with the shapes saves use: a data version, a string, a list and arrays.
fn sample_chunk(data_version: i32) -> Tag {
    Tag::Compound(Compound::from([
        ("DataVersion".to_string(), Tag::Int(data_version)),
        ("Status".to_string(), Tag::String("minecraft:full".into())),
        (
            "sections".to_string(),
            Tag::List(vec![Tag::Compound(Compound::from([(
                "Y".to_string(),
                Tag::Byte(-4),
            )]))]),
        ),
        ("Heightmaps".to_string(), Tag::LongArray(vec![0, -1, 7])),
        ("Biomes".to_string(), Tag::IntArray(vec![1, 2, 3])),
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeds_are_valid() {
        for (_, bytes) in nbt_seeds() {
            assert!(nbt::read(&bytes).is_ok() || nbt::read_compressed(&bytes).is_ok());
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("r.0.0.mca");
        fs::write(&path, &region_seeds()[0].1).unwrap();
        let counts = spyglass_world::region::file_versions(&path).unwrap();
        assert_eq!(
            counts.by_version.into_iter().collect::<Vec<_>>(),
            [(3955, 1), (4189, 1)]
        );
        assert_eq!(counts.unreadable, 1);
    }
}
