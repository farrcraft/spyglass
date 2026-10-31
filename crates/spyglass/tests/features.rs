//! Feature queries against the oracle's feature golden files
//! (`docs/specs/oracle.md#feature-golden-files`): in each seed's golden square, every target the
//! game placed is an answer, and every certain answer is one the game placed.
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
    let (mut game, mut certain, mut candidates) = (0, 0, 0);
    let start = std::time::Instant::now();
    let mut chunks = 0;
    // Two seeds keep the test short; every chunk of their 16 × 16 squares.
    for path in files.iter().take(2) {
        let file = FeatureFile::read(path).unwrap();
        let session = world.session(file.seed).unwrap();
        let found = session
            .features_in(
                &query,
                [-8, -8],
                [7, 7],
                spyglass::FeatureOptions::default(),
            )
            .unwrap();
        chunks += 256;
        let placed: HashSet<[i32; 3]> = file
            .chunks
            .iter()
            .filter(|c| (-8..8).contains(&c.chunk[0]) && (-8..8).contains(&c.chunk[1]))
            .flat_map(|c| &c.tries)
            .filter(|t| t.placed && t.feature.starts_with("minecraft:monster_room"))
            .map(|t| t.pos)
            .collect();
        let answers: HashSet<[i32; 3]> = found.iter().map(|f| f.position).collect();
        game += placed.len();
        for p in &placed {
            if !answers.contains(p) {
                failures.push(format!(
                    "seed {}: the game's dungeon at {p:?} isn't an answer",
                    file.seed
                ));
            }
        }
        for f in &found {
            if f.certain {
                certain += 1;
                if !placed.contains(&f.position) {
                    failures.push(format!(
                        "seed {}: a certain dungeon at {:?} the game didn't place",
                        file.seed, f.position
                    ));
                }
            } else {
                candidates += 1;
            }
        }
    }
    println!(
        "{game} dungeons in the game, {certain} certain answers, {candidates} candidates; {:.0} ms a chunk",
        start.elapsed().as_secs_f64() * 1e3 / f64::from(chunks)
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
