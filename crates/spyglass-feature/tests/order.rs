//! The feature order on the real bundle: every dimension's lists load and sort, as the game
//! requires, and the targets keep their steps (`docs/specs/features.md#the-order`).
//!
//! Needs a bundle for the oracle's version. Run with
//! `cargo test -p spyglass-feature --test order -- --ignored`.

use spyglass_biome::BiomeSource;
use spyglass_data::bundle::Bundle;
use spyglass_feature::FeatureSettings;

#[test]
#[ignore = "needs a bundle: spyglass-extract"]
fn every_dimension_sorts() {
    let bundle = Bundle::open(spyglass_oracle::bundle_dir())
        .unwrap_or_else(|e| panic!("{e}; run spyglass-extract {}", spyglass_oracle::version()));
    let v1_21 = bundle
        .resource("worldgen/configured_feature", "minecraft:monster_room")
        .is_ok();
    for dimension in [
        "minecraft:overworld",
        "minecraft:the_nether",
        "minecraft:the_end",
    ] {
        let biomes = BiomeSource::load(&bundle, "minecraft:normal", dimension).unwrap();
        let settings = FeatureSettings::load(&bundle, biomes.biome_names(), v1_21).unwrap();
        let order = settings.order();
        let names = |step: usize| -> Vec<String> {
            order[step]
                .iter()
                .map(|&r| settings.placed(r).id.clone().unwrap_or_default())
                .collect()
        };
        println!("{dimension}: {} steps", order.len());
        for (step, list) in order.iter().enumerate().take(5) {
            println!("  step {step}: {} features: {:?}", list.len(), names(step));
        }
        if dimension == "minecraft:overworld" {
            assert!(names(3).contains(&"minecraft:monster_room".to_string()));
            assert!(names(2).contains(&"minecraft:amethyst_geode".to_string()));
        }
    }
}
