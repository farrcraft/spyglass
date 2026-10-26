//! Small hand-written bundles for tests that need no game data (ADR 0008).
//!
//! [`overworld`] is a whole dimension the public API can open: the Default preset, a parameter
//! list of plains and desert, and a noise router whose temperature is a gradient along x, so the
//! biome is plains west of x = 0 and desert east of it, the same for every seed. The gradient is
//! never exactly 0 at a block, so no cell is tied between the two. [`structured_overworld`] adds
//! structure sets.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use spyglass_data::bundle::{Bundle, FORMAT, MANIFEST, Manifest, sha1_hex};
use tempfile::TempDir;

/// A bundle in its own temporary bundles folder, deleted when dropped: the bundle is
/// `<folder>/test`, and nothing else is in the folder.
pub struct TestBundle {
    root: TempDir,
}

impl TestBundle {
    /// A bundle of `files`, `(path in the bundle, JSON)`, with a manifest listing them.
    pub fn new(files: &[(String, Value)]) -> Self {
        let root = tempfile::tempdir().expect("a temporary folder");
        let dir = root.path().join("test");
        let mut listed = BTreeMap::new();
        for (path, value) in files {
            let file = dir.join(path);
            fs::create_dir_all(file.parent().expect("a parent")).expect("create folders");
            let text = value.to_string();
            fs::write(&file, &text).expect("write a bundle file");
            listed.insert(path.clone(), sha1_hex(text.as_bytes()).expect("hash"));
        }
        let manifest = Manifest {
            format: FORMAT,
            version: "test".into(),
            data_version: 1,
            jar_sha1: String::new(),
            java_version: String::new(),
            extractor: String::new(),
            files: listed,
        };
        let text = serde_json::to_string_pretty(&manifest).expect("a manifest serializes");
        fs::write(dir.join(MANIFEST), text).expect("write the manifest");
        TestBundle { root }
    }

    /// The bundle's folder.
    pub fn path(&self) -> PathBuf {
        self.root.path().join("test")
    }

    /// The bundles folder holding it and nothing else.
    pub fn bundles(&self) -> &Path {
        self.root.path()
    }

    /// The bundle, opened.
    pub fn open(&self) -> Bundle {
        Bundle::open(self.path()).expect("a valid test bundle")
    }
}

/// `data/minecraft/<registry>/<path>.json`.
pub fn resource(registry: &str, path: &str, value: Value) -> (String, Value) {
    (format!("data/minecraft/{registry}/{path}.json"), value)
}

/// A biome parameter point: every parameter `0` except `temperature`.
fn point(biome: &str, temperature: f64) -> Value {
    json!({"biome": biome, "parameters": {
        "temperature": temperature, "humidity": 0.0, "continentalness": 0.0,
        "erosion": 0.0, "depth": 0.0, "weirdness": 0.0, "offset": 0.0}})
}

/// The fixture Overworld, plus `extra` files (structure sets, structures, tags). With `tie`, a
/// third biome, `minecraft:forest`, has exactly plains' parameters, so every plains cell is a
/// tie between the two.
pub fn overworld(extra: &[(String, Value)], tie: bool) -> TestBundle {
    let mut points = vec![
        point("minecraft:plains", -0.5),
        point("minecraft:desert", 0.5),
    ];
    if tie {
        points.push(point("minecraft:forest", -0.5));
    }
    let constant = |v: f64| json!({"type": "minecraft:constant", "value": v});
    let mut files = vec![
        resource(
            "worldgen/world_preset",
            "normal",
            json!({"dimensions": {"minecraft:overworld": {"type": "minecraft:overworld",
                "generator": {"type": "minecraft:noise", "settings": "minecraft:overworld",
                    "biome_source": {"type": "minecraft:multi_noise",
                        "preset": "minecraft:overworld"}}}}}),
        ),
        resource(
            "worldgen/multi_noise_biome_source_parameter_list",
            "overworld",
            json!({"preset": "minecraft:overworld"}),
        ),
        (
            "reports/biome_parameters/minecraft/overworld.json".to_string(),
            json!({ "biomes": points }),
        ),
        resource(
            "dimension_type",
            "overworld",
            json!({"min_y": -64, "height": 384}),
        ),
        resource("worldgen/density_function", "zero", json!(0.0)),
        resource(
            "tags/block",
            "blocks_motion_in_heightmap",
            json!({"values": ["minecraft:stone"]}),
        ),
        resource(
            "worldgen/noise_settings",
            "overworld",
            json!({
                "sea_level": 63,
                "noise": {"min_y": -64, "height": 384},
                "default_block": "minecraft:stone",
                "default_fluid": "minecraft:water",
                "noise_router": {
                    // A flat seabed: stone below y 50, then water to the sea level.
                    "final_density": {"type": "minecraft:gradient", "axis": "y",
                        "from_coordinate": 0, "from_value": 1.0,
                        "to_coordinate": 100, "to_value": -1.0},
                    "temperature": {"type": "minecraft:gradient", "axis": "x",
                        "from_coordinate": -1000, "from_value": -1.0,
                        "to_coordinate": 1001, "to_value": 1.0},
                    "vegetation": constant(0.0),
                    "continents": constant(0.0),
                    "erosion": "minecraft:zero",
                    "depth": 0.0,
                    "ridges": 0.0
                },
                "spawn_target": [{"minecraft:zero": [-1.0, 1.0]}]
            }),
        ),
    ];
    for biome in ["plains", "desert", "forest"] {
        files.push(resource("worldgen/biome", biome, json!({})));
    }
    files.extend(extra.iter().cloned());
    TestBundle::new(&files)
}

/// A random-spread structure set of one structure, with salt 12345.
pub fn random_spread_set(
    name: &str,
    spacing: i32,
    separation: i32,
    structure: &str,
) -> (String, Value) {
    resource(
        "worldgen/structure_set",
        name,
        json!({"placement": {"type": "minecraft:random_spread", "salt": 12345,
                "spacing": spacing, "separation": separation},
            "structures": [{"structure": structure, "weight": 1}]}),
    )
}

/// A structure of type `kind` (e.g. `minecraft:swamp_hut`) whose biome check is `biomes`.
pub fn structure(name: &str, kind: &str, biomes: &str) -> (String, Value) {
    resource(
        "worldgen/structure",
        name,
        json!({"type": kind, "biomes": biomes}),
    )
}

/// [`overworld`] with structures chosen to exercise each kind of answer: towers check their
/// biome at a point, so they're certain in plains; huts need terrain, so they're only ever
/// candidates; monuments check an area; keeps are on concentric rings. Each random-spread set
/// has spacing 8 and separation 2. Tags: `#minecraft:everything` (towers, and
/// `#minecraft:towers_too`) and `#minecraft:towers_too` (towers and huts). With `tie`, plains
/// cells are tied with forest, as in [`overworld`].
pub fn structured_overworld(tie: bool) -> TestBundle {
    overworld(
        &[
            random_spread_set("towers", 8, 2, "minecraft:tower"),
            structure("tower", "minecraft:stronghold", "minecraft:plains"),
            random_spread_set("huts", 8, 2, "minecraft:hut"),
            structure("hut", "minecraft:swamp_hut", "minecraft:plains"),
            random_spread_set("monuments", 8, 2, "minecraft:monument"),
            structure("monument", "minecraft:ocean_monument", "minecraft:plains"),
            resource(
                "worldgen/structure_set",
                "rings",
                json!({"placement": {"type": "minecraft:concentric_rings", "salt": 0,
                        "distance": 4, "spread": 3, "count": 6,
                        "preferred_biomes": "minecraft:plains"},
                    "structures": [{"structure": "minecraft:keep", "weight": 1}]}),
            ),
            structure("keep", "minecraft:stronghold", "minecraft:plains"),
            resource(
                "tags/worldgen/structure",
                "everything",
                json!({"values": ["minecraft:tower", "#minecraft:towers_too"]}),
            ),
            resource(
                "tags/worldgen/structure",
                "towers_too",
                json!({"values": ["minecraft:tower", "minecraft:hut"]}),
            ),
        ],
        tie,
    )
}
