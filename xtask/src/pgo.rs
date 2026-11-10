//! Profile-guided `dist` builds, for this machine only (docs/adr/0064-build-level-speedups.md).
//!
//! The bench is built instrumented, run once to count where the time goes, and its counts are
//! merged with `llvm-profdata` from rustup's `llvm-tools` component. The profile then feeds a
//! second build with `-Cprofile-use`. A training run's counts depend on how threads were
//! scheduled, so two profiles of one commit differ and so do the binaries built from them,
//! which is why release archives never use one (ADR 0038).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};

use crate::{command, repo_root};

/// Where the instrumented build, its counts and the merged profile go: apart from `target/dist`,
/// so neither build rebuilds the other.
pub fn dir() -> PathBuf {
    repo_root().join("target").join("pgo")
}

/// Trains a profile on the bench over `bundle` (the bench's default if `None`) and returns its
/// path. `flags` are the rustc flags the profile's build will use besides `-Cprofile-use`, and
/// `target` its triple, if it's given explicitly; the instrumented build uses the same.
pub fn train(bundle: Option<&Path>, flags: &[String], target: Option<&str>) -> Result<PathBuf> {
    let raw = dir().join("raw");
    if raw.exists() {
        std::fs::remove_dir_all(&raw).with_context(|| format!("clearing {}", raw.display()))?;
    }
    let mut instrumented = flags.to_vec();
    instrumented.push(format!("-Cprofile-generate={}", raw.display()));
    let build_dir = dir().join("generate");
    let mut cmd = cargo_dist(&build_dir, &instrumented, target);
    cmd.args(["-p", "spyglass", "--example", "bench"]);
    ensure!(
        cmd.status().context("failed to run cargo")?.success(),
        "the instrumented bench didn't build"
    );

    let mut bench = command(
        &output(&build_dir, target)
            .join("examples")
            .join(exe("bench"))
            .to_string_lossy(),
    );
    bench.args(bundle);
    println!("training: the bench, instrumented (slower than usual)");
    ensure!(
        bench
            .status()
            .context("failed to run the instrumented bench")?
            .success(),
        "the training run failed"
    );

    let profile = dir().join("merged.profdata");
    let merged = command(&llvm_profdata()?.to_string_lossy())
        .arg("merge")
        .arg("-o")
        .arg(&profile)
        .arg(&raw)
        .status()
        .context("failed to run llvm-profdata")?;
    ensure!(merged.success(), "llvm-profdata couldn't merge the counts");
    Ok(profile)
}

/// The rustc flag that builds with `profile`.
pub fn use_flag(profile: &Path) -> String {
    format!("-Cprofile-use={}", profile.display())
}

/// `cargo build --profile dist --locked` into `target_dir` with `flags` for rustc, for `target`
/// if it's given. With `--target`, the flags reach only the target's crates, not build scripts
/// or proc macros.
pub fn cargo_dist(
    target_dir: &Path,
    flags: &[String],
    target: Option<&str>,
) -> std::process::Command {
    let mut cmd = command("cargo");
    cmd.args(["build", "--quiet", "--profile", "dist", "--locked"]);
    cmd.args(target.map(|t| ["--target", t]).into_iter().flatten());
    cmd.env("CARGO_TARGET_DIR", target_dir)
        .env("CARGO_ENCODED_RUSTFLAGS", flags.join("\x1f"))
        .env_remove("RUSTFLAGS");
    cmd
}

/// Where a `dist` build into `target_dir` puts its outputs.
pub fn output(target_dir: &Path, target: Option<&str>) -> PathBuf {
    match target {
        Some(target) => target_dir.join(target).join("dist"),
        None => target_dir.join("dist"),
    }
}

/// `name` as an executable's file name on this machine.
pub fn exe(name: &str) -> String {
    format!("{name}{}", std::env::consts::EXE_SUFFIX)
}

/// `llvm-profdata` from the toolchain's `llvm-tools` component.
fn llvm_profdata() -> Result<PathBuf> {
    let out = command("rustc").args(["--print", "sysroot"]).output()?;
    let sysroot = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
    let host = crate::dist::host_target()?;
    let path = sysroot
        .join("lib")
        .join("rustlib")
        .join(host)
        .join("bin")
        .join(exe("llvm-profdata"));
    ensure!(
        path.is_file(),
        "llvm-profdata isn't installed; run `rustup component add llvm-tools`"
    );
    Ok(path)
}
