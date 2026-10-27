//! `cargo xtask version-diff`: what changed between two bundles in the data spyglass reads.
//!
//! For every registry it lists the ids added and removed and counts the files whose bytes
//! changed. For the JSON registries spyglass loads it also compares their shapes: which fields
//! each `type` (or untyped object, by its path) has and what kind of value each holds. A shape in
//! the new bundle that the old one never had is a form some loader may not accept yet, so it
//! shows up here before any load fails.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::Value;
use spyglass_data::bundle::{self, Manifest};

/// The JSON registries whose contents spyglass parses; their shapes are compared field by field.
const READ: &[&str] = &[
    "dimension_type",
    "reports/biome_parameters",
    "tags/block",
    "tags/worldgen/biome",
    "tags/worldgen/structure",
    "worldgen/density_function",
    "worldgen/multi_noise_biome_source_parameter_list",
    "worldgen/noise",
    "worldgen/noise_settings",
    "worldgen/structure",
    "worldgen/structure_set",
    "worldgen/template_pool",
    "worldgen/world_preset",
];

/// Registries spyglass reads without parsing them as JSON: biomes, only checked to exist, and
/// structure templates, which are NBT. Compared by ids and hashes only, like unread registries.
const READ_OPAQUE: &[&str] = &["structure", "worldgen/biome"];

/// A string value is recorded as an enum-like constant, not an id or a color, when it's at most
/// this long and has no `:`, `/` or `#`.
const MAX_ENUM_LEN: usize = 32;

#[derive(clap::Args)]
pub struct VersionDiff {
    /// The older bundle: a directory, or a version under `bundles/`.
    old: String,
    /// The newer bundle: a directory, or a version under `bundles/`.
    new: String,
}

pub fn run(args: &VersionDiff) -> Result<()> {
    let old = Snapshot::read(&resolve(&args.old))?;
    let new = Snapshot::read(&resolve(&args.new))?;
    print!("{}", report(&old, &new));
    Ok(())
}

fn resolve(arg: &str) -> PathBuf {
    let path = PathBuf::from(arg);
    if path.join(bundle::MANIFEST).is_file() {
        path
    } else {
        crate::repo_root().join("bundles").join(arg)
    }
}

/// One file's registry and id: `data/minecraft/worldgen/noise/ridge.json` is
/// `worldgen/noise` and `minecraft:ridge`.
fn classify(path: &str) -> Option<(String, String)> {
    let (rest, ns) = if let Some(rest) = path.strip_prefix("data/") {
        let (ns, rest) = rest.split_once('/')?;
        (rest, ns)
    } else if let Some(rest) = path.strip_prefix("reports/biome_parameters/") {
        let (ns, rest) = rest.split_once('/')?;
        return Some((
            "reports/biome_parameters".into(),
            format!("{ns}:{}", strip_ext(rest)),
        ));
    } else {
        return Some(("reports".into(), path.to_string()));
    };
    let depth = if rest.starts_with("tags/worldgen/") {
        3
    } else if rest.starts_with("worldgen/") || rest.starts_with("tags/") {
        2
    } else {
        1
    };
    let mut parts = rest.splitn(depth + 1, '/');
    let registry: Vec<&str> = parts.by_ref().take(depth).collect();
    let id = parts.next()?;
    Some((registry.join("/"), format!("{ns}:{}", strip_ext(id))))
}

fn strip_ext(id: &str) -> &str {
    id.strip_suffix(".json")
        .or_else(|| id.strip_suffix(".nbt"))
        .unwrap_or(id)
}

/// What `version-diff` compares in one bundle.
struct Snapshot {
    manifest: Manifest,
    /// Registry → id → file hash.
    ids: BTreeMap<String, BTreeMap<String, String>>,
    /// Registry → shape fact → (uses, first id with it).
    shapes: BTreeMap<String, BTreeMap<String, (usize, String)>>,
}

impl Snapshot {
    fn read(dir: &Path) -> Result<Self> {
        let manifest = bundle::read_manifest(dir)
            .with_context(|| format!("reading the bundle at {}", dir.display()))?;
        let mut ids: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
        let mut shapes: BTreeMap<String, BTreeMap<String, (usize, String)>> = BTreeMap::new();
        for (path, hash) in &manifest.files {
            let Some((registry, id)) = classify(path) else {
                continue;
            };
            if READ.contains(&registry.as_str()) && path.ends_with(".json") {
                let file = dir.join(path);
                let text = fs::read_to_string(&file)
                    .with_context(|| format!("reading {}", file.display()))?;
                let value: Value = serde_json::from_str(&text)
                    .with_context(|| format!("parsing {}", file.display()))?;
                let mut facts = BTreeSet::new();
                walk(&value, "", &mut facts);
                let registry_shapes = shapes.entry(registry.clone()).or_default();
                for fact in facts {
                    registry_shapes
                        .entry(fact)
                        .or_insert_with(|| (0, id.clone()))
                        .0 += 1;
                }
            }
            ids.entry(registry).or_default().insert(id, hash.clone());
        }
        Ok(Self {
            manifest,
            ids,
            shapes,
        })
    }
}

/// Records the shape facts under `value`, whose context is `at`: the nearest enclosing `type` id
/// in braces, then the path from it, with `[]` for array elements.
fn walk(value: &Value, at: &str, facts: &mut BTreeSet<String>) {
    match value {
        Value::Object(map) => {
            let at = match map.get("type").and_then(Value::as_str) {
                Some(ty) => {
                    facts.insert(format!("{{{ty}}} at {}", root(at)));
                    format!("{{{ty}}}")
                }
                None => at.to_string(),
            };
            for (key, child) in map {
                if key == "type" && child.is_string() {
                    continue;
                }
                let path = format!("{at}.{key}");
                facts.insert(format!("{path}: {}", kind(child)));
                walk(child, &path, facts);
            }
        }
        Value::Array(items) => {
            let path = format!("{at}[]");
            for item in items {
                facts.insert(format!("{path}: {}", kind(item)));
                walk(item, &path, facts);
            }
        }
        Value::String(s) if is_enum(s) && !at.is_empty() => {
            facts.insert(format!("{at} = {s:?}"));
        }
        _ => {}
    }
}

/// Where a typed object sits, for its `{type} at` fact: its path without enclosing types, so a
/// type used inline and as a whole file both read as one place per field.
fn root(at: &str) -> &str {
    if at.is_empty() {
        "<file>"
    } else {
        at.rsplit_once('}').map_or(at, |(_, field)| field)
    }
}

fn kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn is_enum(s: &str) -> bool {
    s.len() <= MAX_ENUM_LEN && !s.contains([':', '/', '#']) && !s.is_empty()
}

fn report(old: &Snapshot, new: &Snapshot) -> String {
    let mut out = String::new();
    let (o, n) = (&old.manifest, &new.manifest);
    let _ = writeln!(
        out,
        "{} (data version {}) -> {} (data version {})",
        o.version, o.data_version, n.version, n.data_version
    );
    let registries: BTreeSet<&String> = old.ids.keys().chain(new.ids.keys()).collect();
    let empty_ids = BTreeMap::new();
    let empty_shapes = BTreeMap::new();
    let mut quiet = Vec::new();
    for registry in registries {
        let (before, after) = (
            old.ids.get(registry).unwrap_or(&empty_ids),
            new.ids.get(registry).unwrap_or(&empty_ids),
        );
        let added: Vec<&String> = after
            .keys()
            .filter(|id| !before.contains_key(*id))
            .collect();
        let removed: Vec<&String> = before
            .keys()
            .filter(|id| !after.contains_key(*id))
            .collect();
        let changed = after
            .iter()
            .filter(|(id, hash)| before.get(*id).is_some_and(|h| h != *hash))
            .count();
        let (shapes_before, shapes_after) = (
            old.shapes.get(registry).unwrap_or(&empty_shapes),
            new.shapes.get(registry).unwrap_or(&empty_shapes),
        );
        let new_shapes: Vec<_> = shapes_after
            .iter()
            .filter(|(fact, _)| !shapes_before.contains_key(*fact))
            .collect();
        let gone_shapes: Vec<_> = shapes_before
            .iter()
            .filter(|(fact, _)| !shapes_after.contains_key(*fact))
            .collect();
        if added.is_empty() && removed.is_empty() && new_shapes.is_empty() && gone_shapes.is_empty()
        {
            quiet.push(format!("{registry} ({changed} changed)"));
            continue;
        }
        let read = if READ.contains(&registry.as_str()) || READ_OPAQUE.contains(&registry.as_str())
        {
            ""
        } else {
            " (not read by spyglass)"
        };
        let _ = writeln!(
            out,
            "\n{registry}{read}: {} -> {} ids, {} added, {} removed, {changed} changed",
            before.len(),
            after.len(),
            added.len(),
            removed.len()
        );
        list(&mut out, "added", &added);
        list(&mut out, "removed", &removed);
        for (label, facts) in [("new shapes", &new_shapes), ("gone shapes", &gone_shapes)] {
            if facts.is_empty() {
                continue;
            }
            let _ = writeln!(out, "  {label}:");
            for (fact, (uses, example)) in facts {
                let _ = writeln!(out, "    {fact}  ({uses} files, e.g. {example})");
            }
        }
    }
    if !quiet.is_empty() {
        let _ = writeln!(out, "\nno ids or shapes changed: {}", quiet.join(", "));
    }
    out
}

/// Prints up to a screenful of ids, then how many more.
fn list(out: &mut String, label: &str, ids: &[&String]) {
    const SHOWN: usize = 30;
    if ids.is_empty() {
        return;
    }
    let shown: Vec<&str> = ids.iter().take(SHOWN).map(|s| s.as_str()).collect();
    let more = ids.len().saturating_sub(SHOWN);
    let _ = write!(out, "  {label}: {}", shown.join(", "));
    if more > 0 {
        let _ = write!(out, " and {more} more");
    }
    out.push('\n');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_paths() {
        let cases = [
            (
                "data/minecraft/worldgen/noise/ridge.json",
                ("worldgen/noise", "minecraft:ridge"),
            ),
            (
                "data/minecraft/worldgen/density_function/overworld/base_3d_noise.json",
                (
                    "worldgen/density_function",
                    "minecraft:overworld/base_3d_noise",
                ),
            ),
            (
                "data/minecraft/tags/worldgen/biome/is_ocean.json",
                ("tags/worldgen/biome", "minecraft:is_ocean"),
            ),
            (
                "data/minecraft/tags/block/logs.json",
                ("tags/block", "minecraft:logs"),
            ),
            (
                "data/minecraft/structure/igloo/top.nbt",
                ("structure", "minecraft:igloo/top"),
            ),
            (
                "reports/biome_parameters/minecraft/overworld.json",
                ("reports/biome_parameters", "minecraft:overworld"),
            ),
        ];
        for (path, (registry, id)) in cases {
            assert_eq!(
                classify(path),
                Some((registry.to_string(), id.to_string())),
                "{path}"
            );
        }
    }

    fn facts(json: &str) -> BTreeSet<String> {
        let mut facts = BTreeSet::new();
        walk(&serde_json::from_str(json).unwrap(), "", &mut facts);
        facts
    }

    #[test]
    fn shapes_name_types_fields_kinds_and_enums() {
        let got = facts(
            r#"{"type": "minecraft:add", "argument1": 1.0,
                "argument2": {"type": "minecraft:upscale", "x": {"mode": "interpolate_linear"},
                              "input": "minecraft:zero"}}"#,
        );
        let want = [
            "{minecraft:add} at <file>",
            "{minecraft:add}.argument1: number",
            "{minecraft:add}.argument2: object",
            "{minecraft:upscale} at .argument2",
            "{minecraft:upscale}.x: object",
            "{minecraft:upscale}.x.mode: string",
            "{minecraft:upscale}.x.mode = \"interpolate_linear\"",
            "{minecraft:upscale}.input: string",
        ];
        assert_eq!(got, want.iter().map(|s| s.to_string()).collect());
    }

    #[test]
    fn arrays_and_ids() {
        let got = facts(r#"{"amplitudes": [1.0, 0.5], "noise": "minecraft:ridge"}"#);
        let want = [
            ".amplitudes: array",
            ".amplitudes[]: number",
            ".noise: string",
        ];
        assert_eq!(got, want.iter().map(|s| s.to_string()).collect());
    }

    /// Writes a bundle of `files` (path → contents) with a valid manifest.
    fn fixture(version: &str, data_version: u32, files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let mut hashes = BTreeMap::new();
        for (path, text) in files {
            let file = dir.path().join(path);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(&file, text).unwrap();
            hashes.insert(path.to_string(), bundle::sha1_hex(text.as_bytes()).unwrap());
        }
        let manifest = Manifest {
            format: bundle::FORMAT,
            version: version.into(),
            data_version,
            jar_sha1: "0".repeat(40),
            java_version: "25".into(),
            extractor: "0.0.0".into(),
            files: hashes,
        };
        fs::write(
            dir.path().join(bundle::MANIFEST),
            serde_json::to_string(&manifest).unwrap(),
        )
        .unwrap();
        dir
    }

    #[test]
    fn reports_ids_changes_and_shapes() {
        let old = fixture(
            "1.0",
            1,
            &[
                (
                    "data/minecraft/worldgen/noise_settings/overworld.json",
                    r#"{"default_block": {"Name": "minecraft:stone"}, "sea_level": 63}"#,
                ),
                (
                    "data/minecraft/worldgen/density_function/a.json",
                    r#"{"type": "minecraft:interpolated", "argument": 0.0}"#,
                ),
                ("data/minecraft/worldgen/biome/plains.json", "{}"),
                ("data/minecraft/structure/igloo/top.nbt", "x"),
            ],
        );
        let new = fixture(
            "1.1",
            2,
            &[
                (
                    "data/minecraft/worldgen/noise_settings/overworld.json",
                    r#"{"sea_level": 63}"#,
                ),
                (
                    "data/minecraft/worldgen/density_function/a.json",
                    r#"{"type": "minecraft:upscale", "argument": 0.0}"#,
                ),
                ("data/minecraft/worldgen/biome/plains.json", "{}"),
                ("data/minecraft/worldgen/biome/ice_caves.json", "{}"),
                ("data/minecraft/structure/igloo/top.nbt", "y"),
            ],
        );
        let text = report(
            &Snapshot::read(old.path()).unwrap(),
            &Snapshot::read(new.path()).unwrap(),
        );
        let expected = [
            "1.0 (data version 1) -> 1.1 (data version 2)",
            "worldgen/biome: 1 -> 2 ids, 1 added, 0 removed, 0 changed",
            "  added: minecraft:ice_caves",
            "    {minecraft:upscale} at <file>  (1 files, e.g. minecraft:a)",
            "    {minecraft:interpolated} at <file>  (1 files, e.g. minecraft:a)",
            "    .default_block: object  (1 files, e.g. minecraft:overworld)",
            "    .default_block.Name: string  (1 files, e.g. minecraft:overworld)",
        ];
        for line in expected {
            assert!(text.contains(line), "{line:?} in:\n{text}");
        }
        assert!(
            text.contains("no ids or shapes changed: structure (1 changed)"),
            "{text}"
        );
    }
}
