//! Bone meal's flowers from a bundle (`docs/specs/flowers.md`): each biome's features, and
//! each provider's odds against the states it draws.

use std::collections::HashMap;
use std::fs;

use serde_json::Value;
use spyglass_biome::{BiomeId, BiomeSource};
use spyglass_blocks::BlockSettings;
use spyglass_feature::FeatureSettings;
use spyglass_feature::flowers::{Chance, Flowers};
use spyglass_rng::{LegacyRandomSource, RandomSource};

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

/// The oracle's flower files: `providers.json` and one file per seed
/// (`docs/specs/oracle.md#flower-files`).
fn flower_files() -> Option<(Value, Vec<Value>)> {
    let dir = spyglass_oracle::out_dir().join("flowers");
    let providers = fs::read_to_string(dir.join("providers.json")).ok()?;
    let mut seeds = Vec::new();
    for entry in fs::read_dir(&dir).ok()?.flatten() {
        let path = entry.path();
        if path.file_name().is_some_and(|n| n != "providers.json") {
            seeds.push(serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap());
        }
    }
    Some((serde_json::from_str(&providers).unwrap(), seeds))
}

fn ints(v: &Value) -> Vec<i32> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|n| i32::try_from(n.as_i64().unwrap()).unwrap())
        .collect()
}

#[test]
#[ignore = "needs the oracle's flower files and a bundle: cargo xtask oracle flowers"]
fn flower_providers_match_the_game() {
    let (providers, seeds) =
        flower_files().expect("no flower files; run cargo xtask oracle flowers");
    let (flowers, biomes) = load();
    let mut failures = Vec::new();

    // Each provider, draw for draw: the state, then the source's next draw.
    let random_seeds: Vec<i64> = providers["random_seeds"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_i64().unwrap())
        .collect();
    let mut cases = 0;
    for feature in providers["features"].as_array().unwrap() {
        let id = feature["feature"].as_str().unwrap();
        let Some(f) = flowers.features().iter().position(|x| x.id == id) else {
            failures.push(format!("{id}: no such bone meal feature"));
            continue;
        };
        for case in feature["cases"].as_array().unwrap() {
            let pos = ints(&case["pos"]);
            let pos = [pos[0], pos[1], pos[2]];
            for (i, &seed) in random_seeds.iter().enumerate() {
                cases += 1;
                let mut random = LegacyRandomSource::new(seed);
                let state = flowers.draw(f, pos, &mut random).unwrap();
                let want = case["states"][i].as_str().unwrap();
                let next = random.next_int();
                let want_next = case["next"][i].as_i64().unwrap();
                if flowers.state_name(state) != want || i64::from(next) != want_next {
                    failures.push(format!(
                        "{id} at {pos:?} with random {seed}: {} then {next}, the game {want} then {want_next}",
                        flowers.state_name(state)
                    ));
                }
            }
        }
    }

    // Each patch's block biomes (BiomeManager's zoom) and their bone meal features.
    let bundle = spyglass_oracle::open_bundle().unwrap();
    let settings = BlockSettings::load(&bundle, "minecraft:normal", "minecraft:overworld").unwrap();
    let names: HashMap<&str, BiomeId> = biomes
        .biome_names()
        .names()
        .iter()
        .enumerate()
        .map(|(i, n)| (n.as_str(), BiomeId(u16::try_from(i).unwrap())))
        .collect();
    let mut blocks = 0;
    for file in &seeds {
        let seed = file["seed"].as_i64().unwrap();
        let generator = settings.generator(seed).unwrap();
        let palette: Vec<&str> = file["biome_palette"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| b.as_str().unwrap())
            .collect();
        for (name, features) in file["bone_meal_features"].as_object().unwrap() {
            let want: Vec<&str> = features
                .as_array()
                .unwrap()
                .iter()
                .map(|f| f.as_str().unwrap())
                .collect();
            let got: Vec<&str> = flowers
                .biome_features(names[name.as_str()])
                .iter()
                .map(|&f| flowers.features()[f].id.as_str())
                .collect();
            if got != want {
                failures.push(format!(
                    "{name}: bone meal features {got:?}, the game {want:?}"
                ));
            }
        }
        for patch in file["patches"].as_array().unwrap() {
            let chunk = ints(&patch["chunk"]);
            let y = i32::try_from(patch["y"].as_i64().unwrap()).unwrap();
            for (i, cell) in patch["biomes"].as_array().unwrap().iter().enumerate() {
                blocks += 1;
                let i = i32::try_from(i).unwrap();
                let pos = [chunk[0] * 16 + i % 16, y, chunk[1] * 16 + i / 16];
                let want = palette[usize::try_from(cell.as_u64().unwrap()).unwrap()];
                let lookup = generator.block_biome(pos);
                if !lookup.candidates().contains(&names[want]) {
                    failures.push(format!(
                        "seed {seed} at {pos:?}: biome {}, the game {want}",
                        biomes.biome_names().name(lookup.biome())
                    ));
                }
            }
        }
    }
    println!("{cases} provider draws and {blocks} block biomes checked");
    assert!(
        failures.is_empty(),
        "{} failures, first: {:#?}",
        failures.len(),
        &failures[..failures.len().min(20)]
    );
}
