//! Project commands, run as `cargo xtask <command>`.
//! See docs/adr/0003-cargo-xtask-for-project-commands.md.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use spyglass_extract::{java, manifest};

mod ci;
mod code_diff;
mod dist;
mod fuzz;
mod links;
mod linux_check;
mod locate;
mod oracle;
mod oracle_test;
mod release;
mod version_diff;

#[derive(Parser)]
#[command(about = "spyglass project commands")]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Time the public API's main operations in a `dist` build (`crates/spyglass/examples/bench.rs`).
    Bench {
        /// The bundle to use: a directory or a version in `bundles/` [default: the newest release].
        bundle: Option<PathBuf>,
    },
    /// Run every check in .pre-commit-config.yaml against the whole repo, as CI does.
    Check,
    /// Run one game version's CI job: extract its bundle, regenerate every oracle output
    /// (goldens, locate and, where it loads, the test pack), and run every comparison.
    Ci(ci::Ci),
    /// Build one target's release archive into `target/archives/`: the binaries in a `dist`
    /// build, the docs and the third-party licenses (docs/adr/0038-how-release-archives-are-built.md).
    Dist(dist::Dist),
    /// Fuzz a reader of untrusted files with cargo-fuzz on nightly: `nbt` or `region`.
    Fuzz(fuzz::Fuzz),
    /// Report whether this machine is ready to work on spyglass. Never installs anything.
    Doctor {
        /// Game version whose Java requirement to check: an id, `latest` or `latest-snapshot`.
        #[arg(long, default_value = "latest")]
        game_version: String,
        /// Java home or executable to check first.
        #[arg(long)]
        java: Option<PathBuf>,
    },
    /// Print the JDK major version the game version needs, and `found` or `missing`. Used by the
    /// setup scripts' oracle step.
    JdkStatus {
        #[arg(long, default_value = "latest")]
        game_version: String,
    },
    /// Check a fresh clone, scripts/setup.sh and the extractor on a clean Ubuntu machine, in Docker.
    LinuxCheck(linux_check::LinuxCheck),
    /// Check that every relative link and anchor in the repo's Markdown resolves.
    Links,
    /// Check the workspace builds with its minimum supported Rust version (`rust-version` in
    /// Cargo.toml). Needs that toolchain; the setup scripts install it.
    Msrv {
        /// Only print the version, for the setup scripts.
        #[arg(long)]
        print: bool,
    },
    /// Run the ignored tests that compare spyglass with the game, in release, for which this
    /// machine has the bundles and oracle outputs, once per game version with a bundle. Run it
    /// before completing a plan.
    OracleTest {
        /// Check only this game version.
        #[arg(long)]
        game_version: Option<String>,
    },
    /// Build and run the oracle harness: the real game's answers, for golden files.
    Oracle {
        #[command(subcommand)]
        cmd: oracle::OracleCmd,
    },
    /// Prepare a release: set the version, date the changelog, run every check, then commit and
    /// tag. Never pushes (docs/adr/0037-versions-and-releases.md).
    Release(release::Release),
    /// Report what changed between two bundles in the data spyglass reads: ids added and
    /// removed, files changed, and new or gone JSON shapes (types, fields, value kinds).
    VersionDiff(version_diff::VersionDiff),
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Cmd::Bench { bundle } => bench(bundle.as_deref()),
        Cmd::Check => check(),
        Cmd::Ci(args) => ci::run(&args),
        Cmd::Dist(args) => dist::run(&args),
        Cmd::Doctor { game_version, java } => doctor(&game_version, java.as_deref()),
        Cmd::Fuzz(args) => fuzz::run(&args),
        Cmd::JdkStatus { game_version } => jdk_status(&game_version),
        Cmd::LinuxCheck(args) => linux_check::run(&args),
        Cmd::Links => links::run(),
        Cmd::Msrv { print } => msrv(print),
        Cmd::Oracle { cmd } => oracle::run(cmd),
        Cmd::OracleTest { game_version } => oracle_test::run(game_version.as_deref()),
        Cmd::Release(args) => release::run(&args),
        Cmd::VersionDiff(args) => version_diff::run(&args),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives one level below the repo root")
}

/// A command run from the repo root, without the variables `cargo run` set for xtask itself.
///
/// Those variables (`CARGO_PKG_NAME`, `CARGO_MANIFEST_DIR`, ...) describe xtask, not whatever we
/// run. Some dependencies' build scripts (ring's, for one) rerun when they change, so a nested
/// `cargo` that inherits them rebuilds those crates, and then relinks xtask while it's running,
/// which fails on Windows.
fn command(program: &str) -> Command {
    let mut cmd = Command::new(program);
    cmd.current_dir(repo_root());
    for (key, _) in std::env::vars_os() {
        let Some(key) = key.to_str() else { continue };
        let set_by_cargo_run = [
            "CARGO_PKG_",
            "CARGO_MANIFEST_",
            "CARGO_CRATE_",
            "CARGO_BIN_",
        ]
        .iter()
        .any(|prefix| key.starts_with(prefix))
            || matches!(key, "CARGO_PRIMARY_PACKAGE" | "OUT_DIR");
        if set_by_cargo_run {
            cmd.env_remove(key);
        }
    }
    cmd
}

fn bench(bundle: Option<&Path>) -> Result<()> {
    let mut cmd = command("cargo");
    cmd.args([
        "run",
        "--quiet",
        "--profile",
        "dist",
        "-p",
        "spyglass",
        "--example",
        "bench",
    ]);
    if let Some(bundle) = bundle {
        cmd.arg("--").arg(bundle);
    }
    if !cmd.status().context("failed to run cargo")?.success() {
        bail!("the benchmark failed");
    }
    Ok(())
}

/// First line of a command's stdout, or `None` if it can't run or fails.
fn first_line(program: &str, args: &[&str]) -> Option<String> {
    let out = command(program).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    Some(stdout.lines().next().unwrap_or_default().trim().to_string())
}

/// pre-commit, or prek, which reads the same config. See docs/adr/0002-git-hooks-for-local-checks.md.
fn hook_runner() -> Option<(&'static str, String)> {
    ["pre-commit", "prek"]
        .into_iter()
        .find_map(|name| first_line(name, &["--version"]).map(|v| (name, v)))
}

fn check() -> Result<()> {
    let Some((runner, _)) = hook_runner() else {
        bail!(
            "neither pre-commit nor prek is installed; run scripts/setup.ps1 or scripts/setup.sh"
        );
    };
    for stage in ["pre-commit", "pre-push"] {
        let status = command(runner)
            .args(["run", "--all-files", "--hook-stage", stage])
            .status()
            .with_context(|| format!("failed to run {runner}"))?;
        if !status.success() {
            bail!("{stage} checks failed");
        }
    }
    Ok(())
}

/// `rust-version` from the workspace Cargo.toml's `[workspace.package]`.
fn msrv_version() -> Result<String> {
    toml_string("Cargo.toml", &["workspace", "package", "rust-version"])
}

/// The string at `keys` (`["toolchain", "channel"]`) in the TOML file `file` in the repo root.
fn toml_string(file: &str, keys: &[&str]) -> Result<String> {
    let path = repo_root().join(file);
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let document: toml_edit::Document<String> = toml_edit::Document::parse(text)
        .with_context(|| format!("{} isn't valid TOML", path.display()))?;
    let mut item = document.as_item();
    for key in keys {
        item = item
            .get(key)
            .with_context(|| format!("no {} in {}", keys.join("."), path.display()))?;
    }
    item.as_str()
        .map(str::to_string)
        .with_context(|| format!("{} in {} isn't a string", keys.join("."), path.display()))
}

/// Whether rustup has a toolchain for `version`, e.g. `1.88` matches `1.88-x86_64-pc-windows-msvc`.
fn has_toolchain(version: &str) -> bool {
    let Ok(out) = command("rustup").args(["toolchain", "list"]).output() else {
        return false;
    };
    String::from_utf8_lossy(&out.stdout).lines().any(|l| {
        l.split_whitespace()
            .next()
            .is_some_and(|t| t == version || t.starts_with(&format!("{version}-")))
    })
}

fn msrv(print: bool) -> Result<()> {
    let version = msrv_version()?;
    if print {
        println!("{version}");
        return Ok(());
    }
    if !has_toolchain(&version) {
        bail!(
            "Rust {version} isn't installed; run: rustup toolchain install {version} --profile minimal"
        );
    }
    let status = command("cargo")
        .arg(format!("+{version}"))
        .args(["check", "--workspace", "--all-targets", "--quiet"])
        .status()
        .context("failed to run cargo")?;
    if !status.success() {
        bail!("the workspace doesn't build with Rust {version}, its declared rust-version");
    }
    Ok(())
}

fn jdk_status(game_version: &str) -> Result<()> {
    let manifest = manifest::fetch_manifest().context("fetching the version manifest")?;
    let version = manifest::fetch_version(manifest.resolve(game_version)?)?;
    let Some(major) = version.java_version.map(|j| j.major_version) else {
        bail!("{} doesn't state a Java version", version.id);
    };
    let found = java::discover(None)
        .iter()
        .any(|j| j.major == major && j.is_jdk);
    println!("{major} {}", if found { "found" } else { "missing" });
    Ok(())
}

#[derive(Clone, Copy, PartialEq)]
enum Status {
    Ok,
    Warn,
    Fail,
}

struct Report {
    failures: usize,
}

impl Report {
    fn line(&mut self, status: Status, name: &str, detail: &str, fix: Option<&str>) {
        let tag = match status {
            Status::Ok => "[ ok ]",
            Status::Warn => "[warn]",
            Status::Fail => "[FAIL]",
        };
        println!("{tag} {name}: {detail}");
        if let (Some(fix), true) = (fix, status != Status::Ok) {
            println!("       fix: {fix}");
        }
        if status == Status::Fail {
            self.failures += 1;
        }
    }
}

fn doctor(game_version: &str, explicit_java: Option<&Path>) -> Result<()> {
    let mut r = Report { failures: 0 };
    let setup = "run scripts/setup.ps1 (Windows) or scripts/setup.sh";
    rust_checks(&mut r, setup);
    hook_checks(&mut r, setup);
    java_checks(&mut r, game_version, explicit_java);
    bundles_check(&mut r);
    docker_check(&mut r);
    println!();
    if r.failures > 0 {
        bail!("{} check(s) failed", r.failures);
    }
    println!("Ready.");
    Ok(())
}

/// The bundles folder the front ends use by default from here (ADR 0039), and what's in it.
fn bundles_check(r: &mut Report) {
    let dir = spyglass_data::bundle::default_bundles_dir();
    let versions: Vec<String> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| spyglass_data::bundle::read_manifest(&entry.path()).ok())
        .map(|manifest| manifest.version)
        .collect();
    let detail = match versions.as_slice() {
        [] => format!("{}, with no bundles", dir.display()),
        found => format!("{} ({})", dir.display(), found.join(", ")),
    };
    let status = if versions.is_empty() {
        Status::Warn
    } else {
        Status::Ok
    };
    r.line(
        status,
        "bundles",
        &detail,
        Some("cargo run -p spyglass-extract"),
    );
}

/// The toolchain, its components, the cargo tools the hooks need, and the MSRV toolchain.
fn rust_checks(r: &mut Report, setup: &str) {
    // Rust: this binary compiled, so the toolchain and linker work.
    let rustc = first_line("rustc", &["--version"]).unwrap_or_else(|| "unknown".into());
    r.line(Status::Ok, "Rust toolchain", &rustc, None);
    pin_check(r);
    for (component, args) in [
        ("rustfmt", ["fmt", "--version"]),
        ("clippy", ["clippy", "--version"]),
    ] {
        match first_line("cargo", &args) {
            Some(v) => r.line(Status::Ok, component, &v, None),
            None => r.line(Status::Fail, component, "not installed", Some(setup)),
        }
    }

    for (name, subcommand) in [("cargo-nextest", "nextest"), ("cargo-shear", "shear")] {
        match first_line("cargo", &[subcommand, "--version"]) {
            Some(v) => r.line(Status::Ok, name, &v, None),
            None => r.line(
                Status::Fail,
                name,
                "not installed; the git hooks need it",
                Some(setup),
            ),
        }
    }
    // Only `cargo xtask dist` needs it, so it's never a failure.
    match first_line("cargo", &["about", "--version"]) {
        Some(v) => r.line(Status::Ok, "cargo-about", &v, None),
        None => r.line(
            Status::Warn,
            "cargo-about",
            "not installed; only release archives need it",
            Some("run scripts/setup.ps1 -ReleaseTools or scripts/setup.sh --release-tools"),
        ),
    }

    match msrv_version() {
        Ok(version) if has_toolchain(&version) => {
            r.line(
                Status::Ok,
                "MSRV toolchain",
                &format!("Rust {version} installed"),
                None,
            );
        }
        Ok(version) => {
            let fix = format!("rustup toolchain install {version} --profile minimal");
            let detail =
                format!("Rust {version} (rust-version) not installed; the pre-push hook needs it");
            r.line(Status::Fail, "MSRV toolchain", &detail, Some(&fix));
        }
        Err(err) => r.line(Status::Fail, "MSRV toolchain", &format!("{err:#}"), None),
    }
}

/// Whether a `stable` toolchain newer than rust-toolchain.toml's pin is installed: the pin is
/// bumped deliberately, so doctor says when there's something to bump to (ADR 0034). Offline,
/// it knows only what rustup has installed.
fn pin_check(r: &mut Report) {
    let pinned = match toml_string("rust-toolchain.toml", &["toolchain", "channel"]) {
        Ok(pinned) => pinned,
        Err(err) => return r.line(Status::Fail, "toolchain pin", &format!("{err:#}"), None),
    };
    let stable = first_line("rustc", &["+stable", "--version"])
        .and_then(|line| line.split_whitespace().nth(1).map(str::to_string));
    match stable.zip(version_triple(&pinned)) {
        Some((stable, pin)) if version_triple(&stable).is_some_and(|s| s > pin) => r.line(
            Status::Warn,
            "toolchain pin",
            &format!("rust-toolchain.toml pins {pinned}; Rust {stable} is installed"),
            Some("bump channel in rust-toolchain.toml, then fix what clippy finds"),
        ),
        _ => r.line(
            Status::Ok,
            "toolchain pin",
            &format!("{pinned} (rust-toolchain.toml)"),
            None,
        ),
    }
}

/// `1.99.1` as numbers, for ordering versions.
fn version_triple(version: &str) -> Option<(u32, u32, u32)> {
    let mut parts = version.split('.').map(|p| p.parse::<u32>().ok());
    Some((
        parts.next()??,
        parts.next()??,
        parts.next().flatten().unwrap_or(0),
    ))
}

/// The hook runner, and whether the hooks are installed.
fn hook_checks(r: &mut Report, setup: &str) {
    match hook_runner() {
        Some((name, version)) => r.line(
            Status::Ok,
            "hook runner",
            &format!("{name} ({version})"),
            None,
        ),
        None => r.line(
            Status::Fail,
            "hook runner",
            "neither pre-commit nor prek found",
            Some(setup),
        ),
    }

    match first_line("git", &["rev-parse", "--git-path", "hooks"]) {
        Some(dir) => {
            let dir = repo_root().join(dir);
            let missing: Vec<_> = ["pre-commit", "pre-push"]
                .into_iter()
                .filter(|hook| !dir.join(hook).is_file())
                .collect();
            if missing.is_empty() {
                r.line(
                    Status::Ok,
                    "git hooks",
                    "pre-commit and pre-push installed",
                    None,
                );
            } else {
                let detail = format!("missing: {}", missing.join(", "));
                r.line(
                    Status::Fail,
                    "git hooks",
                    &detail,
                    Some("pre-commit install"),
                );
            }
        }
        None => r.line(
            Status::Fail,
            "git hooks",
            "not a git repository, or git not found",
            None,
        ),
    }
}

/// Docker, which only `cargo xtask linux-check` and the dev container need.
fn docker_check(r: &mut Report) {
    // Only cargo xtask linux-check and the dev container need Docker, so this never fails.
    match first_line("docker", &["version", "--format", "{{.Server.Version}}"]) {
        Some(v) => r.line(Status::Ok, "Docker", &format!("server {v}"), None),
        None => r.line(
            Status::Warn,
            "Docker",
            "not found, or its daemon isn't running; only needed for cargo xtask linux-check and the dev container",
            Some("install Docker Desktop (Windows, macOS) or Docker Engine (Linux)"),
        ),
    }
}

fn java_checks(r: &mut Report, game_version: &str, explicit: Option<&Path>) {
    let required = manifest::fetch_manifest()
        .and_then(|m| manifest::fetch_version(m.resolve(game_version)?))
        .map(|v| (v.id, v.java_version.map(|j| j.major_version)));
    let found = java::discover(explicit);
    let list = || {
        if found.is_empty() {
            "none".to_string()
        } else {
            found
                .iter()
                .map(|j| format!("{} ({})", j.version, j.source))
                .collect::<Vec<_>>()
                .join(", ")
        }
    };

    let (id, major) = match required {
        Ok((id, Some(major))) => (id, major),
        Ok((id, None)) => {
            let detail = format!("{id} doesn't state a Java version; found: {}", list());
            r.line(Status::Warn, "Java runtime", &detail, None);
            return;
        }
        Err(err @ manifest::ManifestError::UnknownVersion(_)) => {
            r.line(Status::Fail, "Java runtime", &err.to_string(), None);
            return;
        }
        // Network trouble shouldn't fail an otherwise healthy machine.
        Err(err) => {
            let detail = format!("couldn't look up {game_version} ({err}); found: {}", list());
            r.line(Status::Warn, "Java runtime", &detail, None);
            return;
        }
    };

    let name = format!("Java runtime for {id}");
    match found.iter().find(|j| j.major == major) {
        Some(j) => {
            let detail = format!(
                "Java {} from {} at {}",
                j.version,
                j.source,
                j.home.display()
            );
            r.line(Status::Ok, &name, &detail, None);
        }
        None => {
            let fix = format!(
                "play {id} once in your launcher (it downloads the runtime), or install a Java {major} runtime, e.g. Eclipse Temurin"
            );
            // Newer Java usually runs older game versions, but the launcher pins an exact major.
            match found
                .iter()
                .filter(|j| j.major > major)
                .min_by_key(|j| j.major)
            {
                Some(j) => {
                    let detail = format!(
                        "needs Java {major}; only newer found: Java {} from {}",
                        j.version, j.source
                    );
                    r.line(Status::Warn, &name, &detail, Some(&fix));
                }
                None => {
                    let detail = format!("needs Java {major}; found: {}", list());
                    r.line(Status::Fail, &name, &detail, Some(&fix));
                }
            }
        }
    }

    // A JDK is only needed for oracle work, so this never fails.
    match found.iter().find(|j| j.major == major && j.is_jdk) {
        Some(j) => {
            let detail = format!(
                "JDK {} from {} at {}",
                j.version,
                j.source,
                j.home.display()
            );
            r.line(Status::Ok, "JDK (oracle work)", &detail, None);
        }
        None => {
            let detail = format!("no JDK {major} found; only needed for oracle work");
            r.line(
                Status::Warn,
                "JDK (oracle work)",
                &detail,
                Some(&format!(
                    "scripts/setup.ps1 -Oracle (Windows) or scripts/setup.sh --oracle installs a JDK {major}"
                )),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_order_numerically() {
        assert_eq!(version_triple("1.99.1"), Some((1, 99, 1)));
        assert_eq!(version_triple("1.95"), Some((1, 95, 0)));
        assert!(version_triple("1.100.0") > version_triple("1.99.1"));
        assert_eq!(version_triple("stable"), None);
        assert_eq!(version_triple("1.x"), None);
    }
}
