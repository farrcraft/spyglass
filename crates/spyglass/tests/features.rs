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
