//! Loading every structure set from a real bundle, and computing stronghold rings.
//!
//! Needs a bundle in `bundles/<version>/` for `spyglass_oracle::version`
//! (`SPYGLASS_ORACLE_VERSION`, else the newest release); one test needs `bundles/26.3/`, whose
//! structure sets it lists. Run with `cargo test --release -p spyglass-structure -- --ignored`.

use std::path::PathBuf;
use std::time::Instant;

use spyglass_biome::BiomeSource;
use spyglass_data::bundle::Bundle;
use spyglass_structure::{PlacementKind, Starts, StructureSets};

/// The bundle for the version under test.
fn bundle() -> Bundle {
    let version = spyglass_oracle::version();
    Bundle::open(spyglass_oracle::bundle_dir())
        .unwrap_or_else(|e| panic!("{e}; run spyglass-extract {version}"))
}

/// Expects 26.3's structure sets exactly, so it checks 26.3 whatever the version under test.
#[test]
#[ignore = "needs bundles/26.3"]
fn loads_every_overworld_structure_set() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../bundles/26.3");
    let bundle = Bundle::open(&dir).unwrap_or_else(|e| panic!("{e}; run spyglass-extract 26.3"));
    let source = BiomeSource::overworld(&bundle).unwrap();
    let sets = StructureSets::overworld(&bundle, &source).unwrap_or_else(|e| panic!("{e}"));
    let possible: Vec<&str> = sets
        .sets()
        .iter()
        .filter(|s| s.possible)
        .map(|s| s.id.as_str())
        .collect();
    let impossible: Vec<&str> = sets
        .sets()
        .iter()
        .filter(|s| !s.possible)
        .map(|s| s.id.as_str())
        .collect();
    println!("possible: {possible:?}\nimpossible: {impossible:?}");
    assert_eq!(
        impossible,
        [
            "minecraft:end_cities",
            "minecraft:nether_complexes",
            "minecraft:nether_fossils"
        ]
    );
    assert_eq!(possible.len(), 18);
}

#[test]
#[ignore = "needs bundles/<version> (SPYGLASS_ORACLE_VERSION, else the newest release)"]
fn computes_stronghold_rings() {
    let bundle = bundle();
    let source = BiomeSource::overworld(&bundle).unwrap();
    let sets = StructureSets::overworld(&bundle, &source).unwrap();
    let biomes = source.with_seed(0).unwrap();
    let start = Instant::now();
    let placements = sets.with_seed(0, &biomes);
    println!("rings in {:?}", start.elapsed());
    let strongholds = sets.set_index("strongholds").unwrap();
    assert!(matches!(
        sets.sets()[strongholds].placement.kind,
        PlacementKind::ConcentricRings(_)
    ));
    let rings = placements.ring_positions(strongholds).unwrap();
    assert_eq!(rings.len(), 128);
    for p in &rings[..3] {
        println!("{p:?}");
    }
    for p in rings {
        assert!(placements.is_structure_chunk(strongholds, p.chunk[0], p.chunk[1]));
    }
}

#[test]
#[ignore = "needs bundles/<version> (SPYGLASS_ORACLE_VERSION, else the newest release)"]
fn selects_starts_near_the_origin() {
    let bundle = bundle();
    let source = BiomeSource::overworld(&bundle).unwrap();
    let sets = StructureSets::overworld(&bundle, &source).unwrap();
    let biomes = source.with_seed(0).unwrap();
    let placements = sets.with_seed(0, &biomes);
    let starts = Starts::new(&placements, &biomes);
    let start = Instant::now();
    for (i, set) in sets.sets().iter().enumerate().filter(|(_, s)| s.possible) {
        let found = starts.starts_in(i, [-32, -32], [31, 31]);
        let definite = found.iter().filter(|(_, s)| s.definite().is_some()).count();
        println!(
            "{:32} {:5} starts, {:5} definite",
            set.id,
            found.len(),
            definite
        );
    }
    println!("in {:?}", start.elapsed());
}

#[test]
#[ignore = "needs bundles/<version> (SPYGLASS_ORACLE_VERSION, else the newest release)"]
fn seed_placements_decide_random_spread_from_48_bits() {
    let bundle = bundle();
    let source = BiomeSource::overworld(&bundle).unwrap();
    let sets = StructureSets::overworld(&bundle, &source).unwrap();
    let (min, max) = ([-200, -200], [200, 200]);
    for lower in [0_i64, 1, 0x1234_5678_9abc, 0xffff_ffff_ffff] {
        let seeds = [0_i64, 1, 0x7fff, -1].map(|upper| (upper << 48) | lower);
        let biomes = source.with_seed(seeds[0]).unwrap();
        let placements = sets.with_seed(seeds[0], &biomes);
        for (i, set) in sets.sets().iter().enumerate() {
            let PlacementKind::RandomSpread(_) = set.placement.kind else {
                assert!(
                    sets.seed_placements(seeds[0])
                        .possible_chunks_in(i, min, max)
                        .is_none()
                );
                continue;
            };
            let exact = placements.structure_chunks_in(i, min, max);
            for seed in seeds {
                let possible = sets.seed_placements(seed).possible_chunks_in(i, min, max);
                // In 26.3 every exclusion zone is against a random-spread set, so the lower
                // 48 bits decide every chunk exactly.
                assert_eq!(possible.as_ref(), Some(&exact), "{} seed {seed}", set.id);
            }
        }
    }
}

#[test]
#[ignore = "needs bundles/<version> (SPYGLASS_ORACLE_VERSION, else the newest release)"]
fn ring_chunks_in_a_box_settle_only_what_they_need() {
    let bundle = bundle();
    let source = BiomeSource::overworld(&bundle).unwrap();
    let sets = StructureSets::overworld(&bundle, &source).unwrap();
    let set = sets.set_index("minecraft:strongholds").unwrap();
    let boxes = [
        ([-100, -100], [100, 100]),
        ([-300, -300], [300, 300]),
        ([50, -400], [500, 0]),
        ([-2000, -2000], [2000, 2000]),
    ];
    for seed in [0_i64, 1, -1_556_512_960, 123_456_789] {
        let biomes = source.with_seed(seed).unwrap();
        let full = sets.with_seed(seed, &biomes);
        let all: Vec<[i32; 2]> = full
            .ring_positions(set)
            .unwrap()
            .iter()
            .map(|p| p.chunk)
            .collect();
        for (min, max) in boxes {
            // A fresh session settles only near the box.
            let lazy = sets.with_seed(seed, &biomes);
            let mut expected = Vec::new();
            for c in &all {
                let inside = (0..2).all(|i| (min[i]..=max[i]).contains(&c[i]));
                if inside && !expected.contains(c) && full.is_structure_chunk(set, c[0], c[1]) {
                    expected.push(*c);
                }
            }
            assert_eq!(
                lazy.structure_chunks_in(set, min, max),
                expected,
                "seed {seed} box {min:?} {max:?}"
            );
        }
    }
}
