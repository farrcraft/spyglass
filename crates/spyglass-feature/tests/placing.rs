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
use spyglass_feature::structures;
use spyglass_feature::{
    Blocks, Decorator, Doubt, FeatureSettings, NeighbourBounds, Outcome, Placer, Reading, Region,
};
use spyglass_oracle::features::{FeatureFile, GoldenTry};
use spyglass_structure::{JigsawPools, Starts, StructureSets};
use spyglass_terrain::Terrain;

/// One file's comparison.
#[derive(Default)]
struct Report {
    failures: Vec<String>,
    /// Calls compared and matched.
    matched: usize,
    /// Of those, the calls that hold whatever the neighbours do.
    robust: usize,
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
    // What neighbours decorated first could write: every placed feature of theirs.
    let all: Vec<usize> = features.order().iter().flatten().copied().collect();
    let everything = Decorator::new(
        &features,
        &generator,
        file.seed,
        terrain.anchors(),
        terrain.level_min_y,
        &all,
    )
    .unwrap();
    let bounds = NeighbourBounds::load(&bundle, &features).unwrap();
    // Structures, laid out where they're jigsaws.
    let sets = StructureSets::load(&bundle, "minecraft:normal", &file.dimension, &biomes).unwrap();
    let pools = JigsawPools::load(&bundle, &sets).unwrap();
    let seeded = biomes.with_seed(file.seed).unwrap();
    let placements = sets.with_seed(file.seed, &seeded);
    let terrain_of_seed = Terrain::new(terrain, file.seed).unwrap();
    let starts = Starts::with_terrain(&placements, &seeded, &terrain_of_seed).with_pools(&pools);
    // Neighbouring golden chunks share their carved chunks.
    let mut carved = HashMap::new();
    let mut stored = HashMap::new();
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
        let mut biomes = |c: [i32; 2]| {
            stored
                .entry(c)
                .or_insert_with(|| generator.chunk_biomes(c[0], c[1]))
                .clone()
        };
        let foreign = bounds.writes(&everything, &features, &region, &mut biomes);
        let pieces = structures::pieces(
            &starts,
            &sets,
            chunk.chunk,
            last_step,
            region.min_y(),
            region.max_y(),
        );
        let own = bounds.own_writes(&everything, &features, &region, &mut biomes);
        let ours: Vec<_> = placer
            .decorate(&decorator, &mut region, file.seed, &foreign, &own, &pieces)
            .into_iter()
            .filter(|p| targets.contains(&p.at.placed))
            .collect();
        report.time += start.elapsed();
        report.chunks += 1;
        let game = &chunk.tries;
        // Each placed feature's calls in order, the game's and spyglass's: a feature's tries
        // depend only on its own draws and the blocks, so each is compared on its own, up to
        // its first candidate.
        let by_feature = |calls: &[GoldenTry]| {
            let mut m: BTreeMap<String, Vec<GoldenTry>> = BTreeMap::new();
            for c in calls {
                m.entry(c.feature.clone()).or_default().push(c.clone());
            }
            m
        };
        let games = by_feature(game);
        let afters = chunk.after_neighbours.as_deref().map(by_feature);
        let mut mine: BTreeMap<String, Vec<&spyglass_feature::Placement>> = BTreeMap::new();
        for p in &ours {
            mine.entry(id(p.at.placed)).or_default().push(p);
        }
        for (feature, calls) in &games {
            let empty = Vec::new();
            let ours = mine.get(feature).unwrap_or(&empty);
            let mut compared = 0;
            let mut reason = None;
            for (i, g) in calls.iter().enumerate() {
                let Some(p) = ours.get(i) else {
                    report.failures.push(format!(
                        "seed {} chunk {:?} {feature} call {i}: the game calls at {:?}, spyglass \
                         has no more",
                        file.seed, chunk.chunk, g.pos
                    ));
                    break;
                };
                let placed = match p.outcome {
                    Outcome::Candidate { why, .. } => {
                        let why = match why {
                            Doubt::OtherFeature(r) => format!("after {}", id(r)),
                            Doubt::Neighbours(r) => format!("a neighbour's {}", id(r)),
                            Doubt::Structure(i) => format!("{}'s pieces", sets.structures()[i].id),
                            why => format!("{why:?}"),
                        };
                        reason = Some(format!("{why} at {feature}"));
                        break;
                    }
                    Outcome::Placed => true,
                    Outcome::Declined => false,
                };
                if p.at.pos != g.pos || placed != g.placed {
                    report.failures.push(format!(
                        "seed {} chunk {:?} {feature} call {i}: game {:?} {}, spyglass {:?} {}",
                        file.seed,
                        chunk.chunk,
                        g.pos,
                        if g.placed { "placed" } else { "declined" },
                        p.at.pos,
                        if placed { "placed" } else { "declined" },
                    ));
                    break;
                }
                compared += 1;
            }
            report.matched += compared;
            // Where neighbours decorated first change a call, spyglass must say they could.
            let robust = ours.iter().take_while(|p| p.neighbours.is_none()).count();
            report.robust += robust.min(compared);
            if let Some(afters) = &afters {
                let after = afters.get(feature).map(Vec::as_slice).unwrap_or(&[]);
                let first = calls
                    .iter()
                    .zip(after)
                    .position(|(a, b)| a != b)
                    .or((calls.len() != after.len()).then(|| calls.len().min(after.len())));
                if let Some(d) = first
                    && robust > d
                {
                    report.failures.push(format!(
                        "seed {} chunk {:?} {feature}: neighbours decorated first change call \
                         {d}, which spyglass answers as certain",
                        file.seed, chunk.chunk
                    ));
                }
            }
            match reason {
                Some(reason) => {
                    *report.skipped.entry(reason).or_default() += calls.len() - compared
                }
                None => {
                    let extra = ours[compared.min(ours.len())..]
                        .iter()
                        .filter(|p| !matches!(p.outcome, Outcome::Candidate { .. }))
                        .count();
                    if extra > 0 {
                        report.failures.push(format!(
                            "seed {} chunk {:?} {feature}: {extra} certain calls after the \
                             game's last",
                            file.seed, chunk.chunk
                        ));
                    }
                }
            }
        }
        // Certain calls of a feature the game didn't call.
        for (feature, ours) in &mine {
            if !games.contains_key(feature)
                && ours
                    .iter()
                    .any(|p| !matches!(p.outcome, Outcome::Candidate { .. }))
            {
                report.failures.push(format!(
                    "seed {} chunk {:?} {feature}: certain calls the game doesn't make",
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
    let robust: usize = reports.iter().map(|r| r.robust).sum();
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
        "{} files: {matched} calls match ({robust} whatever the neighbours do), {} not compared, \
         {} differences; {:.2} ms a chunk",
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
