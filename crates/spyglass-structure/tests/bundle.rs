//! Loading every structure set from a real bundle, and computing stronghold rings.
//!
//! Needs a bundle in `bundles/26.3/`. Run with
//! `cargo test --release -p spyglass-structure -- --ignored`.

use std::path::PathBuf;
use std::time::Instant;

use spyglass_biome::BiomeSource;
use spyglass_data::bundle::Bundle;
use spyglass_structure::{PlacementKind, Starts, StructureSets};

fn bundle() -> Bundle {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../bundles/26.3");
    Bundle::open(&dir).unwrap_or_else(|e| panic!("{e}; run spyglass-extract 26.3"))
}

#[test]
#[ignore = "needs bundles/26.3"]
fn loads_every_overworld_structure_set() {
    let bundle = bundle();
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
#[ignore = "needs bundles/26.3"]
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
#[ignore = "needs bundles/26.3"]
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
