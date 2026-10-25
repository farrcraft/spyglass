//! `cargo xtask oracle locate`: the real game's `/locate` answers, from a dedicated server.
//!
//! `ChunkGenerator.findNearestMapStructure` needs a live level, so instead of the standalone
//! harness this runs the server from the same jar on a throwaway world per seed, and sends
//! `execute positioned ... run locate ...` on its console. See docs/specs/oracle.md.

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
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

/// Where each case is run from, as block positions.
const FROM: &[[i32; 3]] = &[[0, 64, 0], [1500, 64, -2500], [-20000, -30, 7000]];

/// `/locate structure` arguments: every Overworld kind, as ids or tags.
const STRUCTURES: &[&str] = &[
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

/// `/locate biome` arguments: common, rare, and underground (the search is 3D).
const BIOMES: &[&str] = &[
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

/// With no `worlds`, writes `oracle-out/<version>/locate/<seed>.json` for each seed, replacing
/// the folder. With `worlds`, writes `world-<folder>.json` for each save, from a copy of it,
/// with the player's position as an extra origin.
pub fn run(harness: &Harness, worlds: &[PathBuf]) -> Result<()> {
    let out = repo_root()
        .join("oracle-out")
        .join(&harness.version)
        .join("locate");
    if worlds.is_empty() && out.exists() {
        fs::remove_dir_all(&out)?;
    }
    fs::create_dir_all(&out)?;
    let mut jobs: Vec<(String, Source<'_>, Vec<[i32; 3]>, Value)> = Vec::new();
    if worlds.is_empty() {
        for seed in SEEDS {
            jobs.push((
                format!("{seed}.json"),
                Source::Seed(seed),
                FROM.to_vec(),
                json!(seed),
            ));
        }
    }
    for world in worlds {
        let save = Save::read(world).with_context(|| format!("reading {}", world.display()))?;
        let mut from = FROM.to_vec();
        if let Some(player) = save
            .player
            .as_ref()
            .filter(|p| p.dimension == "minecraft:overworld")
        {
            from.push(player.pos.map(|v| v.floor() as i32));
        }
        let folder = world
            .file_name()
            .context("a save folder")?
            .to_string_lossy()
            .into_owned();
        jobs.push((
            format!("world-{folder}.json"),
            Source::Save(world),
            from,
            json!(save.seed.to_string()),
        ));
    }
    for (name, source, from, seed) in jobs {
        let t0 = Instant::now();
        let file = out.join(&name);
        let cases = cases(&from);
        let answers = server(harness, &name, &source, &cases)?;
        let rows: Vec<Value> = cases
            .iter()
            .zip(answers)
            .map(|(case, answer)| {
                json!({
                    "kind": case.kind,
                    "target": case.target,
                    "from": case.from,
                    "answer": answer.position,
                    "found": answer.found,
                    "message": answer.message,
                })
            })
            .collect();
        let mut root = json!({
            "format": 1,
            "version": harness.version,
            "seed": seed,
            "dimension": "minecraft:overworld",
            "cases": rows,
        });
        if let Source::Save(dir) = source {
            root["world_dir"] = json!(dir);
        }
        fs::write(&file, serde_json::to_string_pretty(&root)? + "\n")?;
        println!("{}: {:.0} s", file.display(), t0.elapsed().as_secs_f64());
    }
    Ok(())
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

struct Case {
    kind: &'static str,
    target: &'static str,
    from: [i32; 3],
}

fn cases(origins: &[[i32; 3]]) -> Vec<Case> {
    let mut cases = Vec::new();
    for &from in origins {
        for &target in STRUCTURES {
            cases.push(Case {
                kind: "structure",
                target,
                from,
            });
        }
        for &target in BIOMES {
            cases.push(Case {
                kind: "biome",
                target,
                from,
            });
        }
    }
    cases
}

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
) -> Result<Vec<Answer>> {
    let dir = harness
        .work
        .join("locate-server")
        .join(name.trim_end_matches(".json"));
    if dir.exists() {
        fs::remove_dir_all(&dir)?;
    }
    fs::create_dir_all(&dir)?;
    fs::write(dir.join("eula.txt"), "eula=true\n")?;
    // The world is the default level-name, "world". Port 0 binds any free port. Pausing when
    // empty would stop the tick loop that runs console commands.
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
            "{seed}server-ip=127.0.0.1\nserver-port=0\nonline-mode=false\n\
             pause-when-empty-seconds=-1\nsync-chunk-writes=false\nspawn-protection=0\n"
        ),
    )?;

    let mut java = Command::new(&harness.java);
    java.args(["-Xmx4G", "--enable-native-access=ALL-UNNAMED"]);
    // The game's libraries use sun.misc.Unsafe; the option exists from Java 23.
    if harness.major >= 23 {
        java.arg("--sun-misc-unsafe-memory-access=allow");
    }
    let mut child = java
        .arg("-cp")
        .arg(&harness.classpath)
        .args(["net.minecraft.server.Main", "--nogui"])
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .with_context(|| format!("failed to start the server with {}", harness.java.display()))?;
    let stdout = child.stdout.take().context("no server stdout")?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let deadline = Instant::now() + TIMEOUT;
    let next = |what: &str| -> Result<String> {
        let left = deadline.saturating_duration_since(Instant::now());
        rx.recv_timeout(left)
            .with_context(|| format!("the server stopped or timed out waiting for {what}"))
    };

    // "Done (12.345s)! For help, type "help"".
    loop {
        let line = next("startup")?;
        if line.contains("]: Done (") {
            break;
        }
    }
    let mut stdin = child.stdin.take().context("no server stdin")?;
    for (i, case) in cases.iter().enumerate() {
        let [x, y, z] = case.from;
        writeln!(stdin, "say spyglass-case-{i}")?;
        writeln!(
            stdin,
            "execute positioned {x} {y} {z} run locate {} {}",
            case.kind, case.target
        )?;
    }
    writeln!(stdin, "say spyglass-case-end")?;
    writeln!(stdin, "stop")?;
    stdin.flush()?;

    // Each case's lines: from its marker to the next.
    let mut messages: Vec<Vec<String>> = vec![Vec::new(); cases.len()];
    let mut current: Option<usize> = None;
    loop {
        let line = next("locate answers")?;
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
            messages[i].push(message.to_string());
        }
    }
    let status = child.wait()?;
    if !status.success() {
        bail!("the server exited with {status}");
    }
    messages
        .into_iter()
        .zip(cases)
        .map(|(lines, case)| parse(&lines, case))
        .collect()
}

/// The answer in a case's messages: `The nearest <id> is at [x, y, z] (n blocks away)`, where
/// a tag's answer is `#<tag> (<id>)`, or a failure such as `Could not find ...`. Exactly one
/// message is expected.
fn parse(lines: &[String], case: &Case) -> Result<Answer> {
    let [message] = lines else {
        bail!(
            "expected one message for locate {} {} from {:?}, got {lines:?}",
            case.kind,
            case.target,
            case.from
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
