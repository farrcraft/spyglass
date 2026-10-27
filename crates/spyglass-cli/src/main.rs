//! The `spyglass` command: a thin front end over the `spyglass` library (ADR 0014). Output is
//! human-readable, or JSON with `--json` (`docs/specs/cli-json.md`).

mod args;
mod heights;
mod json;
mod locate;
mod render;
mod search;
mod world;

use std::path::Path;
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::Parser;
use serde::Serialize;
use spyglass::{Dimension, Game, SaveReport, Scan, Session, World};

use crate::args::{Cli, Command, Common, Locate, Origin};

/// How many hits human output lists; `--json` has them all.
const SHOWN: usize = 8;

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Locate { what } => run_locate(&what),
        Command::World { what } => world::run(what),
        Command::Render(args) => render::run(&args),
        Command::Search(args) => search::run(&args),
        Command::Heights(args) => heights::run(&args),
    }
}

/// The world and seed `--seed` or `--world` name, and the save's report with `--world`. The
/// world is `--dimension`'s; without it, the player's if `player` (the position asked about is
/// the player's) and the save has one, otherwise the Overworld.
fn load(common: &Common, player: bool) -> Result<(World, i64, Option<SaveReport>)> {
    Ok(match (&common.world, &common.seed) {
        (Some(save), _) => {
            let dir = spyglass::find_save(save)?;
            let report = spyglass::inspect_save(&dir, &common.bundles, Scan::Quick)?;
            let players = report
                .save
                .player
                .as_ref()
                .and_then(spyglass::SaveLocation::dimension_kind)
                .filter(|_| player);
            let dimension = common.dimension.or(players).unwrap_or(Dimension::Overworld);
            let world = report.open_world(dimension)?;
            let seed = report.save.seed;
            (world, seed, Some(report))
        }
        (None, Some(seed)) => (open(common)?, spyglass::parse_seed(seed)?, None),
        (None, None) => bail!("give --seed or --world"),
    })
}

fn run_locate(what: &Locate) -> Result<()> {
    let common = what.common();
    let player = match what {
        Locate::Structure { from, .. }
        | Locate::Biome { from, .. }
        | Locate::Slime { from, .. } => {
            matches!(from.from, Origin::Player)
        }
        Locate::Spawn { .. } => false,
    };
    let (world, seed, report) = load(common, player)?;
    let session = world.session(seed)?;
    let out = Out {
        session: &session,
        json: common.json,
        report: report.as_ref(),
    };
    match what {
        Locate::Structure {
            target,
            from,
            radius,
            unconfirmed,
            bounds,
            ..
        } => {
            let options = spyglass::LocateOptions {
                radius: *radius,
                confirm: !unconfirmed,
            };
            locate::structure(&out, target, out.origin(from.from)?, &options, *bounds)?;
        }
        Locate::Biome {
            biome,
            from,
            radius,
            ..
        } => locate::biome(&out, biome, out.origin(from.from)?, *radius)?,
        Locate::Slime { from, radius, .. } => {
            locate::slime(&out, out.origin(from.from)?, *radius)?;
        }
        Locate::Spawn { .. } => locate::spawn(&out)?,
    }
    out.warn();
    Ok(())
}

fn open(common: &Common) -> Result<World> {
    let dir = bundle_dir(
        common.bundle.as_deref(),
        &common.bundles,
        common.game_version.as_deref(),
    )?;
    let dimension = common.dimension.unwrap_or(Dimension::Overworld);
    World::open_dimension(&dir, dimension)
        .with_context(|| format!("loading {dimension} from the bundle in {}", dir.display()))
}

/// The bundle `--bundle` names, or the one for `--game-version` (or the newest release) in
/// `--bundles`.
fn bundle_dir(
    bundle: Option<&Path>,
    bundles: &Path,
    version: Option<&str>,
) -> Result<std::path::PathBuf> {
    Ok(match bundle {
        Some(dir) => dir.to_path_buf(),
        None => spyglass::find_bundle(bundles, version)?.dir,
    })
}

/// [`bundle_dir`]'s bundle as a [`Game`], every dimension of it opened as it's needed.
fn open_game(bundle: Option<&Path>, bundles: &Path, version: Option<&str>) -> Result<Game> {
    let dir = bundle_dir(bundle, bundles, version)?;
    Game::open(&dir).with_context(|| format!("loading the bundle in {}", dir.display()))
}

/// What every answer is printed with.
struct Out<'a> {
    session: &'a Session<'a>,
    json: bool,
    /// The save, with `--world`.
    report: Option<&'a SaveReport>,
}

impl Out<'_> {
    /// Prints the save's warnings after a text answer.
    fn warn(&self) {
        if let (false, Some(report)) = (self.json, self.report) {
            for warning in &report.warnings {
                println!("warning: {warning}");
            }
        }
    }

    /// Fails unless the session is in the Overworld, which alone has `what`.
    fn overworld_only(&self, what: &str) -> Result<()> {
        let here = self.session.world().dimension_kind();
        if here != Dimension::Overworld {
            bail!("{what} only in the Overworld, not {}", here.name());
        }
        Ok(())
    }

    /// Resolves `--from`.
    fn origin(&self, from: Origin) -> Result<[i32; 3]> {
        match from {
            Origin::At(at) => Ok(at),
            Origin::Player => {
                let Some(report) = self.report else {
                    bail!("--from player needs --world");
                };
                let Some(player) = &report.save.player else {
                    bail!(
                        "{} has no single player (a server world?)",
                        report.save.name
                    );
                };
                let here = self.session.world().dimension_kind();
                if player.dimension_kind() != Some(here) {
                    bail!(
                        "the player is in {}, not {}; leave out --dimension to answer there",
                        player.dimension,
                        here.name()
                    );
                }
                Ok(player.block())
            }
        }
    }

    /// Prints an answer: the header for `command`, then `body`'s fields.
    fn print(&self, command: &str, body: impl Serialize) -> Result<()> {
        let world = self.session.world();
        json::print(&json::Answer {
            header: json::Header {
                format: json::FORMAT,
                command,
                version: world.version(),
                dimension: world.dimension(),
                seed: self.session.seed(),
                world: self.report.map(|r| json::WorldRef {
                    name: r.save.name.clone(),
                    dir: json::path(&r.save.dir),
                }),
                warnings: self.report.map(json::warnings),
            },
            body,
        })
    }

    /// The line after a text answer's title: where from, the save, the seed and the version.
    fn context(&self, from: Option<[i32; 3]>) -> String {
        let at = from.map_or(String::new(), |[x, y, z]| format!(" from {x} {y} {z}"));
        let world = self
            .report
            .map_or(String::new(), |r| format!("{}, ", r.save.name));
        let dimension = self.session.world().dimension_kind();
        let place = if dimension == Dimension::Overworld {
            String::new()
        } else {
            format!("{}, ", dimension.name())
        };
        format!(
            "{at} ({place}{world}seed {}, Minecraft {})",
            self.session.seed(),
            self.session.world().version()
        )
    }
}
