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

#[test]
#[ignore = "needs a bundle: spyglass-extract 26.3"]
fn a_game_spans_the_dimensions() {
    let game = spyglass::Game::open(bundle()).unwrap();
    assert_eq!(game.version(), "26.3");
    let (o, n) = (Dimension::Overworld, Dimension::Nether);
    assert_eq!(game.convert([-1001, 800], o, n).unwrap(), [-126, 100]);
    assert!(game.convert([0, 0], o, Dimension::End).is_err());
    let session = game.session(spyglass::parse_seed("spyglass").unwrap());
    for d in Dimension::ALL {
        assert_eq!(session.dimension(d).unwrap().world().dimension_kind(), d);
    }
    // Every dimension draws biomes and structures; the Nether has no terrain layers, and only
    // the Overworld has slime chunks and the spawn.
    let nether = game.world(n).unwrap();
    let area = spyglass::MapArea::centred_on([0, 0], [64, 64], 16, 64).unwrap();
    let layers = spyglass::Layers {
        structures: vec![nether.structure_query("minecraft:fortress").unwrap()],
        ..spyglass::Layers::default()
    };
    assert!(session.dimension(n).unwrap().render(&area, &layers).is_ok());
    let relief = spyglass::Layers {
        relief: true,
        ..spyglass::Layers::default()
    };
    assert!(relief.check(nether).is_err());
    assert!(relief.check(game.world(Dimension::End).unwrap()).is_ok());
    assert!(relief.check(game.world(o).unwrap()).is_ok());
}
