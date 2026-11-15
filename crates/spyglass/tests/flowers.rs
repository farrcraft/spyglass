//! `Session::biome_at_block` and `Session::flower_at` against the oracle's flower files
//! (`docs/specs/flowers.md`, `docs/specs/oracle.md#flower-files`).

use std::fs;

use serde_json::Value;
use spyglass::{Chance, Dimension, World};

#[test]
#[ignore = "needs oracle-out/<version>/flowers and a bundle: cargo xtask oracle flowers"]
fn flowers_through_the_api_are_the_game_s() {
    let dir = spyglass_oracle::out_dir().join("flowers");
    let world = World::open_with_packs(
        spyglass_oracle::bundle_dir(),
        Dimension::Overworld,
        &spyglass_oracle::packs(),
    )
    .unwrap();
    let mut files = 0;
    let mut failures = Vec::new();
    for entry in fs::read_dir(&dir).expect("no flower files; run cargo xtask oracle flowers") {
        let path = entry.unwrap().path();
        if path.file_name().is_some_and(|n| n == "providers.json") {
            continue;
        }
        files += 1;
        let file: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        let session = world.session(file["seed"].as_i64().unwrap()).unwrap();
        let palette = file["biome_palette"].as_array().unwrap();
        for patch in file["patches"].as_array().unwrap() {
            let chunk = patch["chunk"].as_array().unwrap();
            let (cx, cz) = (
                chunk[0].as_i64().unwrap() as i32,
                chunk[1].as_i64().unwrap() as i32,
            );
            let y = patch["y"].as_i64().unwrap() as i32;
            for (i, cell) in patch["biomes"].as_array().unwrap().iter().enumerate() {
                let i = i as i32;
                let pos = [cx * 16 + i % 16, y, cz * 16 + i / 16];
                let want = palette[cell.as_u64().unwrap() as usize].as_str().unwrap();
                let biome = session.biome_at_block(pos);
                let candidates: Vec<&str> = if biome.tied.is_empty() {
                    vec![biome.biome.as_str()]
                } else {
                    biome.tied.iter().map(String::as_str).collect()
                };
                if !candidates.contains(&want) {
                    failures.push(format!(
                        "{} at {pos:?}: {candidates:?}, the game {want}",
                        path.display()
                    ));
                }
                // The flower answer follows the same biome, and its chances are whole.
                if i % 37 == 0 {
                    let at = session.flower_at(pos).unwrap();
                    assert_eq!(at.first().biome, biome.biome);
                    for b in &at.biomes {
                        if b.features.is_empty() {
                            assert!(b.flowers.is_empty());
                            continue;
                        }
                        let total = b
                            .flowers
                            .iter()
                            .fold(Chance::new_zero(), |a, f| a.plus(f.chance));
                        assert!(total.is_certain(), "{} at {pos:?}: {total:?}", b.biome);
                    }
                }
            }
        }
    }
    assert!(files > 0, "no seed flower files");
    assert!(
        failures.is_empty(),
        "{} failures, first: {:#?}",
        failures.len(),
        &failures[..failures.len().min(20)]
    );
}
