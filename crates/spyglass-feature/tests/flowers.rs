//! Bone meal's flowers from a bundle (`docs/specs/flowers.md`): each biome's features, and
//! each provider's odds against the states it draws.

use spyglass_biome::{BiomeId, BiomeSource};
use spyglass_feature::FeatureSettings;
use spyglass_feature::flowers::{Chance, Flowers};
use spyglass_rng::LegacyRandomSource;

fn load() -> (Flowers, BiomeSource) {
    let bundle =
        spyglass_oracle::open_bundle().unwrap_or_else(|e| panic!("{e}; run spyglass-extract"));
    let v1_21 = bundle
        .resource("worldgen/configured_feature", "minecraft:monster_room")
        .is_ok();
    let biomes = BiomeSource::load(&bundle, "minecraft:normal", "minecraft:overworld").unwrap();
    let settings = FeatureSettings::load(&bundle, biomes.biome_names(), v1_21).unwrap();
    (Flowers::load(&bundle, &settings).unwrap(), biomes)
}

fn biome(biomes: &BiomeSource, name: &str) -> BiomeId {
    let index = biomes
        .biome_names()
        .names()
        .iter()
        .position(|n| n == name)
        .unwrap_or_else(|| panic!("no biome {name}"));
    BiomeId(u16::try_from(index).unwrap())
}

fn feature_ids(flowers: &Flowers, biome: BiomeId) -> Vec<&str> {
    flowers
        .biome_features(biome)
        .iter()
        .map(|&f| flowers.features()[f].id.as_str())
        .collect()
}

#[test]
#[ignore = "needs a bundle: spyglass-extract"]
fn biomes_have_their_bone_meal_features() {
    let (flowers, biomes) = load();
    let ids = |name| feature_ids(&flowers, biome(&biomes, name));
    assert_eq!(
        ids("minecraft:flower_forest"),
        ["minecraft:flower_flower_forest"]
    );
    assert_eq!(ids("minecraft:plains"), ["minecraft:flower_plain"]);
    assert_eq!(ids("minecraft:deep_dark"), ["minecraft:flower_plain"]);
    assert_eq!(ids("minecraft:swamp"), ["minecraft:flower_swamp"]);
    assert_eq!(ids("minecraft:meadow").len(), 2);
    assert!(ids("minecraft:lush_caves").is_empty());
    for feature in flowers.features() {
        assert!(
            feature.error().is_none(),
            "{}: {:?}",
            feature.id,
            feature.error()
        );
    }
}

#[test]
#[ignore = "needs a bundle: spyglass-extract"]
fn odds_sum_to_one_everywhere() {
    let (flowers, biomes) = load();
    for (i, name) in biomes.biome_names().names().iter().enumerate() {
        let id = BiomeId(u16::try_from(i).unwrap());
        for pos in [
            [0, 64, 0],
            [1000, -40, -3000],
            [-123_456, 200, 98_765],
            [7, 70, 13],
        ] {
            let odds = flowers.odds(id, pos).unwrap();
            if flowers.biome_features(id).is_empty() {
                assert!(odds.is_empty(), "{name}");
                continue;
            }
            let total = odds
                .iter()
                .fold(Chance::new_zero(), |a, o| a.plus(o.chance));
            assert!(total.is_certain(), "{name} at {pos:?}: {total:?}");
        }
    }
}

#[test]
#[ignore = "needs a bundle: spyglass-extract"]
fn odds_match_the_states_drawn() {
    let (flowers, _) = load();
    const DRAWS: u32 = 20_000;
    for (f, feature) in flowers.features().iter().enumerate() {
        for pos in [[0, 64, 0], [517, 90, -2048], [-3000, 12, 4000]] {
            let mut counts = std::collections::BTreeMap::new();
            let mut random = LegacyRandomSource::new(i64::from(pos[0]) ^ 0x5eed);
            for _ in 0..DRAWS {
                *counts
                    .entry(flowers.draw(f, pos, &mut random).unwrap())
                    .or_insert(0u32) += 1;
            }
            let odds = flowers.provider_odds(f, pos).unwrap();
            for o in &odds {
                let p = o.chance.as_f64();
                let seen = f64::from(counts.remove(&o.state).unwrap_or(0)) / f64::from(DRAWS);
                // Five standard deviations of the share.
                let allowed = 5.0 * (p * (1.0 - p) / f64::from(DRAWS)).sqrt() + 1e-9;
                assert!(
                    (seen - p).abs() <= allowed,
                    "{} at {pos:?}: {} drawn {seen}, odds {p}",
                    feature.id,
                    flowers.state_name(o.state)
                );
            }
            assert!(
                counts.is_empty(),
                "{} drew states its odds don't have",
                feature.id
            );
        }
    }
}
