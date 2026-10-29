//! `cargo xtask ci-scope --base <ref>`: which CI jobs a pull request needs, from the files it
//! changes (docs/adr/0041-ci-runs-what-a-change-can-affect.md). It prints `key=true|false` lines
//! for `$GITHUB_OUTPUT`. Pushes to main and the release workflow don't ask: they run everything.

use anyhow::{Result, ensure};

use crate::command;

#[derive(clap::Args)]
pub struct CiScope {
    /// What the pull request merges into, e.g. `origin/main`.
    #[arg(long)]
    base: String,
}

/// The jobs to run.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Scope {
    /// Every hook and test on Linux (`cargo xtask check`).
    check: bool,
    /// The same on Windows.
    windows: bool,
    /// The comparisons with the game, one job per version and part.
    oracle: bool,
    /// Only the commit-stage hooks: links, formatting, spelling. For changes to docs alone.
    docs: bool,
}

impl Scope {
    fn or(self, other: Scope) -> Scope {
        Scope {
            check: self.check || other.check,
            windows: self.windows || other.windows,
            oracle: self.oracle || other.oracle,
            docs: self.docs || other.docs,
        }
    }
}

/// Files outside `crates/` and `oracle/` that the oracle jobs build or run with.
const ORACLE_FILES: &[&str] = &[
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain.toml",
    ".github/workflows/ci.yml",
    "xtask/Cargo.toml",
    "xtask/src/ci.rs",
    "xtask/src/locate.rs",
    "xtask/src/oracle.rs",
    "xtask/src/oracle_test.rs",
];

/// Files that only Linux's check runs: Linux setup, the dev container, other workflows.
const LINUX_ONLY: &[&str] = &[
    "scripts/setup.sh",
    ".github/dependabot.yml",
    ".github/workflows/fuzz.yml",
    ".github/workflows/release.yml",
];

/// What a change to `path` needs.
fn needs(path: &str) -> Scope {
    // Specs are docs, but a test reads their headings (each version switch names its section).
    let specs = path.starts_with("docs/specs/");
    let docs = path.ends_with(".md")
        || path.starts_with("docs/")
        || path == "LICENSE"
        || path.ends_with(".png");
    if docs && !specs {
        return Scope {
            docs: true,
            ..Scope::default()
        };
    }
    let oracle = (path.starts_with("crates/") && !path.starts_with("crates/spyglass-viewer/"))
        || path.starts_with("oracle/")
        || ORACLE_FILES.contains(&path);
    let linux_only = specs || LINUX_ONLY.contains(&path) || path.starts_with(".devcontainer/");
    Scope {
        check: true,
        windows: !linux_only,
        oracle,
        docs: false,
    }
}

/// What changing `files` needs. The docs job runs only when nothing runs the full check, which
/// includes it.
fn scope(files: &[&str]) -> Scope {
    let scope = files
        .iter()
        .map(|path| needs(path))
        .fold(Scope::default(), Scope::or);
    if scope == Scope::default() {
        // Nothing changed that the rules know: check, to be safe.
        return Scope {
            check: true,
            ..Scope::default()
        };
    }
    Scope {
        docs: scope.docs && !scope.check,
        ..scope
    }
}

pub fn run(args: &CiScope) -> Result<()> {
    let out = command("git")
        .args(["diff", "--name-only", &format!("{}...HEAD", args.base)])
        .output()?;
    ensure!(
        out.status.success(),
        "git diff against {} failed: {}",
        args.base,
        String::from_utf8_lossy(&out.stderr).trim()
    );
    let text = String::from_utf8_lossy(&out.stdout);
    let files: Vec<&str> = text.lines().filter(|l| !l.is_empty()).collect();
    let scope = scope(&files);
    eprintln!("{} files changed: {scope:?}", files.len());
    println!(
        "check={}\nwindows={}\noracle={}\ndocs={}",
        scope.check, scope.windows, scope.oracle, scope.docs
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: Scope = Scope {
        check: true,
        windows: true,
        oracle: true,
        docs: false,
    };

    #[test]
    fn docs_alone_get_the_docs_job() {
        let docs = Scope {
            docs: true,
            ..Scope::default()
        };
        assert_eq!(scope(&["README.md", "docs/images/viewer-search.png"]), docs);
        assert_eq!(scope(&["docs/adr/0041-x.md", "CHANGELOG.md"]), docs);
    }

    #[test]
    fn specs_get_the_linux_check() {
        let specs = scope(&["docs/specs/terrain.md"]);
        assert!(specs.check && !specs.windows && !specs.oracle && !specs.docs);
    }

    #[test]
    fn worldgen_gets_everything() {
        assert_eq!(scope(&["crates/spyglass-terrain/src/lib.rs"]), ALL);
        assert_eq!(scope(&["Cargo.lock", "README.md"]), ALL);
        assert_eq!(scope(&["oracle/src/Harness.java"]), ALL);
        assert_eq!(scope(&[".github/workflows/ci.yml"]), ALL);
        assert_eq!(scope(&["xtask/src/oracle.rs"]), ALL);
    }

    #[test]
    fn the_viewer_and_tooling_skip_the_oracle() {
        let both = Scope {
            check: true,
            windows: true,
            ..Scope::default()
        };
        assert_eq!(scope(&["crates/spyglass-viewer/src/app.rs"]), both);
        assert_eq!(scope(&["xtask/src/dist.rs", "scripts/setup.ps1"]), both);
    }

    #[test]
    fn linux_only_files_skip_windows() {
        let linux = Scope {
            check: true,
            ..Scope::default()
        };
        assert_eq!(scope(&["scripts/setup.sh"]), linux);
        assert_eq!(
            scope(&[".devcontainer/Dockerfile", ".github/dependabot.yml"]),
            linux
        );
        assert_eq!(scope(&[".github/workflows/release.yml"]), linux);
    }

    #[test]
    fn nothing_changed_still_checks() {
        assert_eq!(
            scope(&[]),
            Scope {
                check: true,
                ..Scope::default()
            }
        );
    }
}
