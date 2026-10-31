//! `spyglass find`: places in one seed that fit a `find` query, nearest first (ADR 0047,
//! `docs/specs/search.md`).

use std::io::{IsTerminal, Write};
use std::ops::ControlFlow;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use spyglass::{
    Dimension, Feature, FindOptions, FindOutcome, FindProgress, Game, SaveReport, Scan,
    SearchQuery, SearchStop, SearchThrottle,
};

use crate::args::{Find, FindOrigin};
use crate::json::{self, Xz};
use crate::search::{feature_json, outcome_word};

pub fn run(args: &Find) -> Result<()> {
    let (game, seed, report) = match (&args.world, &args.seed) {
        (Some(save), _) => {
            let dir = spyglass::find_save(save)?;
            let report = spyglass::inspect_save(&dir, &args.bundles, Scan::Quick)?;
            (report.open_game()?, report.save.seed, Some(report))
        }
        (None, Some(seed)) => (
            crate::open_game(
                args.bundle.as_deref(),
                &args.bundles,
                args.game_version.as_deref(),
            )?,
            spyglass::parse_seed(seed)?,
            None,
        ),
        (None, None) => bail!("give --seed or --world"),
    };
    crate::warn_unchecked(game.unchecked_version(), args.json);
    let text = match (&args.query, &args.query_file) {
        (Some(text), None) => text.clone(),
        (None, Some(path)) => {
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?
        }
        _ => bail!("give the query with -q or --query-file"),
    };
    let query = game.search_query(&text)?;
    let Some(dimension) = query.find_dimension() else {
        bail!(
            "the query has no `find` line: start it with one, e.g. `find minecraft:swamp_hut \
             within 4096`, or search seeds with `spyglass search`"
        );
    };
    let origin = match args.from {
        FindOrigin::At(p) => p,
        FindOrigin::Player => {
            let Some(player) = report.as_ref().and_then(|r| r.save.player.as_ref()) else {
                bail!("--from player needs a --world with a single player");
            };
            let from = player.dimension_kind().unwrap_or(Dimension::Overworld);
            let [x, _, z] = player.pos.map(|v| v.floor() as i32);
            game.convert([x, z], from, dimension)?
        }
    };
    let options = FindOptions {
        origin,
        top: args.top,
        threads: args.threads,
        certain: args.certain,
        confirm: args.confirm,
        time: args.time,
        stop: SearchStop::new(),
        throttle: SearchThrottle::new(),
    };
    {
        // Ctrl-C stops, and the matches so far are printed.
        let stop = options.stop.clone();
        let _ = ctrlc::set_handler(move || {
            stop.stop();
            eprintln!("\nstopping; the matches so far follow");
        });
    }
    let terminal = std::io::stderr().is_terminal();
    let mut last = Instant::now();
    let outcome = game.session(seed).find(&query, &options, |p| {
        if terminal && last.elapsed() >= Duration::from_secs(1) {
            last = Instant::now();
            eprint!("\r{}", status(p));
            let _ = std::io::stderr().flush();
        }
        ControlFlow::Continue(())
    })?;
    if terminal {
        eprint!("\r{}\r", " ".repeat(status(&outcome.progress).len()));
    }
    if args.json {
        print_json(
            &game,
            seed,
            report.as_ref(),
            dimension,
            &query,
            &options,
            &outcome,
        )
    } else {
        print_text(&game, seed, &query, &options, &outcome);
        for warning in report.iter().flat_map(|r| &r.warnings) {
            out!("warning: {warning}");
        }
        Ok(())
    }
}

/// The progress line.
fn status(p: &FindProgress) -> String {
    format!(
        "{} of {} targets judged, {} matches",
        p.judged, p.targets, p.matches
    )
}

/// The query's constraint lines: its canonical text without the `find` line.
fn constraint_lines(query: &SearchQuery) -> Vec<String> {
    query
        .to_string()
        .lines()
        .skip(1)
        .map(str::to_string)
        .collect()
}

fn print_text(
    game: &Game,
    seed: i64,
    query: &SearchQuery,
    options: &FindOptions,
    outcome: &FindOutcome,
) {
    let p = &outcome.progress;
    out!(
        "{} matches, {} of {} targets judged (seed {seed}, Minecraft {}, {:.1} s{}):",
        outcome.matches.len(),
        p.judged,
        p.targets,
        game.version(),
        p.elapsed.as_secs_f64(),
        match (options.certain, options.confirm) {
            (true, true) => ", confirmed, certain only",
            (true, false) => ", certain only",
            (false, true) => ", confirmed",
            (false, false) => "",
        },
    );
    for line in query.to_string().lines() {
        out!("  {line}");
    }
    let lines = constraint_lines(query);
    for (n, m) in outcome.matches.iter().enumerate() {
        let what = match &m.target {
            Some(Feature::Structure(s)) => s.structures.join(" or "),
            Some(Feature::Placed(f)) => f.feature.clone(),
            Some(Feature::Biome { biome, .. }) => biome.biome.clone(),
            _ => "point".to_string(),
        };
        out!();
        out!(
            "{:>3}. {:>9} ~ {:<9} {:>6} blocks  score {}  {}  {what}",
            n + 1,
            m.position[0],
            m.position[1],
            m.distance,
            m.score,
            outcome_word(m.evaluation.verdict),
        );
        for (line, c) in lines.iter().zip(&m.evaluation.constraints) {
            let found = match c.features.first() {
                None => String::new(),
                Some(f) => {
                    let [x, z] = f.map_block();
                    format!(" ({} found, first at {x} {z})", c.features.len())
                }
            };
            out!("       {:<8} {line}{found}", outcome_word(c.outcome));
        }
    }
    if !outcome.complete {
        out!("\nstopped before every target was judged");
    }
}

fn print_json(
    game: &Game,
    seed: i64,
    report: Option<&SaveReport>,
    dimension: Dimension,
    query: &SearchQuery,
    options: &FindOptions,
    outcome: &FindOutcome,
) -> Result<()> {
    let canonical = query.to_string();
    let lines = constraint_lines(query);
    let matches = outcome
        .matches
        .iter()
        .map(|m| json::FindMatch {
            position: Xz {
                x: m.position[0],
                z: m.position[1],
            },
            distance: m.distance,
            target: m.target.as_ref().map(feature_json),
            score: m.score,
            verdict: outcome_word(m.evaluation.verdict),
            constraints: lines
                .iter()
                .zip(query.query().constraints())
                .zip(&m.evaluation.constraints)
                .map(|((line, c), r)| json::ConstraintAnswer {
                    constraint: line,
                    dimension: r.dimension.id(),
                    required: c.importance == spyglass::query::Importance::Required,
                    outcome: outcome_word(r.outcome),
                    best: r.best,
                    worst: r.worst,
                    features: r.features.iter().map(feature_json).collect(),
                })
                .collect(),
        })
        .collect();
    let p = &outcome.progress;
    json::print(&json::FindAnswer {
        header: json::Header {
            format: json::FORMAT,
            command: "find",
            version: game.version(),
            dimension: dimension.id(),
            seed,
            world: report.map(|r| json::WorldRef {
                name: r.save.name.clone(),
                dir: json::path(&r.save.dir),
            }),
            warnings: json::all_warnings(report, game.unchecked_version()),
        },
        query: &canonical,
        origin: Xz {
            x: options.origin[0],
            z: options.origin[1],
        },
        certain: options.certain,
        confirm: options.confirm,
        top: options.top,
        targets: p.targets,
        judged: p.judged,
        complete: outcome.complete,
        elapsed_seconds: p.elapsed.as_secs_f64(),
        matches,
    })
}
