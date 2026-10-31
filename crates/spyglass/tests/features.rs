//! Feature queries against the oracle's feature golden files
//! (`docs/specs/oracle.md#feature-golden-files`): in each seed's golden square, every target the
//! game placed is an answer, every certain answer is one the game placed, and ruling chunks out
//! from the terrain alone (`docs/specs/features.md#ruling-out`) loses no certain or likely one.
//!
//! Needs `cargo xtask oracle features` and a bundle for the same version. Run with
//! `cargo test --release -p spyglass --test features -- --ignored --nocapture`.

use std::collections::HashSet;

use spyglass::World;
use spyglass_oracle::features::FeatureFile;

#[test]
#[ignore = "needs the oracle's feature golden files and a bundle: cargo xtask oracle features"]
fn features_in_are_the_game_s() {
    let version = spyglass_oracle::version();
    let world = World::open(spyglass_oracle::bundle_dir())
        .unwrap_or_else(|e| panic!("{e}; run spyglass-extract {version}"));
    let files: Vec<_> = spyglass_oracle::features::files()
        .into_iter()
        .filter(|p| p.to_string_lossy().contains("overworld"))
        .collect();
    assert!(
        !files.is_empty(),
        "no feature files; run cargo xtask oracle features"
    );
    let query = world.feature_query("minecraft:monster_room").unwrap();
    let mut failures = Vec::new();
    let (mut game, mut certain, mut likely, mut unlikely) = (0, 0, 0, 0);
    let (mut every_time, mut ruling_time) = (0.0, 0.0);
    let mut chunks = 0;
    // Two seeds keep the test short; every chunk of their 16 × 16 squares.
    for path in files.iter().take(2) {
        let file = FeatureFile::read(path).unwrap();
        let session = world.session(file.seed).unwrap();
        let run = |unlikely| {
            let start = std::time::Instant::now();
            let options = spyglass::FeatureOptions {
                unlikely,
                ..Default::default()
            };
            let found = session
                .features_in(&query, [-8, -8], [7, 7], options)
                .unwrap();
            (found, start.elapsed().as_secs_f64())
        };
        let (every, t) = run(true);
        every_time += t;
        let (ruling, t) = run(false);
        ruling_time += t;
        let kept: Vec<_> = every
            .iter()
            .filter(|f| f.certain || f.likely)
            .cloned()
            .collect();
        if ruling != kept {
            failures.push(format!(
                "seed {}: {} certain or likely answers placing every chunk, {} ruling chunks out",
                file.seed,
                kept.len(),
                ruling.len()
            ));
        }
        chunks += 256;
        let placed: HashSet<[i32; 3]> = file
            .chunks
            .iter()
            .filter(|c| (-8..8).contains(&c.chunk[0]) && (-8..8).contains(&c.chunk[1]))
            .flat_map(|c| &c.tries)
            .filter(|t| t.placed && t.feature.starts_with("minecraft:monster_room"))
            .map(|t| t.pos)
            .collect();
        let answers: HashSet<[i32; 3]> = every.iter().map(|f| f.position).collect();
        game += placed.len();
        for p in &placed {
            if !answers.contains(p) {
                failures.push(format!(
                    "seed {}: the game's dungeon at {p:?} isn't an answer",
                    file.seed
                ));
            }
        }
        for f in &every {
            if f.certain {
                certain += 1;
                if !placed.contains(&f.position) {
                    failures.push(format!(
                        "seed {}: a certain dungeon at {:?} the game didn't place",
                        file.seed, f.position
                    ));
                }
            } else if f.likely {
                likely += 1;
            } else {
                unlikely += 1;
            }
        }
    }
    let per_chunk = |t: f64| t * 1e3 / f64::from(chunks);
    println!(
        "{game} dungeons in the game; {certain} certain answers, {likely} likely and {unlikely} \
         unlikely candidates; {:.0} ms a chunk placing every chunk, {:.1} ms ruling chunks out",
        per_chunk(every_time),
        per_chunk(ruling_time)
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The seeds a search for two dungeons near the spawn reports, in certain mode, have both
/// dungeons in the game's decoration. The spawn is kept within 32 blocks of the origin so every
/// dungeon counted is in the golden square, chunks −8 to 7.
#[test]
#[ignore = "needs a bundle and feature golden files for the seeds it reports: cargo xtask oracle features --seed S"]
fn two_dungeon_seeds_are_the_game_s() {
    let version = spyglass_oracle::version();
    let world = World::open(spyglass_oracle::bundle_dir())
        .unwrap_or_else(|e| panic!("{e}; run spyglass-extract {version}"));
    let query = world
        .search_query(
            "require spawn within 32 of 0,0\n\
             require cluster 2 feature minecraft:monster_room within 16 of each other within 95 of spawn",
        )
        .unwrap();
    let start = std::time::Instant::now();
    let options = spyglass::SearchOptions {
        source: spyglass::SeedSource::Range {
            first: 0,
            last: 127,
        },
        top: 128,
        certain: true,
        ..Default::default()
    };
    let outcome = world
        .search(&query, &options, |_| std::ops::ControlFlow::Continue(()))
        .unwrap();
    println!(
        "{} of 128 seeds in {:.0} s: {:?}",
        outcome.hits.len(),
        start.elapsed().as_secs_f64(),
        outcome.hits.iter().map(|h| h.seed).collect::<Vec<_>>()
    );
    assert!(
        !outcome.hits.is_empty(),
        "no seed in 0..=127 has two dungeons"
    );
    let files = spyglass_oracle::features::files();
    let mut missing = Vec::new();
    let mut failures = Vec::new();
    for hit in &outcome.hits {
        let name = format!("{}.json", hit.seed);
        let Some(path) = files.iter().find(|p| {
            p.to_string_lossy().contains("overworld") && p.file_name().unwrap() == name.as_str()
        }) else {
            missing.push(hit.seed.to_string());
            continue;
        };
        let file = FeatureFile::read(path).unwrap();
        let placed: HashSet<[i32; 3]> = file
            .chunks
            .iter()
            .flat_map(|c| &c.tries)
            .filter(|t| t.placed && t.feature.starts_with("minecraft:monster_room"))
            .map(|t| t.pos)
            .collect();
        let pair = &hit.evaluation.constraints[1].features;
        assert_eq!(pair.len(), 2, "seed {}", hit.seed);
        for f in pair {
            let spyglass::Feature::Placed(f) = f else {
                panic!("seed {}: {f:?}", hit.seed);
            };
            assert!(f.certain, "seed {}: {f:?}", hit.seed);
            if !placed.contains(&f.position) {
                failures.push(format!(
                    "seed {}: the game has no dungeon at {:?}",
                    hit.seed, f.position
                ));
            } else {
                println!(
                    "seed {}: the game's {} at {:?}",
                    hit.seed, f.feature, f.position
                );
            }
        }
    }
    assert!(
        missing.is_empty(),
        "no feature files for seeds {}: run cargo xtask oracle features --seed {}",
        missing.join(", "),
        missing.join(" --seed ")
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
