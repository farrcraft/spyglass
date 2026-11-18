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

#[test]
#[ignore = "needs bundles/<version> (SPYGLASS_ORACLE_VERSION, else the newest release)"]
fn best_heights_agree_with_the_map() {
    let world = World::open_with_packs(
        spyglass_oracle::bundle_dir(),
        Dimension::Overworld,
        &spyglass_oracle::packs(),
    )
    .unwrap();
    // Seed 1's nearest flower forest to the origin, around (-736, -256).
    let session = world.session(1).unwrap();
    let area = spyglass::MapArea::centred_on([-736, -256], [96, 96], 2, 64).unwrap();
    let shares = session
        .flower_heights(&area, "minecraft:cornflower", [40, 140], 20)
        .unwrap();
    assert_eq!(shares.len(), 6);
    assert!(
        shares.iter().any(|s| s.possible > 0),
        "no cornflowers near a flower forest"
    );
    for share in shares {
        let map = session
            .flower_map(&spyglass::MapArea { y: share.y, ..area })
            .unwrap();
        let (mut certain, mut possible) = (0, 0);
        for j in 0..area.size[1] {
            for i in 0..area.size[0] {
                let pixel = map.at([i, j]);
                if pixel.certain() == Some("minecraft:cornflower") {
                    certain += 1;
                }
                if pixel.chance_of("minecraft:cornflower").numerator > 0 {
                    possible += 1;
                }
            }
        }
        assert_eq!(
            (share.certain, share.possible),
            (certain, possible),
            "at y {}",
            share.y
        );
        assert_eq!(share.pixels, 96 * 96);
    }
    assert!(
        session
            .flower_heights(&area, "minecraft:diamond_block", [40, 140], 20)
            .is_err()
    );
}

#[test]
#[ignore = "needs bundles/<version> (SPYGLASS_ORACLE_VERSION, else the newest release)"]
fn flower_fields_are_what_the_map_shows() {
    let world = World::open_with_packs(
        spyglass_oracle::bundle_dir(),
        Dimension::Overworld,
        &spyglass_oracle::packs(),
    )
    .unwrap();
    // Seed 1's nearest flower forest to the origin, around (-736, -256), at a few heights.
    let session = world.session(1).unwrap();
    let query = world
        .search_query(
            "require flower minecraft:orange_tulip field 64 within 200 of -736,-256 at y 60 to 100 step 4\n\
             prefer flower minecraft:dandelion field 1000000 within 200 of -736,-256",
        )
        .unwrap();
    let e = session.evaluate(&query, false).unwrap();
    let field = &e.constraints[0];
    assert_eq!(field.outcome, spyglass::Outcome::Met);
    let Some(spyglass::Feature::FlowerField {
        position,
        flower,
        blocks,
        certain,
    }) = field.features.first()
    else {
        panic!("no field: {:?}", field.features);
    };
    assert_eq!(flower, "minecraft:orange_tulip");
    assert!(*certain && *blocks >= 64);
    // The field's first block grows the flower for certain, as the map says.
    assert_eq!(
        session.flower_at(*position).unwrap().certain(),
        Some("minecraft:orange_tulip")
    );
    let area = spyglass::MapArea {
        min: [position[0], position[2]],
        size: [1, 1],
        scale: 1,
        y: position[1],
    };
    let map = session.flower_map(&area).unwrap();
    assert_eq!(map.at([0, 0]).certain(), Some("minecraft:orange_tulip"));
    // A field larger than the place can't be met, and scores by the share it has.
    let too_big = &e.constraints[1];
    assert!(too_big.best < 1000);
}
