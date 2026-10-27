//! `minecraft:dimension_origin` placement on the fixture Overworld: the only placement chunk is
//! the chunk the spawn search ends in, and `/locate` never finds it, as
//! `ChunkGenerator.findNearestMapStructure` handles only rings and random spread.

use serde_json::json;
use spyglass_biome::{BiomeSource, SpawnTargets};
use spyglass_data::anchor::AnchorHeights;
use spyglass_oracle::fixtures::{self, resource, structure};
use spyglass_structure::{LOCATE_RADIUS, PlacementKind, Starts, StructureSets};

#[test]
fn only_the_spawn_origins_chunk_is_placed_and_locate_never_finds_it() {
    let fixture = fixtures::overworld(
        &[
            resource(
                "worldgen/structure_set",
                "origin_towers",
                // MapCodec.unit: other fields are ignored, not required.
                json!({"placement": {"type": "minecraft:dimension_origin", "spacing": "ignored"},
                    "structures": [{"structure": "minecraft:tower", "weight": 1}]}),
            ),
            structure("tower", "minecraft:stronghold", "minecraft:plains"),
        ],
        false,
    );
    let bundle = fixture.open();
    let source = BiomeSource::overworld(&bundle).unwrap();
    let sets = StructureSets::overworld(&bundle, &source).unwrap();
    let set = sets.set_index("minecraft:origin_towers").unwrap();
    assert!(matches!(
        sets.sets()[set].placement.kind,
        PlacementKind::DimensionOrigin
    ));
    let tower = sets.structure_index("minecraft:tower").unwrap();
    // The fixture's dimension type: min y -64, height 384.
    let targets = SpawnTargets::load(&bundle, source.noise_settings())
        .unwrap()
        .within_level(AnchorHeights {
            min_y: -64,
            height: 384,
        });
    for seed in [0, 1, -7, 123_456_789] {
        let origin = targets.with_seed(seed).unwrap().origin().chunk;
        let biomes = source.with_seed(seed).unwrap();
        let placements = sets.with_seed(seed, &biomes);
        assert!(placements.is_structure_chunk(set, origin[0], origin[1]));
        assert!(!placements.is_structure_chunk(set, origin[0] + 1, origin[1]));
        assert!(!placements.is_structure_chunk(set, origin[0], origin[1] - 1));
        let around = |d: i32| [origin[0] - d, origin[1] - d];
        let far = |d: i32| [origin[0] + d, origin[1] + d];
        assert_eq!(
            placements.structure_chunks_in(set, around(5), far(5)),
            [origin]
        );
        assert!(
            placements
                .structure_chunks_in(set, far(1), far(9))
                .is_empty()
        );
        let starts = Starts::new(&placements, &biomes);
        let found = starts.locate(&[tower], [0, 64, 0], LOCATE_RADIUS);
        assert!(found.hits.is_empty() && found.certain_answer.is_none());
    }
}
