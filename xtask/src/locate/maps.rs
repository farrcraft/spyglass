//! The explorer-map cases of `cargo xtask oracle locate`: a data pack whose chest tables give
//! each exploration map the version's tables can, a chest the console fills from them, and the
//! map each fill gives.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::{Value, json};

use super::{DIMENSIONS, Server};
use crate::oracle::Harness;
use crate::repo_root;

/// Where the map cases' chest stands: a loaded block the console's commands can reach.
const MAP_CHEST: [i32; 3] = [0, 100, 0];

/// How many of a seed's golden containers holding one table's maps get cases of their own.
const MAP_CONTAINERS_PER_TABLE: usize = 2;

/// An exploration map the version's chest tables can give: its loot table entry, kept whole
/// so the data pack's table gives it in the version's own form.
pub(super) struct MapEntry {
    /// The table, e.g. `minecraft:chests/shipwreck_map`.
    table: String,
    /// The entry, without its conditions and weight.
    entry: Value,
    /// Its `exploration_map` function, as the table writes it.
    function: Value,
}

/// Every chest table's exploration map entries, in table order, from the bundle.
pub(super) fn map_entries(bundle: &Path) -> Result<Vec<MapEntry>> {
    let dir = bundle.join("data/minecraft/loot_table/chests");
    let mut files: Vec<PathBuf> = walk_json(&dir)?;
    files.sort();
    let mut out = Vec::new();
    for file in files {
        let rel = file.strip_prefix(&dir)?.with_extension("");
        let table = format!(
            "minecraft:chests/{}",
            rel.to_string_lossy().replace('\\', "/")
        );
        let json: Value = serde_json::from_str(&fs::read_to_string(&file)?)?;
        find_map_entries(&json, &table, &mut out);
    }
    Ok(out)
}

fn walk_json(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            out.extend(walk_json(&path)?);
        } else if path.extension().is_some_and(|e| e == "json") {
            out.push(path);
        }
    }
    Ok(out)
}

/// The item entries under `json` with an `exploration_map` function: 26.2 lists them as
/// `functions` with a `function` key, 26.3 on as `modifier` with a `type` key.
fn find_map_entries(json: &Value, table: &str, out: &mut Vec<MapEntry>) {
    match json {
        Value::Object(o) => {
            let functions = o.get("functions").or_else(|| o.get("modifier"));
            let map = functions.and_then(Value::as_array).and_then(|list| {
                list.iter().find(|f| {
                    f.get("function")
                        .or_else(|| f.get("type"))
                        .and_then(Value::as_str)
                        .is_some_and(|k| k.ends_with("exploration_map"))
                })
            });
            if let Some(function) = map {
                let mut entry = o.clone();
                for key in ["conditions", "weight", "quality"] {
                    entry.remove(key);
                }
                out.push(MapEntry {
                    table: table.to_string(),
                    entry: Value::Object(entry),
                    function: function.clone(),
                });
                return;
            }
            for v in o.values() {
                find_map_entries(v, table, out);
            }
        }
        Value::Array(list) => {
            for v in list {
                find_map_entries(v, table, out);
            }
        }
        _ => {}
    }
}

/// One map case: an entry's map, filled from `from` in the Overworld.
pub(super) struct MapCase {
    /// Index into the entries.
    entry: usize,
    from: [i32; 3],
}

/// The map cases for a seed: every entry from the Overworld's origins, then from up to
/// [`MAP_CONTAINERS_PER_TABLE`] of the seed's golden containers with each table, where its
/// container file exists, so the container's own structure is the one it stands in.
pub(super) fn map_cases(
    harness: &Harness,
    seed: &str,
    entries: &[MapEntry],
) -> Result<Vec<MapCase>> {
    let mut cases = Vec::new();
    for &from in DIMENSIONS[0].from {
        for entry in 0..entries.len() {
            cases.push(MapCase { entry, from });
        }
    }
    let file = repo_root()
        .join("oracle-out")
        .join(&harness.version)
        .join("containers/overworld")
        .join(format!("{seed}.json"));
    if let Ok(text) = fs::read_to_string(&file) {
        let json: Value = serde_json::from_str(&text)?;
        let mut taken: std::collections::HashMap<String, usize> = Default::default();
        for chunk in json["chunks"].as_array().into_iter().flatten() {
            for c in chunk["containers"].as_array().into_iter().flatten() {
                let table = c["table"].as_str().unwrap_or_default();
                let count = taken.entry(table.to_string()).or_default();
                if *count >= MAP_CONTAINERS_PER_TABLE {
                    continue;
                }
                let pos: Vec<i32> = c["pos"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|v| v.as_i64().map(|v| v as i32))
                    .collect();
                let [x, y, z] = pos[..] else { continue };
                let mut any = false;
                for (entry, e) in entries.iter().enumerate() {
                    if e.table == table {
                        cases.push(MapCase {
                            entry,
                            from: [x, y, z],
                        });
                        any = true;
                    }
                }
                if any {
                    *count += 1;
                }
            }
        }
    }
    Ok(cases)
}

/// The version's data pack format, from the server jar's `version.json`.
fn data_pack_format(harness: &Harness) -> Result<(u64, u64)> {
    let version = harness
        .version
        .split('+')
        .next()
        .unwrap_or(&harness.version);
    let jar = harness
        .work
        .join("classpath/versions")
        .join(version)
        .join(format!("server-{version}.jar"));
    let mut zip =
        zip::ZipArchive::new(fs::File::open(&jar).with_context(|| jar.display().to_string())?)?;
    let mut text = String::new();
    std::io::Read::read_to_string(&mut zip.by_name("version.json")?, &mut text)?;
    let json: Value = serde_json::from_str(&text)?;
    let pack = &json["pack_version"];
    let major = pack["data_major"]
        .as_u64()
        .or_else(|| pack["data"].as_u64())
        .context("no data pack version in version.json")?;
    Ok((major, pack["data_minor"].as_u64().unwrap_or(0)))
}

/// Writes the data pack the map cases fill from: a table per entry, `spyglass:map/<i>`, giving
/// just that entry.
pub(super) fn write_map_pack(harness: &Harness, world: &Path, entries: &[MapEntry]) -> Result<()> {
    let pack = world.join("datapacks/spyglass_maps");
    let (major, minor) = data_pack_format(harness)?;
    fs::create_dir_all(&pack)?;
    fs::write(
        pack.join("pack.mcmeta"),
        serde_json::to_string_pretty(&json!({
            "pack": {
                "description": "spyglass's map cases",
                "pack_format": major,
                "min_format": [major, minor],
                "max_format": [major, minor],
            }
        }))?,
    )?;
    let tables = pack.join("data/spyglass/loot_table/map");
    fs::create_dir_all(&tables)?;
    for (i, e) in entries.iter().enumerate() {
        let table = json!({"pools": [{"rolls": 1, "entries": [e.entry]}]});
        fs::write(
            tables.join(format!("{i}.json")),
            serde_json::to_string(&table)?,
        )?;
    }
    Ok(())
}

/// The game's answer to one map case: the target's block x and z, or `None` if the map
/// marks nothing.
pub(super) struct MapAnswer {
    target: Option<[i32; 2]>,
    message: String,
}

/// Runs the map cases on a running server, after the locate cases: each fills the chest's
/// first slot from its case's origin and reads the map's target decoration back.
pub(super) fn run_map_cases(
    server: &Server,
    stdin: &mut impl Write,
    cases: &[MapCase],
) -> Result<Vec<MapAnswer>> {
    let [cx, cy, cz] = MAP_CHEST;
    place_map_chest(server, stdin)?;
    for (i, case) in cases.iter().enumerate() {
        let [x, y, z] = case.from;
        writeln!(stdin, "say spyglass-map-{i}")?;
        writeln!(
            stdin,
            "item replace block {cx} {cy} {cz} container.0 with minecraft:air"
        )?;
        writeln!(
            stdin,
            "execute in minecraft:overworld positioned {x} {y} {z} run loot replace block {cx} {cy} {cz} container.0 loot spyglass:map/{}",
            case.entry
        )?;
        writeln!(
            stdin,
            "data get block {cx} {cy} {cz} Items[0].components.\"minecraft:map_decorations\""
        )?;
    }
    writeln!(stdin, "say spyglass-map-end")?;
    stdin.flush()?;
    let mut messages: Vec<Vec<String>> = vec![Vec::new(); cases.len()];
    let mut current = None;
    loop {
        let line = server.next("map answers")?;
        if let Some(rest) = line.split("[Server] spyglass-map-").nth(1) {
            if rest.trim() == "end" {
                break;
            }
            current = Some(rest.trim().parse::<usize>()?);
            continue;
        }
        if let (Some(i), Some((_, message))) = (current, line.split_once("]: "))
            && line.contains("[Server thread/")
        {
            messages[i].push(message.to_string());
        }
    }
    Ok(messages.into_iter().map(map_answer).collect())
}

/// Puts the map chest at [`MAP_CHEST`], in its chunk loaded for good, once the chunk is there.
fn place_map_chest(server: &Server, stdin: &mut impl Write) -> Result<()> {
    let [cx, cy, cz] = MAP_CHEST;
    writeln!(
        stdin,
        "execute in minecraft:overworld run forceload add {cx} {cz}"
    )?;
    loop {
        writeln!(
            stdin,
            "execute in minecraft:overworld run setblock {cx} {cy} {cz} minecraft:chest"
        )?;
        stdin.flush()?;
        let line = loop {
            let line = server.next("the map chest")?;
            if line.contains("Changed the block")
                || line.contains("Could not set the block")
                || line.contains("not loaded")
            {
                break line;
            }
        };
        if !line.contains("not loaded") {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

/// One map case's answer, from the server's lines for it: the target its map decoration marks.
fn map_answer(lines: Vec<String>) -> MapAnswer {
    // "Chest at 0, 100, 0 has the following block data: {"+": {..., x: 12.0d, z: 34.0d}}",
    // or "Found no elements matching …" when the map marks nothing.
    let data = lines
        .iter()
        .find(|l| l.contains("has the following block data"));
    let coord = |data: &str, key: &str| {
        let at = data.find(&format!(" {key}: "))? + key.len() + 3;
        let number: String = data[at..]
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '-' || *c == '.')
            .collect();
        number.parse::<f64>().ok().map(|v| v.floor() as i32)
    };
    let target = data.and_then(|d| Some([coord(d, "x")?, coord(d, "z")?]));
    MapAnswer {
        target,
        message: lines.join("\n"),
    }
}

/// The map cases' rows in the answers file: each case's table entry and starting point beside
/// the target its map points at.
pub(super) fn rows(cases: &[MapCase], answers: Vec<MapAnswer>, entries: &[MapEntry]) -> Vec<Value> {
    cases
        .iter()
        .zip(answers)
        .map(|(case, answer)| {
            let entry = &entries[case.entry];
            json!({
                "table": entry.table,
                "function": entry.function,
                "from": case.from,
                "target": answer.target,
                "message": answer.message,
            })
        })
        .collect()
}
