//! `cargo xtask version-diff --code`: which of the game classes spyglass ports changed between
//! two versions, from their bytecode. The data diff can't see algorithm changes; this can.
//!
//! Each version's game jar comes from the oracle's unpacked classpath
//! (`target/oracle/<version>/classpath/`, made by `cargo xtask oracle build`). `javap -c -p
//! -constants` disassembles the classes under [`PORTED`], and each class's text is normalized so
//! only real changes differ: constant-pool indices, bytecode offsets, branch targets and lambda
//! numbers go. The normalized text is kept in `target/version-diff/<version>/` to read or diff.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use spyglass_extract::java;

use crate::repo_root;

/// Class-name prefixes (with `/`) of the game code spyglass ports or calls into while porting:
/// worldgen, biomes, the chunk generator and dimension types, and `Mth`.
const PORTED: &[&str] = &[
    "net/minecraft/world/level/levelgen/",
    "net/minecraft/world/level/biome/",
    "net/minecraft/world/level/chunk/ChunkGenerator",
    "net/minecraft/world/level/dimension/DimensionType",
    "net/minecraft/util/Mth",
    "net/minecraft/util/CubicSpline",
];

/// Classes per `javap` run.
const BATCH: usize = 200;

/// The changes from `old` to `new` (version ids, e.g. `26.3`) in the classes under [`PORTED`].
pub fn report(old: &str, new: &str) -> Result<String> {
    let jdk = java::discover(None)
        .into_iter()
        .filter(|j| j.is_jdk)
        .max_by_key(|j| j.major)
        .context("no JDK found; scripts/setup.ps1 -Oracle (Windows) or scripts/setup.sh --oracle installs one")?;
    let bin = jdk.home.join("bin");
    let before = classes(&bin, old)?;
    let after = classes(&bin, new)?;

    let names: BTreeSet<&String> = before.keys().chain(after.keys()).collect();
    let (mut added, mut removed, mut changed) = (Vec::new(), Vec::new(), Vec::new());
    for name in names {
        match (before.get(name), after.get(name)) {
            (None, Some(_)) => added.push(name.as_str()),
            (Some(_), None) => removed.push(name.as_str()),
            (Some(a), Some(b)) if a != b => {
                let (minus, plus) = line_changes(a, b);
                changed.push((name.as_str(), minus, plus));
            }
            _ => {}
        }
    }
    let mut out = format!(
        "{old} -> {new}: bytecode of {} ported classes ({} before)\n",
        after.len(),
        before.len()
    );
    let _ = writeln!(
        out,
        "{} changed, {} added, {} removed",
        changed.len(),
        added.len(),
        removed.len()
    );
    if !changed.is_empty() {
        let _ = writeln!(out, "\nchanged (lines removed/added, normalized):");
        for (name, minus, plus) in &changed {
            let _ = writeln!(out, "  {name}  -{minus} +{plus}");
        }
    }
    for (label, list) in [("added", &added), ("removed", &removed)] {
        if !list.is_empty() {
            let _ = writeln!(out, "\n{label}:");
            for name in list.iter() {
                let _ = writeln!(out, "  {name}");
            }
        }
    }
    let _ = writeln!(out, "\nnormalized disassembly: {}", dump_dir("").display());
    Ok(out)
}

fn dump_dir(version: &str) -> PathBuf {
    repo_root()
        .join("target")
        .join("version-diff")
        .join(version)
}

/// The game jar `cargo xtask oracle build` unpacked for `version`.
fn game_jar(version: &str) -> Result<PathBuf> {
    let list = repo_root()
        .join("target")
        .join("oracle")
        .join(version)
        .join("classpath")
        .join("classpath.txt");
    let text = fs::read_to_string(&list).with_context(|| {
        format!(
            "no unpacked jar for {version}; run cargo xtask oracle build --game-version {version}"
        )
    })?;
    text.lines()
        .map(PathBuf::from)
        .find(|p| p.components().any(|c| c.as_os_str() == "versions"))
        .with_context(|| format!("{} lists no game jar", list.display()))
}

/// The normalized disassembly of every class under [`PORTED`] in `version`'s jar, by class name.
/// Also written to `target/version-diff/<version>/`.
fn classes(jdk_bin: &Path, version: &str) -> Result<BTreeMap<String, String>> {
    let jar = game_jar(version)?;
    let listing = run(Command::new(jdk_bin.join(exe("jar"))).arg("tf").arg(&jar))?;
    let names: Vec<String> = listing
        .lines()
        .filter(|l| l.ends_with(".class") && PORTED.iter().any(|p| l.starts_with(p)))
        .map(|l| l.trim_end_matches(".class").replace('/', "."))
        .collect();
    if names.is_empty() {
        bail!("no ported classes in {}", jar.display());
    }
    let dir = dump_dir(version);
    if dir.exists() {
        fs::remove_dir_all(&dir)?;
    }
    fs::create_dir_all(&dir)?;
    let mut out = BTreeMap::new();
    for batch in names.chunks(BATCH) {
        let text = run(Command::new(jdk_bin.join(exe("javap")))
            .args(["-c", "-p", "-constants", "-cp"])
            .arg(&jar)
            .args(batch))?;
        for (name, body) in split(&text) {
            fs::write(dir.join(format!("{name}.txt")), &body)?;
            out.insert(name, body);
        }
    }
    Ok(out)
}

fn exe(tool: &str) -> String {
    if cfg!(windows) {
        format!("{tool}.exe")
    } else {
        tool.to_string()
    }
}

fn run(command: &mut Command) -> Result<String> {
    let output = command
        .output()
        .with_context(|| format!("failed to run {command:?}"))?;
    if !output.status.success() {
        bail!(
            "{command:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// `javap` output split into classes, each normalized, by the name in its declaration.
fn split(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut current: Option<(String, String)> = None;
    for line in text.lines() {
        if line.starts_with("Compiled from ") {
            out.extend(current.take());
            continue;
        }
        if current.is_none() {
            let Some(name) = declared(line) else { continue };
            current = Some((name, String::new()));
        }
        if let Some((_, body)) = current.as_mut() {
            body.push_str(&normalize(line));
            body.push('\n');
        }
    }
    out.extend(current);
    out
}

/// The class a declaration line names: the word after `class` or `interface`, generics cut.
fn declared(line: &str) -> Option<String> {
    let mut words = line.split_whitespace();
    words.find(|w| matches!(*w, "class" | "interface"))?;
    let name = words.next()?;
    Some(name.split('<').next().unwrap_or(name).to_string())
}

/// One line of disassembly without what changes when nothing changed: leading offsets
/// (`12: `), constant-pool indices (`#34`, `#5:6`), branch targets, switch-table targets,
/// lambda numbers and runs of whitespace.
fn normalize(line: &str) -> String {
    let mut s = line.trim().to_string();
    // A bytecode offset, or a switch-table entry ("1: 45", "default: 45").
    if let Some((head, rest)) = s.split_once(": ")
        && (head.parse::<i64>().is_ok() || head == "default")
    {
        let rest = rest.trim();
        if rest.parse::<i64>().is_ok() {
            return if head == "default" { "default" } else { "case" }.to_string();
        }
        if head != "default" {
            s = rest.to_string();
        }
    }
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '#' && chars.peek().is_some_and(char::is_ascii_digit) {
            out.push('#');
            while chars
                .peek()
                .is_some_and(|c| c.is_ascii_digit() || *c == ':')
            {
                chars.next();
            }
            // invokedynamic and invokeinterface's second operand: `#26,  0`.
            let mut ahead = chars.clone();
            if ahead.next() == Some(',') {
                while ahead.peek() == Some(&' ') {
                    ahead.next();
                }
                if ahead.peek().is_some_and(char::is_ascii_digit) {
                    while ahead.peek().is_some_and(char::is_ascii_digit) {
                        ahead.next();
                    }
                    chars = ahead;
                }
            }
            continue;
        }
        out.push(c);
    }
    let mut words: Vec<String> = out.split_whitespace().map(str::to_string).collect();
    // A branch: its target is an offset.
    if words.len() == 2
        && (words[0].starts_with("if") || words[0].starts_with("goto") || words[0] == "jsr")
        && words[1].parse::<i64>().is_ok()
    {
        words[1] = "L".to_string();
    }
    for word in &mut words {
        *word = lambda_numbers(word);
    }
    words.join(" ")
}

/// `lambda$name$12` → `lambda$name$N`.
fn lambda_numbers(word: &str) -> String {
    let mut out = String::new();
    let mut rest = word;
    while let Some(i) = rest.find("lambda$") {
        let (head, tail) = rest.split_at(i + "lambda$".len());
        out.push_str(head);
        let Some(j) = tail.find('$') else {
            rest = tail;
            break;
        };
        out.push_str(&tail[..=j]);
        let after = &tail[j + 1..];
        let digits = after.chars().take_while(char::is_ascii_digit).count();
        if digits > 0 {
            out.push('N');
        }
        rest = &after[digits..];
    }
    out.push_str(rest);
    out
}

/// Lines only in `a` and lines only in `b`, counted as multisets.
fn line_changes(a: &str, b: &str) -> (usize, usize) {
    let mut counts: BTreeMap<&str, i64> = BTreeMap::new();
    for line in a.lines() {
        *counts.entry(line).or_default() += 1;
    }
    for line in b.lines() {
        *counts.entry(line).or_default() -= 1;
    }
    let minus = counts
        .values()
        .filter(|&&c| c > 0)
        .map(|&c| c as usize)
        .sum();
    let plus = counts
        .values()
        .filter(|&&c| c < 0)
        .map(|&c| -c as usize)
        .sum();
    (minus, plus)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_what_changes_when_nothing_did() {
        assert_eq!(
            normalize("        12: ldc2_w        #34                 // double 0.5d"),
            "ldc2_w # // double 0.5d"
        );
        assert_eq!(normalize("        17: ifeq          45"), "ifeq L");
        assert_eq!(normalize("      140: goto          12"), "goto L");
        assert_eq!(normalize("                 1: 45"), "case");
        assert_eq!(normalize("           default: 88"), "default");
        assert_eq!(
            normalize("  3: invokedynamic #26,  0 // InvokeDynamic #0:apply:()"),
            "invokedynamic # // InvokeDynamic #apply:()"
        );
        assert_eq!(
            normalize("private static void lambda$build$12(int);"),
            "private static void lambda$build$N(int);"
        );
        assert_eq!(normalize("  public int x;"), "public int x;");
    }

    #[test]
    fn splits_classes_by_declaration() {
        let text = "Compiled from \"A.java\"\npublic final class a.A$B extends b.C {\n  int x;\n}\n\
                    Compiled from \"D.java\"\npublic interface a.D<T> {\n}\n";
        let classes = split(text);
        let names: Vec<&str> = classes.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["a.A$B", "a.D"]);
        assert!(classes[0].1.contains("int x;"));
    }

    #[test]
    fn counts_line_changes() {
        assert_eq!(line_changes("a\nb\nb\nc\n", "a\nb\nd\n"), (2, 1));
        assert_eq!(line_changes("x\n", "x\n"), (0, 0));
    }
}
