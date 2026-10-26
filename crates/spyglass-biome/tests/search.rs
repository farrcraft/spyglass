//! The biome searches on the hand-written fixture world (`spyglass_oracle::fixtures`): plains
//! west of x = 0, desert east of it, and with `tie`, forest tied with plains everywhere plains
//! is. No game data (ADR 0008).

use spyglass_biome::{BiomeId, BiomeSource, HorizontalArea, MAX_REPLAYED_TIES, SearchVolume};
use spyglass_oracle::fixtures;
use spyglass_rng::LegacyRandomSource;

fn source(tie: bool) -> BiomeSource {
    let bundle = fixtures::overworld(&[], tie);
    BiomeSource::overworld(&bundle.open()).unwrap()
}

fn id(source: &BiomeSource, name: &str) -> BiomeId {
    source.parameters().id(name).unwrap()
}

#[test]
fn closest_horizontal_match_is_on_the_right_side() {
    let source = source(false);
    let biomes = source.with_seed(0).unwrap();
    let plains = id(&source, "minecraft:plains");
    let area = HorizontalArea {
        center: [600, 0, 40],
        radius: 1200,
        increment: 8,
        find_closest: true,
    };
    let mut random = LegacyRandomSource::new(1);
    let search = biomes.find_biome_horizontal(area, |b| b == plains, &mut random);
    let found = search.found.unwrap();
    assert!(found.position[0] < 0, "{found:?}");
    // The closest ring's first match: on the ring at the boundary, its z is within that ring.
    let ring = (600 - found.position[0]) >> 2;
    assert!((found.position[2] - 40).abs() >> 2 <= ring);
    assert!(search.alternatives_known && search.alternatives.is_empty());
}

#[test]
fn a_random_match_depends_only_on_the_random_state() {
    let source = source(false);
    let biomes = source.with_seed(0).unwrap();
    let plains = id(&source, "minecraft:plains");
    let area = HorizontalArea {
        center: [-200, 0, 0],
        radius: 64,
        increment: 2,
        find_closest: false,
    };
    let pick = |seed: i64| {
        let mut random = LegacyRandomSource::new(seed);
        biomes
            .find_biome_horizontal(area, |b| b == plains, &mut random)
            .found
            .unwrap()
            .position
    };
    assert_eq!(pick(5), pick(5));
    let picks: std::collections::HashSet<_> = (0..20).map(pick).collect();
    assert!(picks.len() > 1, "reservoir sampling picks different cells");
    // Nothing matches: no answer, and the random state is untouched.
    let desert_only = |b| b == id(&source, "minecraft:desert");
    let mut random = LegacyRandomSource::new(9);
    assert!(
        biomes
            .find_biome_horizontal(area, desert_only, &mut random)
            .found
            .is_none()
    );
}

#[test]
fn tied_cells_are_replayed_up_to_the_limit() {
    let source = source(true);
    let biomes = source.with_seed(0).unwrap();
    let forest = id(&source, "minecraft:forest");
    let search = |radius: i32| {
        let area = HorizontalArea {
            center: [-400, 0, 0],
            radius,
            increment: 1,
            find_closest: false,
        };
        biomes.find_biome_horizontal(area, |b| b == forest, &mut LegacyRandomSource::new(3))
    };
    // 3 × 3 tied cells: every combination is replayed.
    let small = search(4);
    const { assert!(9 <= MAX_REPLAYED_TIES) };
    assert!(small.alternatives_known);
    assert!(!small.alternatives.is_empty());
    // 5 × 5 is too many to replay.
    let large = search(8);
    assert!(!large.alternatives_known);
    assert!(large.alternatives.is_empty());
}

#[test]
fn closest_3d_match_and_uncertain_hits() {
    let volume = |from| SearchVolume {
        from,
        radius: 6400,
        horizontal_step: 32,
        vertical_step: 64,
        min_y: -64,
        max_y: 319,
    };
    let source = source(true);
    let biomes = source.with_seed(0).unwrap();
    let desert = id(&source, "minecraft:desert");
    let search = biomes.find_closest_biome_3d(volume([-300, 64, 0]), |b| b == desert);
    let hit = search.hits.last().unwrap();
    assert!(hit.certain && hit.position[0] >= 0, "{hit:?}");
    // Forest is only ever tied with plains: every hit is uncertain, up to the limit.
    let forest = id(&source, "minecraft:forest");
    let search = biomes.find_closest_biome_3d(volume([-300, 64, 0]), |b| b == forest);
    assert_eq!(search.hits.len(), spyglass_biome::MAX_UNCERTAIN);
    assert!(search.hits.iter().all(|h| !h.certain));
}

#[test]
fn region_fills_agree_with_point_lookups() {
    let source = source(true);
    let biomes = source.with_seed(7).unwrap();
    let region = biomes.biomes_in([-10, -4, 3], [20, 6, 5]);
    for qy in -4..2 {
        for qz in 3..8 {
            for qx in -10..10 {
                let cell = region.get(qx, qy, qz).unwrap();
                assert_eq!(cell, biomes.biome_at(qx, qy, qz), "({qx}, {qy}, {qz})");
            }
        }
    }
}
