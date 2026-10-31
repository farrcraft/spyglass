//! Placing on the blocks, against the oracle's feature golden files
//! (`docs/specs/oracle.md#feature-golden-files`): every chunk decorated on its own, on its 3 × 3
//! carved chunks, compared call by call with the game's.
//!
//! Needs `cargo xtask oracle features` and a bundle for the same version. Run with
//! `cargo test --release -p spyglass-feature --test placing -- --ignored`.

use std::path::Path;

use rayon::prelude::*;
use spyglass_biome::BiomeSource;
use spyglass_blocks::BlockSettings;
use spyglass_data::bundle::Bundle;
use spyglass_feature::{Blocks, Decorator, Drawn, FeatureSettings, Placer, Reading, Region, Try};
use spyglass_oracle::features::FeatureFile;
use spyglass_rng::{RandomSource, WorldgenRandom};

/// One file's comparison.
#[derive(Default)]
struct Report {
    failures: Vec<String>,
    /// Calls compared and matched.
    matched: usize,
    /// Calls not compared: after one spyglass can't place yet, or on a tie.
    skipped: usize,
    chunks: usize,
    time: std::time::Duration,
    /// Calls not compared, by why the chunk's comparison stopped.
    reasons: std::collections::BTreeMap<String, usize>,
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
    let name = |t: &Try| features.placed(t.placed).id.clone().unwrap_or_default();
    // Neighbouring golden chunks share their carved chunks.
    let mut carved = std::collections::HashMap::new();
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
        let game = &chunk.tries;
        let mut next = 0;
        let mut stopped = false;
        let mut reason = String::new();
        let mut failures = Vec::new();
        let mut matched = 0;
        let mut place = |t: &Try, random: &mut WorldgenRandom, region: &mut Region<'_>| {
            if stopped {
                return Drawn::Unknown;
            }
            let ours = (name(t), t.pos);
            let Some(g) = game.get(next) else {
                failures.push(format!(
                    "seed {} chunk {:?}: spyglass calls {} at {:?} after the game's last",
                    file.seed, chunk.chunk, ours.0, ours.1
                ));
                stopped = true;
                return Drawn::Unknown;
            };
            if !t.certain {
                reason = "tied biome at the try".into();
                stopped = true;
                return Drawn::Unknown;
            }
            if ours.0 != g.feature || ours.1 != g.pos {
                failures.push(format!(
                    "seed {} chunk {:?} call {next}: game {} at {:?}, spyglass {} at {:?}",
                    file.seed, chunk.chunk, g.feature, g.pos, ours.0, ours.1
                ));
                stopped = true;
                return Drawn::Unknown;
            }
            next += 1;
            region.take_doubt();
            if let Some(placed) = placer.place(t.placed, region, random, t.pos, file.seed) {
                if let Some(why) = region.take_doubt() {
                    // Not compared: the answer isn't certain.
                    reason = format!("{why:?} in {}", g.feature);
                    next -= 1;
                    stopped = true;
                    return Drawn::Unknown;
                }
                if placed != g.placed {
                    failures.push(format!(
                        "seed {} chunk {:?} call {}: {} at {:?}: game {}, spyglass {}",
                        file.seed,
                        chunk.chunk,
                        next - 1,
                        g.feature,
                        g.pos,
                        if g.placed { "placed" } else { "declined" },
                        if placed { "placed" } else { "declined" },
                    ));
                    stopped = true;
                    return Drawn::Unknown;
                }
                matched += 1;
                return Drawn::Known;
            }
            matched += 1;
            if g.placed {
                // Its blocks aren't placed yet: nothing after is known.
                reason = format!("{} isn't placed", g.feature);
                stopped = true;
                return Drawn::Unknown;
            }
            if g.feature.starts_with("minecraft:monster_room") {
                random.next_int_bounded(2);
                random.next_int_bounded(2);
            }
            Drawn::Known
        };
        decorator.decorate(&mut region, &mut place);
        if !stopped && next < game.len() {
            failures.push(format!(
                "seed {} chunk {:?}: the game has {} calls, spyglass {next}",
                file.seed,
                chunk.chunk,
                game.len()
            ));
        }
        let skipped = game.len().saturating_sub(next);
        if skipped > 0 {
            *report.reasons.entry(reason).or_default() += skipped;
        }
        report.skipped += skipped;
        report.matched += matched;
        report.failures.extend(failures);
        report.time += start.elapsed();
        report.chunks += 1;
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
    let skipped: usize = reports.iter().map(|r| r.skipped).sum();
    let failures: Vec<&String> = reports.iter().flat_map(|r| &r.failures).collect();
    let chunks: usize = reports.iter().map(|r| r.chunks).sum();
    let time: std::time::Duration = reports.iter().map(|r| r.time).sum();
    let mut reasons = std::collections::BTreeMap::new();
    for r in &reports {
        for (k, v) in &r.reasons {
            *reasons.entry(k.clone()).or_insert(0) += v;
        }
    }
    for (k, v) in &reasons {
        println!("  not compared, {k}: {v}");
    }
    println!(
        "{} files: {matched} calls match, {skipped} not compared, {} differences; {:.2} ms a chunk",
        files.len(),
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
