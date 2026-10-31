//! Tries against the oracle's feature golden files (`docs/specs/oracle.md#feature-golden-files`):
//! each step's order, and every target feature's tries in decoration order.
//!
//! Needs `cargo xtask oracle features` and a bundle for the same version. Run with
//! `cargo test --release -p spyglass-feature --test tries -- --ignored`.

use std::collections::HashMap;
use std::path::Path;

use rayon::prelude::*;
use spyglass_biome::BiomeSource;
use spyglass_blocks::BlockSettings;
use spyglass_data::bundle::Bundle;
use spyglass_feature::{Decorator, Drawn, FeatureSettings, Try};
use spyglass_oracle::features::FeatureFile;
use spyglass_rng::{RandomSource, WorldgenRandom};

/// One file's comparison.
#[derive(Default)]
struct Report {
    failures: Vec<String>,
    /// Tries compared and matched.
    matched: usize,
    /// Tries not compared: after a modifier that reads blocks, after a call whose draws
    /// aren't known, or on a tied biome.
    skipped: usize,
    /// Chunks decorated, and the time their tries took.
    chunks: usize,
    time: std::time::Duration,
}

fn check(path: &Path) -> Report {
    let file = FeatureFile::read(path).unwrap();
    let bundle = Bundle::open(spyglass_oracle::bundle_dir())
        .unwrap_or_else(|e| panic!("{e}; run spyglass-extract {}", file.version));
    let v1_21 = bundle
        .resource("worldgen/configured_feature", "minecraft:monster_room")
        .is_ok();
    let biomes = BiomeSource::load(&bundle, "minecraft:normal", &file.dimension).unwrap();
    let features = FeatureSettings::load(&bundle, biomes.biome_names(), v1_21).unwrap();
    let blocks = BlockSettings::load(&bundle, "minecraft:normal", &file.dimension).unwrap();
    let generator = blocks.generator(file.seed).unwrap();
    let mut report = Report::default();

    // The order, step by step.
    let ours: Vec<Vec<Option<String>>> = features
        .order()
        .iter()
        .map(|step| {
            step.iter()
                .map(|&r| features.placed(r).id.clone())
                .collect()
        })
        .collect();
    if ours != file.order {
        report.failures.push(format!(
            "seed {} {}: the order differs\n  game:     {:?}\n  spyglass: {:?}",
            file.seed, file.dimension, file.order, ours
        ));
        return report;
    }

    // The placed features the file records.
    let mut wanted = Vec::new();
    for chunk in &file.chunks {
        for t in &chunk.tries {
            let r = features
                .order()
                .iter()
                .flatten()
                .copied()
                .find(|&r| features.placed(r).id.as_deref() == Some(t.feature.as_str()))
                .unwrap_or_else(|| panic!("{} isn't in the order", t.feature));
            if !wanted.contains(&r) {
                wanted.push(r);
            }
        }
    }
    let terrain = blocks.terrain();
    let decorator = Decorator::new(
        &features,
        &generator,
        file.seed,
        terrain.anchors(),
        terrain.level_min_y,
        &wanted,
    )
    .unwrap();
    for chunk in &file.chunks {
        // The game's calls, per placed feature, in order.
        let mut calls: HashMap<&str, Vec<&spyglass_oracle::features::GoldenTry>> = HashMap::new();
        for t in &chunk.tries {
            calls.entry(t.feature.as_str()).or_default().push(t);
        }
        let mut seen: HashMap<usize, usize> = HashMap::new();
        let name = |t: &Try| features.placed(t.placed).id.clone().unwrap_or_default();
        // A declining dungeon draws its two sizes; a placing one, more than is known here.
        let mut feature = |t: &Try, random: &mut WorldgenRandom| {
            let k = seen.entry(t.placed).or_default();
            let call = calls.get(name(t).as_str()).and_then(|c| c.get(*k));
            *k += 1;
            match call {
                Some(call) if !call.placed && name(t).starts_with("minecraft:monster_room") => {
                    random.next_int_bounded(2);
                    random.next_int_bounded(2);
                    Drawn::Known
                }
                _ => Drawn::Unknown,
            }
        };
        let start = std::time::Instant::now();
        let tries = decorator.tries(chunk.chunk, &mut feature);
        report.time += start.elapsed();
        report.chunks += 1;
        let mut by_feature: HashMap<String, Vec<&Try>> = HashMap::new();
        for t in &tries {
            by_feature.entry(name(t)).or_default().push(t);
        }
        for (id, game) in &calls {
            let ours = by_feature.get(*id).map(Vec::as_slice).unwrap_or(&[]);
            for (i, g) in game.iter().enumerate() {
                match ours.get(i) {
                    Some(t) if t.exact && t.certain => {
                        if t.pos == g.pos {
                            report.matched += 1;
                        } else {
                            report.failures.push(format!(
                                "seed {} {} chunk {:?} {id} try {i}: game {:?}, spyglass {:?}",
                                file.seed, file.dimension, chunk.chunk, g.pos, t.pos
                            ));
                            break;
                        }
                    }
                    Some(_) => {
                        report.skipped += game.len() - i;
                        break;
                    }
                    None => {
                        report.failures.push(format!(
                            "seed {} {} chunk {:?} {id}: the game has {} tries, spyglass {}",
                            file.seed,
                            file.dimension,
                            chunk.chunk,
                            game.len(),
                            ours.len()
                        ));
                        break;
                    }
                }
            }
            // Exact tries the game doesn't have.
            let extra = ours
                .iter()
                .skip(game.len())
                .filter(|t| t.exact && t.certain)
                .count();
            if extra > 0 {
                report.failures.push(format!(
                    "seed {} {} chunk {:?} {id}: {extra} exact tries more than the game's {}",
                    file.seed,
                    file.dimension,
                    chunk.chunk,
                    game.len()
                ));
            }
        }
        for (id, ours) in &by_feature {
            if !calls.contains_key(id.as_str()) {
                let exact = ours.iter().filter(|t| t.exact && t.certain).count();
                if exact > 0 {
                    report.failures.push(format!(
                        "seed {} {} chunk {:?} {id}: {exact} exact tries where the game has none",
                        file.seed, file.dimension, chunk.chunk
                    ));
                }
            }
        }
    }
    report
}

#[test]
#[ignore = "needs the oracle's feature golden files and a bundle: cargo xtask oracle features"]
fn tries_match_the_game() {
    let files = spyglass_oracle::features::files();
    assert!(
        !files.is_empty(),
        "no feature files for {}; run cargo xtask oracle features",
        spyglass_oracle::version()
    );
    let reports: Vec<Report> = files.par_iter().map(|p| check(p)).collect();
    let matched: usize = reports.iter().map(|r| r.matched).sum();
    let skipped: usize = reports.iter().map(|r| r.skipped).sum();
    let failures: Vec<&String> = reports.iter().flat_map(|r| &r.failures).collect();
    let chunks: usize = reports.iter().map(|r| r.chunks).sum();
    let time: std::time::Duration = reports.iter().map(|r| r.time).sum();
    println!(
        "{} files: {matched} tries match, {skipped} not compared, {} differences; {:.1} µs a chunk's tries",
        files.len(),
        failures.len(),
        time.as_secs_f64() * 1e6 / chunks.max(1) as f64
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
