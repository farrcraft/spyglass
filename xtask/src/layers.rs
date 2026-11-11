//! The crate order, checked: docs/architecture.md's crate table lists the crates from the bottom
//! layer up, with what each depends on, and a crate may depend only on crates above it in the
//! table. Cargo rejects only cycles, so a dependency that runs the wrong way, or a table that
//! has drifted from the manifests, would otherwise go unnoticed
//! (docs/adr/0069-a-checked-crate-order-and-cited-game-methods.md).
//!
//! The rules are on normal and build dependencies. Dev-dependencies may point anywhere: tests
//! use `spyglass-oracle`, which sits above the crates it tests.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, bail};
use serde_json::Value;

/// The test-support crate, which only tests may use.
const ORACLE: &str = "spyglass-oracle";

/// A row of the crate table: the crate and what its "Depends on" cell names.
#[derive(Debug, PartialEq)]
struct Row {
    name: String,
    depends_on: BTreeSet<String>,
}

/// A workspace crate's dependencies on other workspace crates, by kind.
#[derive(Debug, Default, Clone)]
struct Deps {
    /// Normal and build dependencies: what the rules cover.
    normal: BTreeSet<String>,
    dev: BTreeSet<String>,
}

/// The rows of the table under `## Crates` in `markdown`, in order.
fn table(markdown: &str) -> Result<Vec<Row>> {
    let mut lines = markdown
        .lines()
        .skip_while(|line| line.trim() != "## Crates")
        .skip(1)
        .skip_while(|line| !line.starts_with('|'));
    let header = lines.next().context("no table under `## Crates`")?;
    let columns = cells(header);
    let crate_column = columns
        .iter()
        .position(|c| *c == "Crate")
        .context("the crate table has no Crate column")?;
    let depends_column = columns
        .iter()
        .position(|c| *c == "Depends on")
        .context("the crate table has no Depends on column")?;
    let mut rows = Vec::new();
    // The line after the header is its separator.
    for line in lines.skip(1).take_while(|line| line.starts_with('|')) {
        let row = cells(line);
        let (Some(name), Some(depends_on)) = (row.get(crate_column), row.get(depends_column))
        else {
            bail!("a crate table row has too few cells: {line}");
        };
        rows.push(Row {
            name: name.trim_matches('`').to_string(),
            depends_on: depends_on
                .split(',')
                .map(str::trim)
                .filter(|d| !d.is_empty() && *d != "—")
                .map(full_name)
                .collect(),
        });
    }
    Ok(rows)
}

/// A table row's cells, trimmed.
fn cells(line: &str) -> Vec<&str> {
    line.trim()
        .trim_matches('|')
        .split('|')
        .map(str::trim)
        .collect()
}

/// A crate as the table's "Depends on" cells write it (`rng`, `spyglass-extract`), in full.
fn full_name(short: &str) -> String {
    let short = short.trim_matches('`');
    if short.starts_with("spyglass") {
        short.to_string()
    } else {
        format!("spyglass-{short}")
    }
}

/// Each workspace crate's dependencies on workspace crates, from `cargo metadata --no-deps`.
fn manifests(metadata: &Value) -> Result<BTreeMap<String, Deps>> {
    let packages = metadata["packages"]
        .as_array()
        .context("cargo metadata has no packages")?;
    let names: BTreeSet<&str> = packages.iter().filter_map(|p| p["name"].as_str()).collect();
    let mut crates = BTreeMap::new();
    for package in packages {
        let name = package["name"].as_str().context("a package has no name")?;
        let mut deps = Deps::default();
        for dep in package["dependencies"].as_array().into_iter().flatten() {
            let Some(dep_name) = dep["name"].as_str().filter(|d| names.contains(d)) else {
                continue;
            };
            match dep["kind"].as_str() {
                Some("dev") => deps.dev.insert(dep_name.to_string()),
                _ => deps.normal.insert(dep_name.to_string()),
            };
        }
        crates.insert(name.to_string(), deps);
    }
    Ok(crates)
}

/// Every way `crates` break the table's order or disagree with it, one line each.
fn problems(rows: &[Row], crates: &BTreeMap<String, Deps>) -> Vec<String> {
    let mut problems = Vec::new();
    let layer: BTreeMap<&str, usize> = rows
        .iter()
        .enumerate()
        .map(|(i, row)| (row.name.as_str(), i))
        .collect();
    for row in rows {
        if !crates.contains_key(&row.name) {
            problems.push(format!(
                "the table lists {}, which isn't a workspace crate",
                row.name
            ));
        }
    }
    for (name, deps) in crates {
        if deps.normal.contains("xtask") || deps.dev.contains("xtask") {
            problems.push(format!("{name} depends on xtask, which is tooling"));
        }
        if name == "xtask" {
            continue;
        }
        if deps.normal.contains(ORACLE) {
            problems.push(format!(
                "{name} depends on {ORACLE} outside [dev-dependencies]"
            ));
        }
        let Some(&own) = layer.get(name.as_str()) else {
            problems.push(format!("{name} has no row in the table"));
            continue;
        };
        for dep in &deps.normal {
            match layer.get(dep.as_str()) {
                Some(&theirs) if theirs >= own => problems.push(format!(
                    "{name} depends on {dep}, which the table lists below it"
                )),
                _ => {}
            }
        }
        let listed = &rows[own].depends_on;
        for dep in deps.normal.difference(listed) {
            problems.push(format!(
                "{name} depends on {dep}, but its row doesn't say so"
            ));
        }
        for dep in listed.difference(&deps.normal) {
            problems.push(format!(
                "{name}'s row names {dep}, but it doesn't depend on it"
            ));
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{command, repo_root};

    /// The workspace against docs/architecture.md. A problem is fixed in the manifest, or in the
    /// table when the layering itself changes, which wants an ADR.
    #[test]
    fn layers_match_the_architecture() {
        let markdown = std::fs::read_to_string(repo_root().join("docs/architecture.md")).unwrap();
        let rows = table(&markdown).unwrap();
        let output = command(&std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
            .args([
                "metadata",
                "--format-version",
                "1",
                "--no-deps",
                "--offline",
            ])
            .output()
            .unwrap();
        assert!(output.status.success(), "cargo metadata failed");
        let crates = manifests(&serde_json::from_slice(&output.stdout).unwrap()).unwrap();
        let problems = problems(&rows, &crates);
        assert!(
            problems.is_empty(),
            "the crates and docs/architecture.md's crate table disagree:\n{}",
            problems.join("\n")
        );
    }

    const TABLE: &str = "\
# Architecture

## Crates

| Crate            | Role   | Depends on |
| ---------------- | ------ | ---------- |
| `spyglass-rng`   | RNGs.  | —          |
| `spyglass-noise` | Noise. | rng        |
| `spyglass-cli`   | CLI.   | spyglass-noise |

Text after the table.
";

    fn deps(normal: &[&str], dev: &[&str]) -> Deps {
        Deps {
            normal: normal.iter().map(|d| d.to_string()).collect(),
            dev: dev.iter().map(|d| d.to_string()).collect(),
        }
    }

    fn workspace(crates: &[(&str, Deps)]) -> BTreeMap<String, Deps> {
        crates
            .iter()
            .map(|(name, deps)| (name.to_string(), deps.clone()))
            .collect()
    }

    #[test]
    fn the_table_is_read_in_order_with_full_names() {
        let rows = table(TABLE).unwrap();
        let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["spyglass-rng", "spyglass-noise", "spyglass-cli"]);
        assert!(rows[0].depends_on.is_empty());
        assert_eq!(
            rows[1].depends_on,
            BTreeSet::from(["spyglass-rng".to_string()])
        );
        assert_eq!(
            rows[2].depends_on,
            BTreeSet::from(["spyglass-noise".to_string()])
        );
    }

    #[test]
    fn a_matching_workspace_has_no_problems() {
        let crates = workspace(&[
            ("spyglass-rng", deps(&[], &[])),
            ("spyglass-noise", deps(&["spyglass-rng"], &["spyglass-cli"])),
            ("spyglass-cli", deps(&["spyglass-noise"], &[])),
            ("xtask", deps(&["spyglass-noise"], &[])),
        ]);
        assert_eq!(
            problems(&table(TABLE).unwrap(), &crates),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_dependency_on_a_lower_row_fails() {
        let crates = workspace(&[
            ("spyglass-rng", deps(&["spyglass-noise"], &[])),
            ("spyglass-noise", deps(&["spyglass-rng"], &[])),
            ("spyglass-cli", deps(&["spyglass-noise"], &[])),
        ]);
        let found = problems(&table(TABLE).unwrap(), &crates);
        assert!(found.contains(
            &"spyglass-rng depends on spyglass-noise, which the table lists below it".to_string()
        ));
        assert!(found.contains(
            &"spyglass-rng depends on spyglass-noise, but its row doesn't say so".to_string()
        ));
    }

    #[test]
    fn the_table_and_the_manifests_must_agree() {
        let crates = workspace(&[
            ("spyglass-rng", deps(&[], &[])),
            ("spyglass-noise", deps(&[], &[])),
            (
                "spyglass-cli",
                deps(&["spyglass-noise", "spyglass-rng"], &[]),
            ),
            ("spyglass-world", deps(&[], &[])),
        ]);
        let found = problems(&table(TABLE).unwrap(), &crates);
        assert_eq!(
            found,
            [
                "spyglass-cli depends on spyglass-rng, but its row doesn't say so",
                "spyglass-noise's row names spyglass-rng, but it doesn't depend on it",
                "spyglass-world has no row in the table",
            ]
        );
    }

    #[test]
    fn the_oracle_and_xtask_are_kept_out() {
        let crates = workspace(&[
            ("spyglass-rng", deps(&[], &[ORACLE])),
            ("spyglass-noise", deps(&["spyglass-rng", ORACLE], &[])),
            ("spyglass-cli", deps(&["spyglass-noise"], &["xtask"])),
        ]);
        let found = problems(&table(TABLE).unwrap(), &crates);
        assert!(found.contains(&format!(
            "spyglass-noise depends on {ORACLE} outside [dev-dependencies]"
        )));
        assert!(found.contains(&"spyglass-cli depends on xtask, which is tooling".to_string()));
        assert!(!found.iter().any(|p| p.starts_with("spyglass-rng")));
    }
}
