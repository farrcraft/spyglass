//! `cargo xtask oracle locate`: the real game's `/locate` answers, from a dedicated server.
//!
//! `ChunkGenerator.findNearestMapStructure` needs a live level, so instead of the standalone
//! harness this runs the server from the same jar on a throwaway world per seed, and sends
//! `execute in ... positioned ... run locate ...` on its console. See docs/specs/oracle.md.

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use spyglass_world::Save;

use crate::oracle::Harness;
use crate::repo_root;

/// Seeds for the `/locate` cases: trivial, text and the two real worlds.
const SEEDS: &[&str] = &[
    "0",
    "spyglass",
    "-4893098694207956070",
    "2514887864703934023",
];

/// One dimension's cases: where they're run from (block positions in that dimension), and the
/// `/locate structure` and `/locate biome` arguments.
struct Cases {
    dimension: &'static str,
    from: &'static [[i32; 3]],
    structures: &'static [&'static str],
    biomes: &'static [&'static str],
}

const DIMENSIONS: &[Cases] = &[
    Cases {
        dimension: "minecraft:overworld",
        from: &[[0, 64, 0], [1500, 64, -2500], [-20000, -30, 7000]],
        structures: OVERWORLD_STRUCTURES,
        biomes: OVERWORLD_BIOMES,
    },
    // The Overworld's places divided by 8, roughly, at heights inside the Nether.
    Cases {
        dimension: "minecraft:the_nether",
        from: &[[0, 64, 0], [190, 40, -310], [-2500, 100, 875]],
        structures: &[
            "minecraft:fortress",
            "minecraft:bastion_remnant",
            "minecraft:nether_fossil",
            "minecraft:ruined_portal_nether",
        ],
        biomes: &[
            "minecraft:nether_wastes",
            "minecraft:soul_sand_valley",
            "minecraft:crimson_forest",
            "minecraft:warped_forest",
            "minecraft:basalt_deltas",
        ],
    },
    // The main island, the outer islands, and far out.
    Cases {
        dimension: "minecraft:the_end",
        from: &[[0, 64, 0], [1500, 64, -2500], [-20000, 64, 7000]],
        structures: &["minecraft:end_city"],
        biomes: &[
            "minecraft:the_end",
            "minecraft:end_highlands",
            "minecraft:end_midlands",
            "minecraft:small_end_islands",
            "minecraft:end_barrens",
        ],
    },
];

/// `/locate structure` arguments in the Overworld: every kind, as ids or tags.
const OVERWORLD_STRUCTURES: &[&str] = &[
    "#minecraft:village",
    "minecraft:stronghold",
    "minecraft:mansion",
    "minecraft:monument",
    "minecraft:ancient_city",
    "minecraft:trial_chambers",
    "minecraft:desert_pyramid",
    "minecraft:jungle_pyramid",
    "minecraft:swamp_hut",
    "minecraft:igloo",
    "minecraft:pillager_outpost",
    "minecraft:buried_treasure",
    "#minecraft:ocean_ruin",
    "#minecraft:shipwreck",
    "#minecraft:ruined_portal",
    "minecraft:trail_ruins",
    "#minecraft:mineshaft",
    "#minecraft:abandoned_camp",
];

/// `/locate biome` arguments in the Overworld: common, rare, and underground (the search is 3D).
const OVERWORLD_BIOMES: &[&str] = &[
    "minecraft:plains",
    "minecraft:cherry_grove",
    "minecraft:mushroom_fields",
    "minecraft:ice_spikes",
    "minecraft:eroded_badlands",
    "minecraft:deep_dark",
    "minecraft:lush_caves",
];

/// How long one world may take, startup and every case included.
const TIMEOUT: Duration = Duration::from_secs(1800);

/// Where a server's world comes from.
enum Source<'a> {
    /// A new world with this seed.
    Seed(&'a str),
    /// A copy of this save.
    Save(&'a Path),
}

/// The environment variable that accepts the Minecraft EULA, as `--accept-eula` does.
const ACCEPT_EULA_VAR: &str = "SPYGLASS_ACCEPT_EULA";

/// An error unless whoever runs this has accepted the Minecraft EULA, with `--accept-eula`
/// (`flag`) or [`ACCEPT_EULA_VAR`] set to `1` or `true`. The dedicated server won't start
/// without an `eula.txt` that accepts it, and accepting is for the person running it, not for
/// spyglass to do silently (ADR 0033).
pub fn require_eula(flag: bool) -> Result<()> {
    let var = std::env::var(ACCEPT_EULA_VAR).unwrap_or_default();
    if flag || eula_value_accepts(&var) {
        return Ok(());
    }
    bail!(
        "this runs Mojang's dedicated server, which needs you to accept the Minecraft EULA \
         (https://aka.ms/MinecraftEULA); pass --accept-eula or set {ACCEPT_EULA_VAR}=1 if you do"
    )
}

/// Whether the variable's value accepts the EULA.
fn eula_value_accepts(value: &str) -> bool {
    matches!(value.trim(), "1" | "true")
}

/// With no `worlds`, writes `oracle-out/<version>/locate/<seed>.json` for each seed, replacing
/// the folder. With `worlds`, writes `world-<folder>.json` for each save, from a copy of it,
/// with the player's position as an extra origin in the player's dimension.
pub fn run(harness: &Harness, worlds: &[PathBuf]) -> Result<()> {
    let out = repo_root()
        .join("oracle-out")
        .join(&harness.version)
        .join("locate");
    if worlds.is_empty() && out.exists() {
        fs::remove_dir_all(&out)?;
    }
    fs::create_dir_all(&out)?;
    let mut jobs: Vec<Job<'_>> = Vec::new();
    let entries = map_entries(&repo_root().join("bundles").join(&harness.version))?;
    if worlds.is_empty() {
        for seed in SEEDS {
            jobs.push(Job {
                name: format!("{seed}.json"),
                source: Source::Seed(seed),
                cases: cases(None),
                maps: map_cases(harness, seed, &entries)?,
                seed: json!(seed),
            });
        }
    }
    for world in worlds {
        let save = Save::read(world).with_context(|| format!("reading {}", world.display()))?;
        let player = save
            .player
            .as_ref()
            .map(|p| (p.dimension.as_str(), p.pos.map(|v| v.floor() as i32)));
        let folder = world
            .file_name()
            .context("a save folder")?
            .to_string_lossy()
            .into_owned();
        jobs.push(Job {
            name: format!("world-{folder}.json"),
            source: Source::Save(world),
            cases: cases(player),
            // A save's structures may already be referenced by its own maps.
            maps: Vec::new(),
            seed: json!(save.seed.to_string()),
        });
    }
    for Job {
        name,
        source,
        cases,
        maps,
        seed,
    } in jobs
    {
        let started = Instant::now();
        let file = out.join(&name);
        let (answers, map_answers) = server(harness, &name, &source, &cases, &maps, &entries)?;
        let rows: Vec<Value> = cases
            .iter()
            .zip(answers)
            .map(|(case, answer)| {
                json!({
                    "dimension": case.dimension,
                    "kind": case.kind,
                    "target": case.target,
                    "from": case.from,
                    "answer": answer.position,
                    "found": answer.found,
                    "message": answer.message,
                })
            })
            .collect();
        let map_rows: Vec<Value> = maps
            .iter()
            .zip(map_answers)
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
            .collect();
        let mut root = json!({
            "format": 3,
            "version": harness.version,
            "seed": seed,
            "cases": rows,
            "maps": map_rows,
        });
        if let Source::Save(dir) = source {
            root["world_dir"] = json!(dir);
        }
        fs::write(&file, serde_json::to_string_pretty(&root)? + "\n")?;
        println!(
            "{}: {:.0} s",
            file.display(),
            started.elapsed().as_secs_f64()
        );
    }
    Ok(())
}

/// Where the map cases' chest stands: a loaded block the console's commands can reach.
const MAP_CHEST: [i32; 3] = [0, 100, 0];

/// How many of a seed's golden containers holding one table's maps get cases of their own.
const MAP_CONTAINERS_PER_TABLE: usize = 2;

/// An exploration map the version's chest tables can give: its loot table entry, kept whole
/// so the data pack's table gives it in the version's own form.
struct MapEntry {
    /// The table, e.g. `minecraft:chests/shipwreck_map`.
    table: String,
    /// The entry, without its conditions and weight.
    entry: Value,
    /// Its `exploration_map` function, as the table writes it.
    function: Value,
}

/// Every chest table's exploration map entries, in table order, from the bundle.
fn map_entries(bundle: &Path) -> Result<Vec<MapEntry>> {
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
struct MapCase {
    /// Index into the entries.
    entry: usize,
    from: [i32; 3],
}

/// The map cases for a seed: every entry from the Overworld's origins, then from up to
/// [`MAP_CONTAINERS_PER_TABLE`] of the seed's golden containers with each table, where its
/// container file exists, so the container's own structure is the one it stands in.
fn map_cases(harness: &Harness, seed: &str, entries: &[MapEntry]) -> Result<Vec<MapCase>> {
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
fn write_map_pack(harness: &Harness, world: &Path, entries: &[MapEntry]) -> Result<()> {
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
struct MapAnswer {
    target: Option<[i32; 2]>,
    message: String,
}

/// Runs the map cases on a running server, after the locate cases: each fills the chest's
/// first slot from its case's origin and reads the map's target decoration back.
fn run_map_cases(
    server: &Server,
    stdin: &mut impl Write,
    cases: &[MapCase],
) -> Result<Vec<MapAnswer>> {
    let [cx, cy, cz] = MAP_CHEST;
    // The chest's chunk, loaded for good; the chest goes in once it's there.
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
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
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
    Ok(messages
        .into_iter()
        .map(|lines| {
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
        })
        .collect())
}

/// One server run: its file's name, its world, its cases, and its seed as the file gives it.
struct Job<'a> {
    name: String,
    source: Source<'a>,
    cases: Vec<Case>,
    maps: Vec<MapCase>,
    seed: Value,
}

/// Copies a save, skipping its `session.lock`, so the server never touches the original.
fn copy_save(from: &Path, to: &Path) -> Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_save(&entry.path(), &target)?;
        } else if entry.file_name() != "session.lock" {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// One `/locate` question.
struct Case {
    /// The dimension asked in.
    dimension: &'static str,
    /// `structure` or `biome`, as `/locate` takes it.
    kind: &'static str,
    /// The structure or biome id, or `#tag`.
    target: &'static str,
    /// Where it's asked from.
    from: [i32; 3],
}

/// Every dimension's cases from its origins, plus `extra`'s origin in its dimension (a
/// save's player).
fn cases(extra: Option<(&str, [i32; 3])>) -> Vec<Case> {
    let mut cases = Vec::new();
    for dimension_cases in DIMENSIONS {
        let mut origins = dimension_cases.from.to_vec();
        if let Some((dimension, at)) = extra
            && dimension == dimension_cases.dimension
        {
            origins.push(at);
        }
        for from in origins {
            for &target in dimension_cases.structures {
                cases.push(Case {
                    dimension: dimension_cases.dimension,
                    kind: "structure",
                    target,
                    from,
                });
            }
            for &target in dimension_cases.biomes {
                cases.push(Case {
                    dimension: dimension_cases.dimension,
                    kind: "biome",
                    target,
                    from,
                });
            }
        }
    }
    cases
}

/// The game's answer to one case.
struct Answer {
    /// Block x, y and z; y is `None` where the game prints `~` (structures).
    position: Option<[Option<i32>; 3]>,
    /// The structure or biome the game named, e.g. `minecraft:village_plains`.
    found: Option<String>,
    /// The game's message, for diagnosis.
    message: String,
}

/// Runs a server on a fresh world or a copy of a save, in a throwaway folder named after
/// `name`, sends every case after it's ready, and collects one message per case between `say`
/// markers.
fn server(
    harness: &Harness,
    name: &str,
    source: &Source<'_>,
    cases: &[Case],
    maps: &[MapCase],
    entries: &[MapEntry],
) -> Result<(Vec<Answer>, Vec<MapAnswer>)> {
    let dir = server_dir(harness, name, source)?;
    if !maps.is_empty() {
        write_map_pack(harness, &dir.join("world"), entries)?;
    }
    let mut server = Server::start(harness, &dir)?;

    // "Done (12.345s)! For help, type "help"".
    while !server.next("startup")?.contains("]: Done (") {}
    let mut stdin = server.child.0.stdin.take().context("no server stdin")?;
    for (i, case) in cases.iter().enumerate() {
        let [x, y, z] = case.from;
        writeln!(stdin, "say spyglass-case-{i}")?;
        writeln!(
            stdin,
            "execute in {} positioned {x} {y} {z} run locate {} {}",
            case.dimension, case.kind, case.target
        )?;
    }
    writeln!(stdin, "say spyglass-case-end")?;
    stdin.flush()?;

    // Each case's lines: from its marker to the next.
    let mut messages: Vec<Vec<String>> = vec![Vec::new(); cases.len()];
    let mut current: Option<usize> = None;
    loop {
        let line = server.next("locate answers")?;
        if let Some(rest) = line.split("[Server] spyglass-case-").nth(1) {
            if rest.trim() == "end" {
                break;
            }
            current = Some(rest.trim().parse()?);
            continue;
        }
        // Server log lines: "[time] [Server thread/INFO]: System chat: message", plus a
        // "Locating element ... took n ms" timing line, which isn't an answer.
        if let (Some(i), Some((_, message))) = (current, line.split_once("]: "))
            && line.contains("[Server thread/")
            && !message.starts_with("Locating element ")
        {
            let message = message.strip_prefix("System chat: ").unwrap_or(message);
            messages
                .get_mut(i)
                .with_context(|| format!("the server echoed an unknown case marker {i}"))?
                .push(message.to_string());
        }
    }
    let map_answers = if maps.is_empty() {
        Vec::new()
    } else {
        run_map_cases(&server, &mut stdin, maps)?
    };
    writeln!(stdin, "stop")?;
    stdin.flush()?;
    let status = server.child.0.wait()?;
    if !status.success() {
        bail!("the server exited with {status}");
    }
    let answers = messages
        .into_iter()
        .zip(cases)
        .map(|(lines, case)| parse(&lines, case))
        .collect::<Result<_>>()?;
    Ok((answers, map_answers))
}

/// A fresh server folder for `name`: the EULA, properties, and the world (a seed in the
/// properties, or a copy of a save).
fn server_dir(harness: &Harness, name: &str, source: &Source<'_>) -> Result<PathBuf> {
    let dir = harness
        .work
        .join("locate-server")
        .join(name.trim_end_matches(".json"));
    if dir.exists() {
        fs::remove_dir_all(&dir)?;
    }
    fs::create_dir_all(&dir)?;
    // Accepted by whoever ran this: `require_eula` checked before any server started.
    fs::write(dir.join("eula.txt"), "eula=true\n")?;
    // The world is the default level-name, "world". Port 0 binds any free port. Pausing when
    // empty would stop the tick loop that runs console commands. A `/locate` runs inside one
    // tick, and on a slow machine an old version's can pass the watchdog's default 60 s, which
    // stops the server; -1 turns the watchdog off.
    let seed = match source {
        Source::Seed(seed) => format!("level-seed={seed}\n"),
        Source::Save(save) => {
            copy_save(save, &dir.join("world"))?;
            String::new()
        }
    };
    fs::write(
        dir.join("server.properties"),
        format!(
            "{seed}server-ip=127.0.0.1\n\
             server-port=0\n\
             online-mode=false\n\
             pause-when-empty-seconds=-1\n\
             max-tick-time=-1\n\
             sync-chunk-writes=false\n\
             spawn-protection=0\n"
        ),
    )?;
    Ok(dir)
}

/// A running server and its output lines, with a deadline for the whole run.
struct Server {
    /// The server's process, killed if the run ends early.
    child: KillOnDrop,
    /// Its output, a line at a time, read on a thread of its own.
    lines: mpsc::Receiver<String>,
    /// When the whole run gives up.
    deadline: Instant,
    /// The server's folder, for its log.
    dir: PathBuf,
}

impl Server {
    /// Starts the game's server in `dir`.
    fn start(harness: &Harness, dir: &Path) -> Result<Server> {
        let mut java = Command::new(&harness.java);
        java.args(["-Xmx4G", "--enable-native-access=ALL-UNNAMED"]);
        // The game's libraries use sun.misc.Unsafe; the option exists from Java 23.
        if harness.major >= 23 {
            java.arg("--sun-misc-unsafe-memory-access=allow");
        }
        let mut child = KillOnDrop(
            java.arg("-cp")
                .arg(&harness.classpath)
                .args(["net.minecraft.server.Main", "--nogui"])
                .current_dir(dir)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .spawn()
                .with_context(|| {
                    format!("failed to start the server with {}", harness.java.display())
                })?,
        );
        let stdout = child.0.stdout.take().context("no server stdout")?;
        let (tx, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        Ok(Server {
            child,
            lines,
            deadline: Instant::now() + TIMEOUT,
            dir: dir.to_path_buf(),
        })
    }

    /// The next output line, or an error naming `what` was awaited if the server stopped or
    /// the deadline passed. The error ends with the server's own log, which says why it stopped.
    fn next(&self, what: &str) -> Result<String> {
        let left = self.deadline.saturating_duration_since(Instant::now());
        self.lines.recv_timeout(left).with_context(|| {
            let log =
                fs::read_to_string(self.dir.join("logs").join("latest.log")).unwrap_or_default();
            let lines: Vec<&str> = log.lines().collect();
            let tail = lines[lines.len().saturating_sub(20)..].join("\n");
            format!("the server stopped or timed out waiting for {what}; its log ends:\n{tail}")
        })
    }
}

/// A child process that's killed if it's still running when dropped, so an error part way
/// through a run doesn't leave a 4 GB server behind.
struct KillOnDrop(Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        if let Ok(None) = self.0.try_wait() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

/// The answer in a case's messages: `The nearest <id> is at [x, y, z] (n blocks away)`, where
/// a tag's answer is `#<tag> (<id>)`, or a failure such as `Could not find ...`. Exactly one
/// message is expected.
fn parse(lines: &[String], case: &Case) -> Result<Answer> {
    let [message] = lines else {
        bail!(
            "expected one message for locate {} {} from {:?} in {}, got {lines:?}",
            case.kind,
            case.target,
            case.from,
            case.dimension
        );
    };
    let Some(rest) = message.strip_prefix("The nearest ") else {
        return Ok(Answer {
            position: None,
            found: None,
            message: message.clone(),
        });
    };
    let (found, rest) = rest
        .split_once(" is at [")
        .with_context(|| format!("unexpected message: {message}"))?;
    let (coords, _) = rest
        .split_once(']')
        .with_context(|| format!("unexpected message: {message}"))?;
    let parts: Vec<Option<i32>> = coords
        .split(", ")
        .map(|p| {
            if p == "~" {
                Ok(None)
            } else {
                p.parse().map(Some)
            }
        })
        .collect::<Result<_, _>>()
        .with_context(|| format!("unexpected coordinates: {message}"))?;
    let [x, y, z] = parts[..] else {
        bail!("unexpected coordinates: {message}");
    };
    // "#minecraft:village (minecraft:village_plains)" names the structure in parentheses.
    let found = match found.split_once(" (") {
        Some((_, id)) => id.trim_end_matches(')'),
        None => found,
    };
    Ok(Answer {
        position: Some([x, y, z]),
        found: Some(found.to_string()),
        message: message.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_eula_is_accepted_only_when_asked() {
        assert!(require_eula(true).is_ok());
        for yes in ["1", "true", " 1 "] {
            assert!(eula_value_accepts(yes), "{yes:?}");
        }
        for no in ["", "0", "false", "yes", "TRUE"] {
            assert!(!eula_value_accepts(no), "{no:?}");
        }
    }
}
