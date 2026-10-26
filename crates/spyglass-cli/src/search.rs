//! `spyglass search`: the seeds that fit a query best (`docs/specs/search.md`).

use std::io::{IsTerminal, Write};
use std::ops::ControlFlow;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use spyglass::{
    Feature, Outcome, SearchOptions, SearchOutcome, SearchProgress, SearchQuery, SeedSource, World,
};

use crate::args::Search;
use crate::json::{self, Xyz, Xz};

pub fn run(args: &Search) -> Result<()> {
    let world = crate::open_bundle(
        args.bundle.as_deref(),
        &args.bundles,
        args.game_version.as_deref(),
    )?;
    let text = match (&args.query, &args.query_file) {
        (Some(text), None) => text.clone(),
        (None, Some(path)) => {
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?
        }
        _ => bail!("give the query with -q or --query-file"),
    };
    let query = world.search_query(&text)?;
    let source = match (&args.range, &args.seeds) {
        (Some([first, last]), None) => SeedSource::Range {
            first: *first,
            last: *last,
        },
        (None, Some(path)) => SeedSource::List(read_seeds(path)?),
        (None, None) => SeedSource::All,
        (Some(_), Some(_)) => unreachable!("clap rejects --range with --seeds"),
    };
    let options = SearchOptions {
        source,
        from: args.from,
        seeds: args.count,
        time: args.time,
        top: args.top,
        threads: args.threads,
        certain: args.certain,
    };

    let stop = Arc::new(AtomicBool::new(false));
    {
        let stop = Arc::clone(&stop);
        // A second handler can't be installed; then Ctrl-C just ends the process.
        let _ = ctrlc::set_handler(move || stop.store(true, Ordering::Relaxed));
    }
    let terminal = std::io::stderr().is_terminal();
    if terminal
        && options.source == SeedSource::All
        && options.seeds.is_none()
        && options.time.is_none()
    {
        eprintln!("searching every seed; Ctrl-C stops and prints the best so far");
    }
    let mut last = Instant::now();
    let outcome = world.search(&query, &options, |p| {
        if terminal && last.elapsed() >= Duration::from_secs(1) {
            last = Instant::now();
            eprint!("\r{}", status(p));
            let _ = std::io::stderr().flush();
        }
        if stop.load(Ordering::Relaxed) {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    })?;
    if terminal {
        eprint!("\r{}\r", " ".repeat(status(&outcome.progress).len()));
    }
    if args.json {
        print_json(&world, &query, &options, &outcome)
    } else {
        print_text(&world, &query, &options, &outcome);
        Ok(())
    }
}

/// Seeds from a file: one per line, numbers or text as `--seed` takes them; blank lines are
/// skipped.
fn read_seeds(path: &std::path::Path) -> Result<Vec<i64>> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    text.lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty())
        .map(|(i, l)| {
            spyglass::parse_seed(l).with_context(|| format!("{} line {}", path.display(), i + 1))
        })
        .collect()
}

/// The progress line.
fn status(p: &SearchProgress) -> String {
    let rate = p.evaluated as f64 / p.elapsed.as_secs_f64().max(0.001);
    let best = p.top.first().map_or("none yet".to_string(), |h| {
        format!("score {} (seed {})", h.score, h.seed)
    });
    format!(
        "{} seeds ({:.0}/s), {} skipped by placement, {} reported; best {best}",
        p.evaluated, rate, p.prefiltered, p.reported
    )
}

fn outcome_word(o: Outcome) -> &'static str {
    match o {
        Outcome::Met => "met",
        Outcome::PossiblyMet => "possibly",
        Outcome::Failed => "failed",
    }
}

fn print_text(
    world: &World,
    query: &SearchQuery,
    options: &SearchOptions,
    outcome: &SearchOutcome,
) {
    let p = &outcome.progress;
    println!(
        "{} of {} seeds reported, best {} shown (Minecraft {}, {:.1} s{}):",
        p.reported,
        p.evaluated,
        outcome.hits.len(),
        world.version(),
        p.elapsed.as_secs_f64(),
        if options.certain {
            ", certain only"
        } else {
            ""
        },
    );
    for line in query.to_string().lines() {
        println!("  {line}");
    }
    let lines: Vec<String> = query.to_string().lines().map(str::to_string).collect();
    for (rank, hit) in outcome.hits.iter().enumerate() {
        println!();
        println!(
            "{:>3}. seed {}  score {}  {}",
            rank + 1,
            hit.seed,
            hit.score,
            outcome_word(hit.evaluation.verdict)
        );
        for (line, c) in lines.iter().zip(&hit.evaluation.constraints) {
            let found = match c.features.first() {
                None => String::new(),
                Some(f) => format!(
                    " ({} found{})",
                    c.features.len(),
                    feature_at(f).map_or(String::new(), |[x, z]| format!(", first at {x} {z}"))
                ),
            };
            println!("       {:<8} {line}{found}", outcome_word(c.outcome));
        }
    }
    match p.next {
        Some(next) => println!("\ncontinue with --from {next}"),
        None => println!("\nevery seed in the source was searched"),
    }
}

fn feature_at(f: &Feature) -> Option<[i32; 2]> {
    match f {
        Feature::Structure(s) => Some([s.position[0], s.position[2]]),
        Feature::SlimeChunk(c) => Some(c.map(|v| v * 16 + 8)),
        Feature::Biome { position, .. } => Some([position[0], position[2]]),
        Feature::Spawn(s) => Some(s.map_block()),
        _ => None,
    }
}

fn print_json(
    world: &World,
    query: &SearchQuery,
    options: &SearchOptions,
    outcome: &SearchOutcome,
) -> Result<()> {
    let p = &outcome.progress;
    let canonical = query.to_string();
    let lines: Vec<&str> = canonical.lines().collect();
    let source = match &options.source {
        SeedSource::All => json::Source::All,
        SeedSource::Range { first, last } => json::Source::Range {
            first: *first,
            last: *last,
        },
        SeedSource::List(seeds) => json::Source::List { count: seeds.len() },
    };
    let hits = outcome
        .hits
        .iter()
        .map(|h| json::SearchHit {
            index: h.index,
            seed: h.seed,
            score: h.score,
            verdict: outcome_word(h.evaluation.verdict),
            constraints: lines
                .iter()
                .zip(query.query().constraints())
                .zip(&h.evaluation.constraints)
                .map(|((line, c), r)| json::ConstraintAnswer {
                    constraint: line,
                    required: c.importance == spyglass::query::Importance::Required,
                    outcome: outcome_word(r.outcome),
                    best: r.best,
                    worst: r.worst,
                    features: r.features.iter().map(feature_json).collect(),
                })
                .collect(),
        })
        .collect();
    json::print(&json::SearchAnswer {
        format: json::FORMAT,
        command: "search",
        version: world.version(),
        dimension: world.dimension(),
        query: &canonical,
        source,
        from: options.from,
        certain: options.certain,
        top: options.top,
        next: p.next,
        evaluated: p.evaluated,
        prefiltered: p.prefiltered,
        checked: p.checked,
        reported: p.reported,
        elapsed_seconds: p.elapsed.as_secs_f64(),
        hits,
    })
}

fn feature_json(f: &Feature) -> json::FeatureAnswer<'_> {
    match f {
        Feature::Structure(s) => json::FeatureAnswer::Structure(json::StructureMark {
            position: Xz {
                x: s.position[0],
                z: s.position[2],
            },
            chunk: s.chunk,
            set: &s.set,
            structures: &s.structures,
            certain: s.certain,
        }),
        Feature::SlimeChunk(chunk) => json::FeatureAnswer::SlimeChunk { chunk: *chunk },
        Feature::Biome {
            position: [x, y, z],
            biome,
            certain,
        } => json::FeatureAnswer::Biome {
            position: Xyz {
                x: *x,
                y: *y,
                z: *z,
            },
            biome: &biome.biome,
            tied: &biome.tied,
            certain: *certain,
        },
        Feature::Spawn(s) => json::FeatureAnswer::Spawn(json::SpawnAnswer::new(s)),
        _ => json::FeatureAnswer::Other,
    }
}
