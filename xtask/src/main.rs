//! Project commands, run as `cargo xtask <command>`.
//! See docs/adr/0003-cargo-xtask-for-project-commands.md.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use spyglass_data::{java, manifest};

mod oracle;

#[derive(Parser)]
#[command(about = "spyglass project commands")]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run every check in .pre-commit-config.yaml against the whole repo, as CI does.
    Check,
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
    /// Check the workspace builds with its minimum supported Rust version (`rust-version` in
    /// Cargo.toml). Needs that toolchain; the setup scripts install it.
    Msrv {
        /// Only print the version, for the setup scripts.
        #[arg(long)]
        print: bool,
    },
    /// Build and run the oracle harness: the real game's answers, for golden files.
    Oracle {
        #[command(subcommand)]
        cmd: oracle::OracleCmd,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Cmd::Check => check(),
        Cmd::Doctor { game_version, java } => doctor(&game_version, java.as_deref()),
        Cmd::JdkStatus { game_version } => jdk_status(&game_version),
        Cmd::Msrv { print } => msrv(print),
        Cmd::Oracle { cmd } => oracle::run(cmd),
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
    let path = repo_root().join("Cargo.toml");
    let text = std::fs::read_to_string(&path)?;
    let mut in_package = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_package = line == "[workspace.package]";
        } else if in_package
            && let Some((key, value)) = line.split_once('=')
            && key.trim() == "rust-version"
        {
            return Ok(value.trim().trim_matches('"').to_string());
        }
    }
    bail!(
        "no rust-version in [workspace.package] of {}",
        path.display()
    )
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

    // Rust: this binary compiled, so the toolchain and linker work.
    let rustc = first_line("rustc", &["--version"]).unwrap_or_else(|| "unknown".into());
    r.line(Status::Ok, "Rust toolchain", &rustc, None);
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

    java_checks(&mut r, game_version, explicit_java);

    println!();
    if r.failures > 0 {
        bail!("{} check(s) failed", r.failures);
    }
    println!("Ready.");
    Ok(())
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
