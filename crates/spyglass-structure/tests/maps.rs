//! Exploration maps' targets against the game's (`docs/specs/loot.md#maps`): the map cases of
//! the oracle's locate files (`docs/specs/oracle.md#locate-answers`), each a map filled on a
//! real server from a position, in order. A map that skips known structures claims what it
//! finds, so later cases see the claims of earlier ones, as spyglass's do.
//!
//! Needs `cargo xtask oracle locate` and a bundle for the same version. Run with
//! `cargo test --release -p spyglass-structure --test maps -- --ignored --nocapture`.

use serde_json::Value;
use spyglass_biome::BiomeSource;
use spyglass_data::bundle::Bundle;
use spyglass_data::versions::Behavior;
use spyglass_structure::{JigsawPools, Starts, StructureSets};
use spyglass_terrain::{Terrain, TerrainSettings};

/// `WorldOptions.parseSeed` for the seeds the locate files use: a `long`, or a text's hash.
fn seed(text: &str) -> i64 {
    text.parse()
        .unwrap_or_else(|_| i64::from(spyglass_rng::java_string_hash(text)))
}

#[test]
#[ignore = "needs oracle-out/<version>/locate with map cases and bundles/<version>: cargo xtask oracle locate"]
fn map_targets_match_the_game() {
    let dir = spyglass_oracle::out_dir().join("locate");
    let bundle = Bundle::open(spyglass_oracle::bundle_dir()).unwrap();
    let behavior = Behavior::of(bundle.manifest().data_version);
    let source = BiomeSource::load(&bundle, "minecraft:normal", "minecraft:overworld").unwrap();
    let sets =
        StructureSets::load(&bundle, "minecraft:normal", "minecraft:overworld", &source).unwrap();
    let pools = JigsawPools::load(&bundle, &sets).unwrap();
    let settings =
        TerrainSettings::load(&bundle, source.noise_settings(), "minecraft:overworld").unwrap();
    let (mut cases, mut matched, mut unsure, mut stopped) = (0, 0, 0, 0);
    let mut failures = Vec::new();
    let files = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}; run cargo xtask oracle locate", dir.display()));
    for file in files.flatten() {
        let json: Value =
            serde_json::from_str(&std::fs::read_to_string(file.path()).unwrap()).unwrap();
        let Some(maps) = json["maps"].as_array() else {
            continue;
        };
        let seed = seed(json["seed"].as_str().unwrap());
        let biomes = source.with_seed(seed).unwrap();
        let placements = sets.with_seed(seed, &biomes);
        let terrain = Terrain::new(&settings, seed).unwrap();
        let starts = Starts::with_terrain(&placements, &biomes, &terrain).with_pools(&pools);
        let mut claims = Vec::new();
        for (i, case) in maps.iter().enumerate() {
            cases += 1;
            let function = &case["function"];
            // 26.2 writes no destination for treasure maps: its default.
            let destination = function["destination"]
                .as_str()
                .unwrap_or("#minecraft:on_treasure_maps");
            let targets = sets.resolve(&bundle, destination).unwrap();
            let radius = function["search_radius"].as_i64().unwrap_or(50) as i32;
            let skip = function["skip_existing_chunks"].as_bool().unwrap_or(true);
            let from: Vec<i32> = case["from"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_i64().unwrap() as i32)
                .collect();
            let from = [from[0], from[1], from[2]];
            let game = case["target"]
                .as_array()
                .map(|t| [t[0].as_i64().unwrap() as i32, t[1].as_i64().unwrap() as i32]);
            let at = format!(
                "seed {seed} map {i} ({} from {from:?}, {destination})",
                case["table"]
            );
            match starts.map_target(
                &targets,
                from,
                radius,
                skip,
                !behavior.map_claims_nothing,
                &mut claims,
            ) {
                Ok(ours) => {
                    let ours = ours.map(|h| [h.position[0], h.position[2]]);
                    if ours == game {
                        matched += 1;
                    } else {
                        failures.push(format!("{at}: spyglass {ours:?}, the game {game:?}"));
                    }
                }
                Err(search) => {
                    unsure += 1;
                    println!(
                        "  not certain: {at}: {}",
                        match &search {
                            None => "the start the container stands in".to_string(),
                            Some(s) => format!(
                                "{} hits, {} candidates",
                                s.hits.len(),
                                s.hits.iter().filter(|h| !h.certain).count()
                            ),
                        }
                    );
                    // The game's claim, if spyglass can find which start it was; otherwise
                    // what later maps skip isn't known, and the seed's comparison stops.
                    let claimed = search.and_then(|s| {
                        s.hits
                            .iter()
                            .find(|h| Some([h.position[0], h.position[2]]) == game)
                            .map(|h| (h.set, h.chunk))
                    });
                    match (skip, claimed, game) {
                        (false, _, _) | (true, _, None) => {}
                        (true, Some(c), Some(_)) => claims.push(c),
                        (true, None, Some(_)) => {
                            stopped += maps.len() - i - 1;
                            break;
                        }
                    }
                }
            }
        }
    }
    println!(
        "{cases} map cases: {matched} certain and the game's, {unsure} not certain, {stopped} \
         after an unknown claim; {} differences",
        failures.len()
    );
    assert!(cases > 0, "no map cases; run cargo xtask oracle locate");
    assert!(
        failures.is_empty(),
        "{} differences:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
