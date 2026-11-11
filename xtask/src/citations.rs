//! Where spyglass's code cites the game: every backticked `` `Class` ``, `` `Class.method` `` or
//! `` `Outer.Inner.method` `` in a comment under `crates/`. A function that ports a game method
//! opens its doc comment with it (CLAUDE.md), so the citations map the game's classes and methods
//! to the code that ports them, and `version-diff --code` reads them to say which code a version's
//! changes reach (docs/adr/0069-a-checked-crate-order-and-cited-game-methods.md).
//!
//! A citation is a lead, not a proof: a comment can go stale, and a port can lack one.

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};

use crate::{command, repo_root};

/// One citation: where it is, and the method it names, if any.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cite {
    /// The file, relative to the repo root, with `/`.
    pub file: String,
    pub line: usize,
    pub method: Option<String>,
}

impl Cite {
    /// The crate the file is in: the directory under `crates/`.
    pub fn crate_name(&self) -> &str {
        self.file
            .strip_prefix("crates/")
            .and_then(|rest| rest.split('/').next())
            .unwrap_or(&self.file)
    }
}

/// Every citation in the tracked Rust files under `crates/`, by the class it cites as the
/// disassembly names nested classes (`Biome$ClimateSettings`).
pub fn scan() -> Result<BTreeMap<String, Vec<Cite>>> {
    let output = command("git")
        .args(["ls-files", "-z", "--", "crates/*.rs"])
        .output()
        .context("failed to run git")?;
    if !output.status.success() {
        bail!("git ls-files failed");
    }
    let listed = String::from_utf8(output.stdout).context("git listed a non-UTF-8 path")?;
    let mut out: BTreeMap<String, Vec<Cite>> = BTreeMap::new();
    for file in listed.split('\0').filter(|f| !f.is_empty()) {
        let text = std::fs::read_to_string(repo_root().join(file))
            .with_context(|| format!("failed to read {file}"))?;
        for (class, cite) in cites(file, &text) {
            out.entry(class).or_default().push(cite);
        }
    }
    Ok(out)
}

/// The citations in one file's text.
fn cites(file: &str, text: &str) -> Vec<(String, Cite)> {
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let Some((_, comment)) = line.split_once("//") else {
            continue;
        };
        // The odd pieces between backticks are code spans.
        for span in comment.split('`').skip(1).step_by(2) {
            if let Some((class, method)) = cited(span) {
                out.push((
                    class,
                    Cite {
                        file: file.to_string(),
                        line: i + 1,
                        method,
                    },
                ));
            }
        }
    }
    out
}

/// The class and method a code span names: `NoiseSpawnFinder.radialSearch` →
/// (`NoiseSpawnFinder`, `radialSearch`), `Biome.ClimateSettings` → `Biome$ClimateSettings`.
/// Leading parts that start with a capital (and aren't all capitals, as constants are) are the
/// class; the next part, if it starts lowercase, is the method. Anything from the first character
/// that can't be in a name on (`(`, `[`, a space) is ignored.
fn cited(span: &str) -> Option<(String, Option<String>)> {
    let name: String = span
        .chars()
        .take_while(|c| c.is_alphanumeric() || matches!(c, '_' | '$' | '.'))
        .collect();
    let mut classes = Vec::new();
    let mut method = None;
    for part in name.split('.') {
        let first = part.chars().next()?;
        if first.is_uppercase() && part.chars().any(char::is_lowercase) && method.is_none() {
            classes.push(part);
        } else if first.is_lowercase() && !classes.is_empty() && method.is_none() {
            method = Some(part.to_string());
        } else {
            break;
        }
    }
    if classes.is_empty() {
        return None;
    }
    Some((classes.join("$"), method))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_classes_and_methods_from_code_spans() {
        assert_eq!(
            cited("NoiseSpawnFinder.radialSearch"),
            Some(("NoiseSpawnFinder".into(), Some("radialSearch".into())))
        );
        assert_eq!(
            cited("Biome.ClimateSettings"),
            Some(("Biome$ClimateSettings".into(), None))
        );
        assert_eq!(
            cited("Mth.sin(double)"),
            Some(("Mth".into(), Some("sin".into())))
        );
        assert_eq!(cited("Mth.SIN[i]"), Some(("Mth".into(), None)));
        assert_eq!(cited("(int) (Math.sin(angle) * radius)"), None);
        assert_eq!(cited("minecraft:village"), None);
        assert_eq!(cited("f64::sin"), None);
    }

    #[test]
    fn finds_spans_only_in_comments() {
        let text = "\
/// `Mth.lerp3`: three lerps.
fn lerp3() {}
let s = \"`NotAComment.x`\";
    // From `DensityFunctions.Spline`, then `Interval.add`.
";
        let found: Vec<(String, usize, Option<String>)> =
            cites("crates/spyglass-noise/src/mth.rs", text)
                .into_iter()
                .map(|(class, cite)| (class, cite.line, cite.method))
                .collect();
        assert_eq!(
            found,
            [
                ("Mth".into(), 1, Some("lerp3".into())),
                ("DensityFunctions$Spline".into(), 4, None),
                ("Interval".into(), 4, Some("add".into())),
            ]
        );
        let cite = Cite {
            file: "crates/spyglass-noise/src/mth.rs".into(),
            line: 1,
            method: None,
        };
        assert_eq!(cite.crate_name(), "spyglass-noise");
    }
}
