//! Opening each of the Default preset's dimensions through the public API (ADR 0023). The
//! answers themselves are checked against the game by the lower crates' golden tests.
//!
//! Needs a bundle: run with `cargo test --release -p spyglass --test dimensions -- --ignored`.

use spyglass::{Dimension, World};

fn bundle() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../bundles/26.3")
}

#[test]
fn dimensions_have_ids() {
    for d in Dimension::ALL {
        assert_eq!(Dimension::from_id(d.id()), Some(d));
    }
    assert_eq!(Dimension::from_id("the_nether"), Some(Dimension::Nether));
    assert_eq!(Dimension::from_id("minecraft:nether"), None);
}

#[test]
#[ignore = "needs a bundle: spyglass-extract 26.3"]
fn each_dimension_opens() {
    let nether = World::open_dimension(bundle(), Dimension::Nether).unwrap();
    assert_eq!(nether.dimension(), "minecraft:the_nether");
    assert_eq!(nether.coordinate_scale(), 8.0);
    assert_eq!(nether.biomes().len(), 5);
    assert!(nether.structures().any(|s| s == "minecraft:fortress"));
    let end = World::open_dimension(bundle(), Dimension::End).unwrap();
    assert_eq!(end.dimension_kind(), Dimension::End);
    assert_eq!(end.coordinate_scale(), 1.0);
    assert_eq!(
        end.biomes(),
        [
            "minecraft:the_end",
            "minecraft:end_highlands",
            "minecraft:end_midlands",
            "minecraft:small_end_islands",
            "minecraft:end_barrens",
        ]
    );
    // The main island is the_end for every seed; the Nether has no spawn targets, so its spawn
    // origin is chunk 0, 0.
    let session = end.session(12_345).unwrap();
    assert_eq!(session.biome_at([100, 64, -200]).biome, "minecraft:the_end");
    let origin = nether.session(1).unwrap().spawn_origin();
    assert_eq!(origin.chunk, [0, 0]);
    assert_eq!(origin.position, None);
    // Terrain binds with legacy randomness: the Nether's surface height is its solid ceiling,
    // one above the noise range's top block (127); the End's void has no blocks.
    let height = |w: &World, at| {
        w.session(0)
            .unwrap()
            .height_at(at, spyglass::Heightmap::WorldSurfaceWg)
    };
    assert_eq!(height(&nether, [0, 0]), 128);
    assert_eq!(height(&end, [5000, 5000]), 0);
    let overworld = World::open(bundle()).unwrap();
    assert_eq!(overworld.dimension_kind(), Dimension::Overworld);
    assert_eq!(overworld.coordinate_scale(), 1.0);
}
