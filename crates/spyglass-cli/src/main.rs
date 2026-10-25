//! The `spyglass` command: a thin front end over the `spyglass` library (ADR 0014). Output is
//! human-readable, or JSON with `--json` (`docs/specs/cli-json.md`).

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
use serde_json::{Value, json};
use spyglass::{LOCATE_BIOME_RADIUS, LOCATE_RADIUS, Session, World};

/// The `--json` format version (`docs/specs/cli-json.md`).
const JSON_FORMAT: u32 = 1;

/// How many hits human output lists; `--json` has them all.
const SHOWN: usize = 8;

#[derive(Parser)]
#[command(
    name = "spyglass",
    version,
    about = "Explore Minecraft: Java Edition seeds locally."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Find structures, biomes, slime chunks or the spawn for a seed, as `/locate` would.
    Locate {
        #[command(subcommand)]
        what: Locate,
    },
}

#[derive(Subcommand)]
enum Locate {
    /// The nearest structure: an id (`minecraft:mansion`) or a tag (`#minecraft:village`).
    Structure {
        /// Structure id or `#tag`.
        target: String,
        #[command(flatten)]
        common: Common,
        #[command(flatten)]
        from: From,
        /// Rings of placement cells to search; the game's `/locate` searches 100.
        #[arg(long, default_value_t = LOCATE_RADIUS)]
        radius: i32,
    },
    /// The nearest biome, searched in 3D from `--from`, as `/locate biome` does.
    Biome {
        /// Biome id, e.g. `minecraft:cherry_grove`.
        biome: String,
        #[command(flatten)]
        common: Common,
        #[command(flatten)]
        from: From,
        /// Search radius in blocks; the game's `/locate` searches 6400.
        #[arg(long, default_value_t = LOCATE_BIOME_RADIUS)]
        radius: i32,
    },
    /// Slime chunks near `--from`, nearest first.
    Slime {
        #[command(flatten)]
        common: Common,
        #[command(flatten)]
        from: From,
        /// Search radius in chunks.
        #[arg(long, default_value_t = 8)]
        radius: i32,
    },
    /// The chunk the game's world-spawn search starts from.
    Spawn {
        #[command(flatten)]
        common: Common,
    },
}

#[derive(Args)]
struct Common {
    /// The world seed: a number, or text hashed as the world-creation screen does.
    #[arg(long, allow_hyphen_values = true)]
    seed: String,
    /// Game version of the bundle to use, from `--bundles`. Defaults to the newest there.
    #[arg(long)]
    game_version: Option<String>,
    /// A bundle directory to use instead of choosing one from `--bundles`.
    #[arg(long, conflicts_with = "game_version")]
    bundle: Option<PathBuf>,
    /// Where bundles are, one directory per version.
    #[arg(long, default_value = "bundles")]
    bundles: PathBuf,
    /// Print JSON (docs/specs/cli-json.md) instead of text.
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct From {
    /// Where to search from: `x,y,z` or `x,z` (y 64), in blocks.
    #[arg(long, default_value = "0,64,0", allow_hyphen_values = true, value_parser = position)]
    from: [i32; 3],
}

fn position(text: &str) -> Result<[i32; 3], String> {
    let parts: Vec<i32> = text
        .split(',')
        .map(|p| p.trim().parse::<i32>())
        .collect::<Result<_, _>>()
        .map_err(|e| format!("{e} in {text:?}"))?;
    match parts[..] {
        [x, y, z] => Ok([x, y, z]),
        [x, z] => Ok([x, 64, z]),
        _ => Err(format!("expected x,y,z or x,z, got {text:?}")),
    }
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn open(common: &Common) -> Result<World> {
    let dir = match (&common.bundle, &common.game_version) {
        (Some(dir), _) => dir.clone(),
        (None, Some(version)) => common.bundles.join(version),
        (None, None) => {
            let found = spyglass::bundles_in(&common.bundles).with_context(|| {
                format!(
                    "no bundles in {}; extract one with spyglass-extract",
                    common.bundles.display()
                )
            })?;
            match found.into_iter().next() {
                Some(b) => b.dir,
                None => bail!(
                    "no bundles in {}; extract one with spyglass-extract",
                    common.bundles.display()
                ),
            }
        }
    };
    World::open(&dir).with_context(|| format!("loading the bundle in {}", dir.display()))
}

/// The fields every JSON answer starts with.
fn header(session: &Session<'_>, command: &str) -> serde_json::Map<String, Value> {
    let world = session.world();
    let mut map = serde_json::Map::new();
    map.insert("format".into(), json!(JSON_FORMAT));
    map.insert("command".into(), json!(command));
    map.insert("version".into(), json!(world.version()));
    map.insert("dimension".into(), json!(world.dimension()));
    map.insert("seed".into(), json!(session.seed()));
    map
}

fn print_json(mut map: serde_json::Map<String, Value>, fields: Value) -> Result<()> {
    if let Value::Object(fields) = fields {
        map.extend(fields);
    }
    println!("{}", serde_json::to_string_pretty(&Value::Object(map))?);
    Ok(())
}

fn context(session: &Session<'_>, from: Option<[i32; 3]>) -> String {
    let at = from.map_or(String::new(), |[x, y, z]| format!(" from {x} {y} {z}"));
    format!(
        "{at} (seed {}, Minecraft {})",
        session.seed(),
        session.world().version()
    )
}

fn run(cli: Cli) -> Result<()> {
    let Command::Locate { what } = cli.command;
    let common = match &what {
        Locate::Structure { common, .. }
        | Locate::Biome { common, .. }
        | Locate::Slime { common, .. }
        | Locate::Spawn { common } => common,
    };
    let seed = spyglass::parse_seed(&common.seed)?;
    let world = open(common)?;
    let session = world.session(seed)?;
    match &what {
        Locate::Structure {
            target,
            from,
            radius,
            ..
        } => structure(&session, common.json, target, from.from, *radius),
        Locate::Biome {
            biome,
            from,
            radius,
            ..
        } => self::biome(&session, common.json, biome, from.from, *radius),
        Locate::Slime { from, radius, .. } => slime(&session, common.json, from.from, *radius),
        Locate::Spawn { .. } => spawn(&session, common.json),
    }
}

fn structure(
    session: &Session<'_>,
    as_json: bool,
    target: &str,
    from: [i32; 3],
    radius: i32,
) -> Result<()> {
    let found = session.locate_structure(target, from, radius)?;
    if as_json {
        let hits: Vec<Value> = found
            .hits
            .iter()
            .map(|h| {
                json!({
                    "position": {"x": h.start.position[0], "z": h.start.position[2]},
                    "chunk": h.start.chunk,
                    "set": h.start.set,
                    "structures": h.start.structures,
                    "certain": h.start.certain,
                    "blocks_away": h.blocks_away,
                })
            })
            .collect();
        return print_json(
            header(session, "locate structure"),
            json!({
                "target": target,
                "from": from,
                "radius": radius,
                "complete": found.complete,
                "certain_answer": found.certain_answer,
                "hits": hits,
            }),
        );
    }
    println!("{target}{}", context(session, Some(from)));
    if found.hits.is_empty() {
        println!("  none within {radius} rings");
        return Ok(());
    }
    for h in found.hits.iter().take(SHOWN) {
        let [x, _, z] = h.start.position;
        println!(
            "  {:>9} ~ {:<9} {:>7} blocks  {:<10} {}",
            x,
            z,
            h.blocks_away,
            if h.start.certain {
                "certain"
            } else {
                "candidate"
            },
            h.start.structures.join(" or "),
        );
    }
    if found.hits.len() > SHOWN {
        println!(
            "  ... and {} more (--json lists all)",
            found.hits.len() - SHOWN
        );
    }
    if found.hits.iter().any(|h| !h.start.certain) {
        let rest = if found.complete && found.certain_answer.is_some() {
            "."
        } else {
            ", or further out."
        };
        println!(
            "Candidates depend on terrain (or a tied biome), which spyglass doesn't compute yet. \
             The game's answer is the first of these it generates, usually the first listed{rest}"
        );
    }
    Ok(())
}

fn biome(
    session: &Session<'_>,
    as_json: bool,
    biome: &str,
    from: [i32; 3],
    radius: i32,
) -> Result<()> {
    let found = session.locate_biome(biome, from, radius)?;
    if as_json {
        let hits: Vec<Value> = found
            .hits
            .iter()
            .map(|h| {
                json!({
                    "position": {"x": h.position[0], "y": h.position[1], "z": h.position[2]},
                    "biome": h.biome.biome,
                    "tied": h.biome.tied,
                    "certain": h.certain,
                    "blocks_away": h.blocks_away,
                })
            })
            .collect();
        return print_json(
            header(session, "locate biome"),
            json!({"target": biome, "from": from, "radius": radius, "hits": hits}),
        );
    }
    println!("{biome}{}", context(session, Some(from)));
    if found.hits.is_empty() {
        println!("  none within {radius} blocks");
        return Ok(());
    }
    for h in found.hits.iter().take(SHOWN) {
        let [x, y, z] = h.position;
        let tie = if h.biome.tied.is_empty() {
            String::new()
        } else {
            format!(" (tied: {})", h.biome.tied.join(", "))
        };
        println!(
            "  {x:>9} {y:>4} {z:<9} {:>7} blocks  {}{tie}",
            h.blocks_away,
            if h.certain { "certain" } else { "candidate" },
        );
    }
    if found.hits.iter().any(|h| !h.certain) {
        println!(
            "Candidates are tied cells, where the game's biome depends on lookup history; the \
             game's answer is the first of these where it finds the biome."
        );
    }
    Ok(())
}

fn slime(session: &Session<'_>, as_json: bool, from: [i32; 3], radius: i32) -> Result<()> {
    let centre = [from[0] >> 4, from[2] >> 4];
    let min = [
        centre[0].saturating_sub(radius),
        centre[1].saturating_sub(radius),
    ];
    let max = [
        centre[0].saturating_add(radius),
        centre[1].saturating_add(radius),
    ];
    let mut chunks = session.slime_chunks(min, max);
    let distance = |[x, z]: [i32; 2]| {
        let (dx, dz) = (
            i64::from(x) - i64::from(centre[0]),
            i64::from(z) - i64::from(centre[1]),
        );
        dx * dx + dz * dz
    };
    chunks.sort_by_key(|&c| (distance(c), c[1], c[0]));
    if as_json {
        return print_json(
            header(session, "locate slime"),
            json!({"from": from, "radius": radius, "chunks": chunks}),
        );
    }
    println!(
        "Slime chunks within {radius} chunks of chunk {} {}{}",
        centre[0],
        centre[1],
        context(session, None)
    );
    for &[x, z] in chunks.iter().take(SHOWN * 2) {
        println!(
            "  chunk {x:>6} {z:<6}  blocks {}..{}, {}..{}",
            x << 4,
            (x << 4) + 15,
            z << 4,
            (z << 4) + 15
        );
    }
    if chunks.len() > SHOWN * 2 {
        println!(
            "  ... and {} more (--json lists all)",
            chunks.len() - SHOWN * 2
        );
    }
    Ok(())
}

fn spawn(session: &Session<'_>, as_json: bool) -> Result<()> {
    let origin = session.spawn_origin();
    if as_json {
        return print_json(
            header(session, "locate spawn"),
            json!({
                "chunk": origin.chunk,
                "position": origin.position.map(|[x, _, z]| json!({"x": x, "z": z})),
                "platform_dependent": origin.platform_dependent,
            }),
        );
    }
    let [cx, cz] = origin.chunk;
    println!("World spawn{}", context(session, None));
    println!(
        "  the spawn search starts in chunk {cx} {cz} (blocks {}..{}, {}..{})",
        cx << 4,
        (cx << 4) + 15,
        cz << 4,
        (cz << 4) + 15
    );
    if let Some([x, _, z]) = origin.position {
        println!("  the climate search's best point is {x} ~ {z}");
    }
    if origin.platform_dependent {
        println!("  this may differ on other platforms (Java's sin and cos aren't exact)");
    }
    println!(
        "The spawn itself is in the 11 x 11 chunks around it, chosen by terrain, which spyglass \
         doesn't compute yet."
    );
    Ok(())
}
