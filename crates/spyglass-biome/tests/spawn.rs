//! The spawn search against the oracle's spawn golden files (`docs/specs/oracle.md`): the
//! origin chunk, each phase of the search from the game's previous phase, and the fitness at
//! fixed positions.
//!
//! Needs `cargo xtask oracle spawn` (or `golden`) and a bundle for the same version. Run with
//! `cargo test --release -p spyglass-biome --test spawn -- --ignored`.

use serde_json::Value;
use spyglass_biome::{BiomeSource, SpawnPoint, SpawnTargets};
use spyglass_data::bundle::Bundle;

fn point(v: &Value) -> SpawnPoint {
    let p = &v["position"];
    SpawnPoint {
        position: [p[0].as_i64().unwrap() as i32, p[1].as_i64().unwrap() as i32],
        fitness: v["fitness"].as_i64().unwrap(),
    }
}

#[test]
#[ignore = "needs oracle-out/<version>/spawn and bundles/<version>: cargo xtask oracle spawn"]
fn spawn_origin_matches_the_game() {
    let version = spyglass_oracle::version();
    let mut checked = 0;
    let mut flagged = 0;
    let mut failures = Vec::new();
    for dimension in ["overworld", "the_nether", "the_end"] {
        let dir = spyglass_oracle::out_dir().join("spawn").join(dimension);
        let Ok(files) = std::fs::read_dir(&dir) else {
            continue;
        };
        let bundle = Bundle::open(spyglass_oracle::bundle_dir())
            .unwrap_or_else(|e| panic!("{e}; run spyglass-extract {version}"));
        let source = BiomeSource::load(
            &bundle,
            "minecraft:normal",
            &format!("minecraft:{dimension}"),
        )
        .unwrap();
        let targets =
            SpawnTargets::load(&bundle, source.noise_settings()).unwrap_or_else(|e| panic!("{e}"));
        for file in files.flatten() {
            let path = file.path();
            let json: Value =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            let name = format!(
                "{version}/{dimension}/{}",
                path.file_name().unwrap().to_string_lossy()
            );
            let search = targets.with_seed(json["seed"].as_i64().unwrap()).unwrap();
            let origin = search.origin();
            checked += 1;
            flagged += usize::from(origin.platform_dependent);
            let mut fail = |what: String| failures.push(format!("{name}: {what}"));

            let chunk = [
                json["origin"][0].as_i64().unwrap() as i32,
                json["origin"][1].as_i64().unwrap() as i32,
            ];
            if origin.chunk != chunk {
                fail(format!(
                    "origin {:?}, game {chunk:?}{}",
                    origin.chunk,
                    if origin.platform_dependent {
                        " (flagged platform-dependent)"
                    } else {
                        ""
                    }
                ));
            }
            let Some(phases) = json["phases"].as_array() else {
                assert!(origin.found.is_none(), "{name}: the game has no targets");
                continue;
            };
            let phases: Vec<SpawnPoint> = phases.iter().map(point).collect();
            if origin.found != phases.last().copied() {
                fail(format!(
                    "found {:?}, game {:?}",
                    origin.found,
                    phases.last()
                ));
            }
            // Each phase from the game's previous one, so a mismatch names its step.
            if search.fitness_at(0, 0) != phases[0] {
                fail(format!(
                    "start {:?}, game {:?}",
                    search.fitness_at(0, 0),
                    phases[0]
                ));
            }
            for (i, (max_radius, step)) in [(2048.0, 512.0), (512.0, 32.0)].into_iter().enumerate()
            {
                let (ours, flagged) = search.radial_search(phases[i], max_radius, step);
                if ours != phases[i + 1] {
                    fail(format!(
                        "radial search {i}: {ours:?}, game {:?}{}",
                        phases[i + 1],
                        if flagged { " (flagged)" } else { "" }
                    ));
                }
            }
            for sample in json["samples"].as_array().unwrap() {
                let game = point(sample);
                let ours = search.fitness_at(game.position[0], game.position[1]);
                if ours != game {
                    fail(format!("fitness {ours:?}, game {game:?}"));
                }
            }
        }
    }
    assert!(
        checked > 0,
        "no spawn golden files; run cargo xtask oracle spawn"
    );
    println!("{checked} spawn files checked, {flagged} origins flagged platform-dependent");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
