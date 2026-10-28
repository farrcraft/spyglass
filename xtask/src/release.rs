//! `cargo xtask release <version>` and `cargo xtask release-tag <version>`: a release goes
//! through a pull request like any change (docs/adr/0040-releases-go-through-a-pull-request.md).
//! docs/releasing.md has the whole process.
//!
//! `release` runs every check on `main`, then makes a `release/v<version>` branch, sets the
//! workspace version, turns the changelog's Unreleased section into the release's with the game
//! versions this build was checked against, and commits. A pre-release (`0.1.0-rc.1`) leaves the
//! changelog alone: its notes are Unreleased. Once the pull request is merged, `release-tag` tags
//! the merged release commit on `main`. Neither pushes.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail, ensure};
use spyglass_data::versions::CHECKED;

use crate::{command, dist::civil_date, repo_root, toml_string};

/// Where the changelog's links point.
const REPO: &str = "https://github.com/farrcraft/spyglass";

#[derive(clap::Args)]
pub struct Release {
    /// The version to release, e.g. `0.1.0`, or a pre-release such as `0.1.0-rc.1`.
    version: String,
}

/// `git` with `args`, from the repo root: its trimmed stdout, or an error if it fails.
fn git(args: &[&str]) -> Result<String> {
    let out = command("git")
        .args(args)
        .output()
        .context("failed to run git")?;
    ensure!(
        out.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&out.stderr).trim()
    );
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// The release branch for `version`.
fn branch(version: &Version) -> String {
    format!("release/v{version}")
}

pub fn run(args: &Release) -> Result<()> {
    let version = Version::parse(&args.version)?;
    let current = toml_string("Cargo.toml", &["workspace", "package", "version"])?;
    let current = Version::parse(&current)?;
    ensure!(
        version.core > current.core || (version.core == current.core && current.pre.is_some()),
        "{version} isn't after the current version, {current}"
    );
    ensure!(
        git(&["status", "--porcelain"])?.is_empty(),
        "the tree has uncommitted changes"
    );
    ensure!(
        git(&["rev-parse", "--abbrev-ref", "HEAD"])? == "main",
        "start a release from main"
    );
    git(&["fetch", "--quiet", "origin", "main"])?;
    ensure!(
        git(&["rev-parse", "HEAD"])? == git(&["rev-parse", "origin/main"])?,
        "main isn't the same as origin/main; pull (or push) first"
    );
    let tag = format!("v{version}");
    ensure!(
        git(&["tag", "--list", &tag])?.is_empty(),
        "the tag {tag} already exists"
    );
    let branch = branch(&version);
    ensure!(
        git(&["branch", "--list", &branch])?.is_empty(),
        "the branch {branch} already exists"
    );

    let changelog_path = repo_root().join("CHANGELOG.md");
    let changelog = std::fs::read_to_string(&changelog_path)?.replace("\r\n", "\n");
    ensure!(
        unreleased(&changelog).is_some_and(|text| !text.trim().is_empty()),
        "CHANGELOG.md's Unreleased section is empty; a release needs something in it"
    );

    // Before the version changes: a new version makes every workspace crate stale, xtask among
    // them, and a hook rebuilding the running xtask.exe fails on Windows. What changes after is
    // only the version and the changelog, which the commit's hooks and the pull request's CI check.
    println!("Running every check on main, as CI does...");
    if let Err(err) = crate::check() {
        bail!("{err:#}\nNothing has changed. Fix main through a pull request, then try again.");
    }

    git(&["switch", "--quiet", "--create", &branch])?;
    if version.pre.is_none() {
        let checked: Vec<&str> = CHECKED.iter().map(|c| c.id).collect();
        let released = release_changelog(&changelog, &version.to_string(), &today(), &checked)?;
        std::fs::write(&changelog_path, released)?;
    }
    let manifest_path = repo_root().join("Cargo.toml");
    let manifest = std::fs::read_to_string(&manifest_path)?;
    std::fs::write(
        &manifest_path,
        set_version(&manifest, &version.to_string())?,
    )?;
    // Only the workspace's own entries change in Cargo.lock.
    let status = command("cargo")
        .args(["update", "--workspace", "--quiet"])
        .status()?;
    ensure!(status.success(), "cargo update --workspace failed");

    if let Err(err) = git(&[
        "commit",
        "--all",
        "--quiet",
        "-m",
        &format!("Release {version}"),
    ]) {
        bail!(
            "{err:#}\nThe version and changelog changes are left uncommitted on {branch}. Fix what \
             the commit's hooks found and commit there, or start again: `git checkout . && git \
             switch main && git branch -D {branch}`."
        );
    }
    println!(
        "Committed `Release {version}` on {branch}. Nothing is pushed. Next:\n\n    \
         git push --set-upstream origin {branch}\n    \
         gh pr create --fill\n\n\
         Once the pull request is merged into main:\n\n    \
         cargo xtask release-tag {version}\n\n\
         (docs/releasing.md)"
    );
    Ok(())
}

#[derive(clap::Args)]
pub struct ReleaseTag {
    /// The version whose merged release commit to tag, e.g. `0.1.0`.
    version: String,
}

/// Tags the commit on origin/main that set the workspace version to the release's: the merged
/// release pull request. Never pushes.
pub fn tag(args: &ReleaseTag) -> Result<()> {
    let version = Version::parse(&args.version)?;
    let tag = format!("v{version}");
    git(&["fetch", "--quiet", "--tags", "origin", "main"])?;
    ensure!(
        git(&["tag", "--list", &tag])?.is_empty(),
        "the tag {tag} already exists"
    );
    // The newest commit on main that changed the workspace version to this one: the merged
    // release pull request.
    let wanted = version.to_string();
    let version_at = |rev: &str| -> Option<String> {
        manifest_version(&git(&["show", &format!("{rev}:Cargo.toml")]).ok()?)
    };
    let commit = git(&["log", "origin/main", "--format=%H", "--", "Cargo.toml"])?
        .lines()
        .find(|commit| {
            version_at(commit).as_deref() == Some(wanted.as_str())
                && version_at(&format!("{commit}^")).as_deref() != Some(wanted.as_str())
        })
        .map(str::to_string)
        .with_context(|| {
            format!(
                "no commit on origin/main sets the version to {version}; is the release pull \
                 request merged?"
            )
        })?;
    let later = git(&["rev-list", "--count", &format!("{commit}..origin/main")])?;
    if later != "0" {
        eprintln!(
            "note: {later} commit(s) merged into main after the release; the tag is on the \
             release commit itself"
        );
    }
    git(&[
        "tag",
        "--annotate",
        &tag,
        &commit,
        "-m",
        &format!("spyglass {version}"),
    ])?;
    let subject = git(&["log", "-1", "--format=%h %s", &commit])?;
    println!(
        "Tagged {tag} on {subject}. Nothing is pushed. To release:\n\n    \
         git push origin {tag}\n\n\
         The release workflow then builds a draft release to publish (docs/releasing.md)."
    );
    Ok(())
}

/// `[workspace.package]`'s version in the text of a `Cargo.toml`.
fn manifest_version(manifest: &str) -> Option<String> {
    let mut in_package = false;
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_package = line == "[workspace.package]";
        } else if in_package && let Some(value) = line.strip_prefix("version = ") {
            return Some(value.trim_matches('"').to_string());
        }
    }
    None
}

/// A SemVer version: major, minor and patch, and an optional pre-release.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Version {
    core: (u64, u64, u64),
    pre: Option<String>,
}

impl Version {
    fn parse(text: &str) -> Result<Self> {
        let (core, pre) = match text.split_once('-') {
            Some((core, pre)) => (core, Some(pre.to_string())),
            None => (text, None),
        };
        let parts: Vec<u64> = core
            .split('.')
            .map(str::parse)
            .collect::<Result<_, _>>()
            .with_context(|| format!("{text:?} isn't a version like 0.1.0"))?;
        let [major, minor, patch] = parts[..] else {
            bail!("{text:?} isn't a version like 0.1.0");
        };
        ensure!(
            pre.as_deref()
                .is_none_or(|pre| !pre.is_empty()
                    && pre.chars().all(|c| c.is_ascii_alphanumeric() || c == '.')),
            "{text:?} has a pre-release that isn't letters, digits and dots"
        );
        Ok(Version {
            core: (major, minor, patch),
            pre,
        })
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (major, minor, patch) = self.core;
        write!(f, "{major}.{minor}.{patch}")?;
        if let Some(pre) = &self.pre {
            write!(f, "-{pre}")?;
        }
        Ok(())
    }
}

/// Today's date (UTC), `YYYY-MM-DD`.
fn today() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let (year, month, day) = civil_date(seconds / 86_400);
    format!("{year}-{month:02}-{day:02}")
}

const UNRELEASED: &str = "## [Unreleased]\n";

/// The text of the changelog's Unreleased section, after its heading and before the next.
fn unreleased(changelog: &str) -> Option<&str> {
    section(changelog, "Unreleased")
}

/// The text of the changelog's section `name` (a version, or `Unreleased`): after its heading,
/// up to the next section or the link definitions.
fn section<'a>(changelog: &'a str, name: &str) -> Option<&'a str> {
    let heading = format!("## [{name}]");
    let start = changelog
        .match_indices(&heading)
        .map(|(at, _)| at)
        .find(|&at| at == 0 || changelog[..at].ends_with('\n'))?;
    let rest = &changelog[start..];
    let rest = &rest[rest.find('\n')? + 1..];
    let end = rest
        .find("\n## ")
        .or_else(|| rest.find("\n["))
        .unwrap_or(rest.len());
    Some(&rest[..end])
}

#[derive(clap::Args)]
pub struct ReleaseNotes {
    /// The release's tag, e.g. `v0.1.0`.
    #[arg(long)]
    tag: String,
    /// Where to write the notes.
    #[arg(long)]
    out: PathBuf,
}

/// Checks a pushed tag and writes its release notes: the release's changelog section, or
/// Unreleased for a pre-release, with relative links made absolute to the tag.
pub fn notes(args: &ReleaseNotes) -> Result<()> {
    let tag = &args.tag;
    let version = tag
        .strip_prefix('v')
        .with_context(|| format!("the tag {tag:?} isn't v<version>"))?;
    let parsed = Version::parse(version)?;
    let workspace = toml_string("Cargo.toml", &["workspace", "package", "version"])?;
    ensure!(
        workspace == version,
        "the tag {tag} doesn't match the workspace version, {workspace}"
    );
    let on_main = command("git")
        .args(["merge-base", "--is-ancestor", "HEAD", "origin/main"])
        .status()
        .context("failed to run git")?;
    ensure!(on_main.success(), "{tag}'s commit isn't on origin/main");
    let changelog =
        std::fs::read_to_string(repo_root().join("CHANGELOG.md"))?.replace("\r\n", "\n");
    let text = match parsed.pre {
        Some(_) => unreleased(&changelog),
        None => section(&changelog, version),
    }
    .filter(|text| !text.trim().is_empty())
    .with_context(|| format!("CHANGELOG.md has nothing for {version}"))?;
    std::fs::write(&args.out, format!("{}\n", absolute_links(text.trim(), tag)))?;
    Ok(())
}

/// `text` with each relative Markdown link target made a link to the file at `tag` on GitHub,
/// since a release page resolves nothing relative.
fn absolute_links(text: &str, tag: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("](") {
        let (before, after) = rest.split_at(at + 2);
        out.push_str(before);
        let end = after.find(')').unwrap_or(after.len());
        let target = &after[..end];
        let absolute =
            target.contains("://") || target.starts_with('#') || target.starts_with("mailto:");
        if !absolute {
            out.push_str(&format!("{REPO}/blob/{tag}/"));
        }
        out.push_str(target);
        rest = &after[end..];
    }
    out.push_str(rest);
    out
}

/// The changelog with Unreleased become `version`'s section, dated `date` and listing the game
/// versions `checked`, a new empty Unreleased above it, and the link definitions at the end
/// updated.
fn release_changelog(
    changelog: &str,
    version: &str,
    date: &str,
    checked: &[&str],
) -> Result<String> {
    let previous = changelog
        .lines()
        .filter_map(|line| line.strip_prefix("## ["))
        .filter_map(|line| line.split_once(']').map(|(v, _)| v))
        .find(|v| *v != "Unreleased")
        .map(str::to_string);
    let heading = format!(
        "{UNRELEASED}\n## [{version}] - {date}\n\n### Game versions\n\nChecked against the game for {}.\n",
        checked.join(", ")
    );
    ensure!(
        changelog.contains(UNRELEASED),
        "CHANGELOG.md has no Unreleased section"
    );
    let body = changelog.replacen(UNRELEASED, &heading, 1);
    // Link definitions: everything from the first `[Unreleased]: ` on is rewritten.
    let body = match body.find("\n[Unreleased]: ") {
        Some(at) => body[..=at].to_string(),
        None => format!("{}\n", body.trim_end()),
    };
    let mut links = format!("\n[Unreleased]: {REPO}/compare/v{version}...HEAD\n");
    links.push_str(&match &previous {
        Some(previous) => format!("[{version}]: {REPO}/compare/v{previous}...v{version}\n"),
        None => format!("[{version}]: {REPO}/releases/tag/v{version}\n"),
    });
    // Older releases' definitions, after the old `[Unreleased]: `, stay as they were.
    if let Some(at) = changelog.find("\n[Unreleased]: ") {
        for line in changelog[at + 1..].lines().skip(1) {
            links.push_str(line);
            links.push('\n');
        }
    }
    Ok(format!("{body}{links}"))
}

/// `Cargo.toml` with `[workspace.package]`'s version set to `version`.
fn set_version(manifest: &str, version: &str) -> Result<String> {
    let mut out = String::with_capacity(manifest.len());
    let mut in_package = false;
    let mut done = false;
    for line in manifest.split_inclusive('\n') {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_package = trimmed == "[workspace.package]";
        }
        if in_package && !done && trimmed.starts_with("version = ") {
            let ending = if line.ends_with("\r\n") { "\r\n" } else { "\n" };
            out.push_str(&format!("version = \"{version}\"{ending}"));
            done = true;
        } else {
            out.push_str(line);
        }
    }
    ensure!(done, "no version in Cargo.toml's [workspace.package]");
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIRST: &str = "# Changelog\n\nIntro.\n\n## [Unreleased]\n\nThe first release.\n\n### Added\n\n- Everything.\n";

    #[test]
    fn versions() {
        let v = Version::parse("0.1.0").unwrap();
        assert_eq!((v.core, v.pre.as_deref()), ((0, 1, 0), None));
        let rc = Version::parse("0.1.0-rc.1").unwrap();
        assert_eq!((rc.core, rc.pre.as_deref()), ((0, 1, 0), Some("rc.1")));
        assert_eq!(rc.to_string(), "0.1.0-rc.1");
        for bad in ["0.1", "0.1.0.0", "v0.1.0", "0.1.x", "0.1.0-", "0.1.0-rc 1"] {
            assert!(Version::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_first_release() {
        assert_eq!(
            unreleased(FIRST).unwrap().trim(),
            "The first release.\n\n### Added\n\n- Everything."
        );
        let out = release_changelog(FIRST, "0.1.0", "2026-11-02", &["26.2", "26.3"]).unwrap();
        assert_eq!(
            out,
            "# Changelog\n\nIntro.\n\n## [Unreleased]\n\n## [0.1.0] - 2026-11-02\n\n\
             ### Game versions\n\nChecked against the game for 26.2, 26.3.\n\n\
             The first release.\n\n### Added\n\n- Everything.\n\n\
             [Unreleased]: https://github.com/farrcraft/spyglass/compare/v0.1.0...HEAD\n\
             [0.1.0]: https://github.com/farrcraft/spyglass/releases/tag/v0.1.0\n"
        );
        assert_eq!(unreleased(&out).unwrap().trim(), "");
    }

    #[test]
    fn a_later_release() {
        let first = release_changelog(FIRST, "0.1.0", "2026-11-02", &["26.3"]).unwrap();
        let next = first.replacen(UNRELEASED, "## [Unreleased]\n\n### Fixed\n\n- A bug.\n", 1);
        let out = release_changelog(&next, "0.2.0", "2026-12-01", &["26.3", "26.4"]).unwrap();
        assert!(out.contains("## [0.2.0] - 2026-12-01\n\n### Game versions\n\nChecked against the game for 26.3, 26.4.\n\n### Fixed\n\n- A bug.\n\n## [0.1.0] - 2026-11-02"), "{out}");
        assert!(
            out.ends_with(
                "[Unreleased]: https://github.com/farrcraft/spyglass/compare/v0.2.0...HEAD\n\
             [0.2.0]: https://github.com/farrcraft/spyglass/compare/v0.1.0...v0.2.0\n\
             [0.1.0]: https://github.com/farrcraft/spyglass/releases/tag/v0.1.0\n"
            ),
            "{out}"
        );
        assert_eq!(unreleased(&next).unwrap().trim(), "### Fixed\n\n- A bug.");
    }

    #[test]
    fn sections() {
        let changelog = "## [Unreleased]\n\n- Next.\n\n## [0.1.0-rc.1] - x\n\n- Pre.\n\n\
                         ## [0.1.0] - 2026-11-02\n\n- First.\n\n[Unreleased]: x\n";
        assert_eq!(section(changelog, "0.1.0").unwrap().trim(), "- First.");
        assert_eq!(section(changelog, "Unreleased").unwrap().trim(), "- Next.");
        assert_eq!(section(changelog, "0.2.0"), None);
    }

    #[test]
    fn links_become_absolute() {
        assert_eq!(
            absolute_links(
                "See [the spec](docs/specs/cli-json.md#warnings), [SemVer](https://semver.org/) \
                 and [below](#added).",
                "v0.1.0"
            ),
            "See [the spec](https://github.com/farrcraft/spyglass/blob/v0.1.0/docs/specs/\
             cli-json.md#warnings), [SemVer](https://semver.org/) and [below](#added)."
        );
    }

    #[test]
    fn manifest_versions() {
        let manifest = "[workspace]\nmembers = []\n\n[workspace.package]\nversion = \"0.1.0\"\n\n\
                        [workspace.dependencies]\nfoo = { version = \"1.0\" }\n";
        assert_eq!(manifest_version(manifest).as_deref(), Some("0.1.0"));
        assert_eq!(manifest_version("[package]\nversion = \"1.0.0\"\n"), None);
    }

    #[test]
    fn the_workspace_version_alone_changes() {
        let manifest = "[workspace]\nmembers = []\n\n[workspace.package]\nversion = \"0.0.0\"\nedition = \"2024\"\n\n[workspace.dependencies]\nfoo = { version = \"1.0\" }\n";
        let out = set_version(manifest, "0.1.0").unwrap();
        assert_eq!(
            out,
            manifest.replace("version = \"0.0.0\"", "version = \"0.1.0\"")
        );
        assert!(set_version("[package]\nversion = \"1\"\n", "0.1.0").is_err());
    }
}
