//! Placing on the blocks, against the oracle's feature golden files
//! (`docs/specs/oracle.md#feature-golden-files`): every chunk decorated on its own, on its 3 × 3
//! carved chunks, with every feature of the steps up to the last target's, compared call by
//! call with the game's: every certain answer must be the game's.
//!
//! Needs `cargo xtask oracle features` and a bundle for the same version. Run with
//! `cargo test --release -p spyglass-feature --test placing -- --ignored`.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use rayon::prelude::*;
use spyglass_biome::BiomeSource;
use spyglass_blocks::BlockSettings;
use spyglass_data::bundle::Bundle;
use spyglass_feature::{Blocks, Decorator, FeatureSettings, Outcome, Placer, Reading, Region};
use spyglass_oracle::features::FeatureFile;

/// One file's comparison.
#[derive(Default)]
struct Report {
    failures: Vec<String>,
    /// Calls compared and matched.
    matched: usize,
    /// Calls not compared: from a chunk's first candidate on, by its reason.
    skipped: BTreeMap<String, usize>,
    chunks: usize,
    time: std::time::Duration,
}

fn check(path: &Path) -> Report {
    let file = FeatureFile::read(path).unwrap();
    let mut report = Report::default();
    if file.chunks.is_empty() {
        return report;
    }
    let bundle = Bundle::open(spyglass_oracle::bundle_dir())
        .unwrap_or_else(|e| panic!("{e}; run spyglass-extract {}", file.version));
    let v1_21 = bundle
        .resource("worldgen/configured_feature", "minecraft:monster_room")
        .is_ok();
    let biomes = BiomeSource::load(&bundle, "minecraft:normal", &file.dimension).unwrap();
    let features = FeatureSettings::load(&bundle, biomes.biome_names(), v1_21).unwrap();
    let settings = BlockSettings::load(&bundle, "minecraft:normal", &file.dimension).unwrap();
    let generator = settings.generator(file.seed).unwrap();
    let mut blocks = Blocks::new(&bundle, settings.palette()).unwrap();

    // The targets the file records, and every feature of the steps up to the last of theirs.
    let id = |r: usize| features.placed(r).id.clone().unwrap_or_default();
    let mut targets = Vec::new();
    for chunk in &file.chunks {
        for t in &chunk.tries {
            let r = features
                .order()
                .iter()
                .flatten()
                .copied()
                .find(|&r| id(r) == t.feature)
                .unwrap_or_else(|| panic!("{} isn't in the order", t.feature));
            if !targets.contains(&r) {
                targets.push(r);
            }
        }
    }
    let last_step = features
        .order()
        .iter()
        .rposition(|step| step.iter().any(|r| targets.contains(r)))
        .unwrap_or(0);
    let wanted: Vec<usize> = features.order()[..=last_step]
        .iter()
        .flatten()
        .copied()
        .collect();
    let placer = Placer::load(&bundle, &features, &mut blocks, &wanted).unwrap();
    let terrain = settings.terrain();
    let mut reading = Reading {
        resources: &bundle,
        blocks: &mut blocks,
    };
    let decorator = Decorator::with_reading(
        &features,
        &generator,
        file.seed,
        terrain.anchors(),
        terrain.level_min_y,
        &wanted,
        Some(&mut reading),
    )
    .unwrap();
    // Neighbouring golden chunks share their carved chunks.
    let mut carved = HashMap::new();
    for chunk in &file.chunks {
        let [cx, cz] = chunk.chunk;
        let start = std::time::Instant::now();
        let mut chunks = Vec::with_capacity(9);
        for dz in -1..=1 {
            for dx in -1..=1 {
                let key = [cx + dx, cz + dz];
                let c = carved
                    .entry(key)
                    .or_insert_with(|| generator.carved_chunk(key[0], key[1]));
                chunks.push(c.clone());
            }
        }
        let mut region = Region::new(&blocks, chunk.chunk, chunks);
        // The golden files record the targets' calls.
        let ours: Vec<_> = placer
            .decorate(&decorator, &mut region, file.seed)
            .into_iter()
            .filter(|p| targets.contains(&p.at.placed))
            .collect();
        report.time += start.elapsed();
        report.chunks += 1;
        let game = &chunk.tries;
        let at = |i: usize| format!("seed {} chunk {:?} call {i}", file.seed, chunk.chunk);
        let mut compared = 0;
        let mut reason = None;
        for (i, g) in game.iter().enumerate() {
            let Some(p) = ours.get(i) else {
                report.failures.push(format!(
                    "{}: the game calls {} at {:?}, spyglass has no more",
                    at(i),
                    g.feature,
                    g.pos
                ));
                break;
            };
            let placed = match p.outcome {
                Outcome::Candidate { why, .. } => {
                    let why = match why {
                        spyglass_feature::Doubt::OtherFeature(r) => format!("after {}", id(r)),
                        why => format!("{why:?}"),
                    };
                    reason = Some(format!("{why} at {}", id(p.at.placed)));
                    break;
                }
                Outcome::Placed => true,
                Outcome::Declined => false,
            };
            if id(p.at.placed) != g.feature || p.at.pos != g.pos || placed != g.placed {
                report.failures.push(format!(
                    "{}: game {} at {:?} {}, spyglass {} at {:?} {}",
                    at(i),
                    g.feature,
                    g.pos,
                    if g.placed { "placed" } else { "declined" },
                    id(p.at.placed),
                    p.at.pos,
                    if placed { "placed" } else { "declined" },
                ));
                break;
            }
            compared += 1;
        }
        report.matched += compared;
        if let Some(reason) = reason {
            *report.skipped.entry(reason).or_default() += game.len() - compared;
        } else if compared == game.len() {
            let extra = ours[compared..]
                .iter()
                .filter(|p| !matches!(p.outcome, Outcome::Candidate { .. }))
                .count();
            if extra > 0 {
                report.failures.push(format!(
                    "seed {} chunk {:?}: {extra} certain calls after the game's last",
                    file.seed, chunk.chunk
                ));
            }
        }
    }
    report
}

#[test]
#[ignore = "needs the oracle's feature golden files and a bundle: cargo xtask oracle features"]
fn placing_matches_the_game() {
    let files = spyglass_oracle::features::files();
    assert!(
        !files.is_empty(),
        "no feature files for {}; run cargo xtask oracle features",
        spyglass_oracle::version()
    );
    let reports: Vec<Report> = files.par_iter().map(|p| check(p)).collect();
    let matched: usize = reports.iter().map(|r| r.matched).sum();
    let failures: Vec<&String> = reports.iter().flat_map(|r| &r.failures).collect();
    let chunks: usize = reports.iter().map(|r| r.chunks).sum();
    let time: std::time::Duration = reports.iter().map(|r| r.time).sum();
    let mut skipped = BTreeMap::new();
    for r in &reports {
        for (k, v) in &r.skipped {
            *skipped.entry(k.clone()).or_insert(0) += v;
        }
    }
    for (k, v) in &skipped {
        println!("  not compared, {k}: {v}");
    }
    println!(
        "{} files: {matched} calls match, {} not compared, {} differences; {:.2} ms a chunk",
        files.len(),
        skipped.values().sum::<usize>(),
        failures.len(),
        time.as_secs_f64() * 1e3 / chunks.max(1) as f64
    );
    assert!(
        failures.is_empty(),
        "{} differences:\n{}",
        failures.len(),
        failures
            .iter()
            .take(30)
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    );
}
