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
    let mut jobs: Vec<(String, Source<'_>, Vec<Case>, Value)> = Vec::new();
    if worlds.is_empty() {
        for seed in SEEDS {
            jobs.push((
                format!("{seed}.json"),
                Source::Seed(seed),
                cases(None),
                json!(seed),
            ));
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
        jobs.push((
            format!("world-{folder}.json"),
            Source::Save(world),
            cases(player),
            json!(save.seed.to_string()),
        ));
    }
    for (name, source, cases, seed) in jobs {
        let t0 = Instant::now();
        let file = out.join(&name);
        let answers = server(harness, &name, &source, &cases)?;
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
        let mut root = json!({
            "format": 2,
            "version": harness.version,
            "seed": seed,
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
    dimension: &'static str,
    kind: &'static str,
    target: &'static str,
    from: [i32; 3],
}

/// Every dimension's cases from its origins, plus `extra`'s origin in its dimension (a
/// save's player).
fn cases(extra: Option<(&str, [i32; 3])>) -> Vec<Case> {
    let mut cases = Vec::new();
    for d in DIMENSIONS {
        let mut origins = d.from.to_vec();
        if let Some((dimension, at)) = extra
            && dimension == d.dimension
        {
            origins.push(at);
        }
        for from in origins {
            for &target in d.structures {
                cases.push(Case {
                    dimension: d.dimension,
                    kind: "structure",
                    target,
                    from,
                });
            }
            for &target in d.biomes {
                cases.push(Case {
                    dimension: d.dimension,
                    kind: "biome",
                    target,
                    from,
                });
            }
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
    let dir = server_dir(harness, name, source)?;
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
    writeln!(stdin, "stop")?;
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
    let status = server.child.0.wait()?;
    if !status.success() {
        bail!("the server exited with {status}");
    }
    messages
        .into_iter()
        .zip(cases)
        .map(|(lines, case)| parse(&lines, case))
        .collect()
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
    fs::write(
        dir.join("eula.txt"),
        "eula=true
",
    )?;
    // The world is the default level-name, "world". Port 0 binds any free port. Pausing when
    // empty would stop the tick loop that runs console commands.
    let seed = match source {
        Source::Seed(seed) => format!(
            "level-seed={seed}
"
        ),
        Source::Save(save) => {
            copy_save(save, &dir.join("world"))?;
            String::new()
        }
    };
    fs::write(
        dir.join("server.properties"),
        format!(
            "{seed}server-ip=127.0.0.1
server-port=0
online-mode=false
             pause-when-empty-seconds=-1
sync-chunk-writes=false
spawn-protection=0
"
        ),
    )?;
    Ok(dir)
}

/// A running server and its output lines, with a deadline for the whole run.
struct Server {
    child: KillOnDrop,
    lines: mpsc::Receiver<String>,
    deadline: Instant,
}

impl Server {
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
        })
    }

    /// The next output line, or an error naming `what` was awaited if the server stopped or
    /// the deadline passed.
    fn next(&self, what: &str) -> Result<String> {
        let left = self.deadline.saturating_duration_since(Instant::now());
        self.lines
            .recv_timeout(left)
            .with_context(|| format!("the server stopped or timed out waiting for {what}"))
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
