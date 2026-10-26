//! The `spyglass` command: a thin front end over the `spyglass` library (ADR 0014). Output is
//! human-readable, or JSON with `--json` (`docs/specs/cli-json.md`).

mod args;
mod json;
mod locate;
mod world;

use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::Parser;
use serde::Serialize;
use spyglass::{SaveReport, Scan, Session, World};

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
    let what = match cli.command {
        Command::Locate { what } => what,
        Command::World { what } => return world::run(what),
    };
    let common = what.common();
    let (world, seed, report) = match (&common.world, &common.seed) {
        (Some(save), _) => {
            let dir = spyglass::find_save(save)?;
            let report = spyglass::inspect_save(&dir, &common.bundles, Scan::Quick)?;
            let world = report.open_world()?;
            let seed = report.save.seed;
            (world, seed, Some(report))
        }
        (None, Some(seed)) => (open(common)?, spyglass::parse_seed(seed)?, None),
        (None, None) => bail!("give --seed or --world"),
    };
    let session = world.session(seed)?;
    let out = Out {
        session: &session,
        json: common.json,
        report: report.as_ref(),
    };
    match &what {
        Locate::Structure {
            target,
            from,
            radius,
            ..
        } => locate::structure(&out, target, out.origin(from.from)?, *radius)?,
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
    if let (false, Some(report)) = (out.json, &report) {
        for warning in &report.warnings {
            println!("warning: {warning}");
        }
    }
    Ok(())
}

fn open(common: &Common) -> Result<World> {
    let dir = match &common.bundle {
        Some(dir) => dir.clone(),
        None => spyglass::find_bundle(&common.bundles, common.game_version.as_deref())?.dir,
    };
    World::open(&dir).with_context(|| format!("loading the bundle in {}", dir.display()))
}

/// What every answer is printed with.
struct Out<'a> {
    session: &'a Session<'a>,
    json: bool,
    /// The save, with `--world`.
    report: Option<&'a SaveReport>,
}

impl Out<'_> {
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
                if player.dimension != "minecraft:overworld" {
                    bail!(
                        "the player is in {}; spyglass answers for the overworld only",
                        player.dimension
                    );
                }
                Ok(player.block())
            }
        }
    }

    /// Prints a locate answer: the header for `command`, then `body`'s fields.
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
        format!(
            "{at} ({world}seed {}, Minecraft {})",
            self.session.seed(),
            self.session.world().version()
        )
    }
}
