//! `spyglass world` and `locate --world` on saves built in the test (no game files, ADR 0008).
//! They need a bundle for data version 5023 (26.3) in `bundles/`; run with
//! `cargo test -p spyglass-cli --test world -- --ignored`.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;
use spyglass_data::nbt::{self, Compound, Tag};

fn bundles() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../bundles")
}

/// The command with `args`, with `--seed` on the 26.3 bundle in `bundles/` (others there may be
/// newer).
fn spyglass(args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_spyglass"));
    command.args(args).arg("--bundles").arg(bundles());
    if args.contains(&"--seed") {
        command.args(["--game-version", "26.3"]);
    }
    command.output().unwrap()
}

fn json(args: &[&str]) -> Value {
    let mut args = args.to_vec();
    args.push("--json");
    let out = spyglass(&args);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

fn c<const N: usize>(entries: [(&str, Tag); N]) -> Tag {
    Tag::Compound(Compound::from(entries.map(|(k, v)| (k.to_string(), v))))
}

fn s(text: &str) -> Tag {
    Tag::String(text.into())
}

/// A 26.x save with `generator` for the overworld, the given data version and packs, and a
/// single player at `player`.
fn save(dir: &Path, data_version: i32, generator: Tag, packs: &[&str], player: [f64; 3]) {
    let write = |path: PathBuf, tag: &Tag| {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, nbt::write_compressed("", tag)).unwrap();
    };
    write(
        dir.join("level.dat"),
        &c([(
            "Data",
            c([
                ("LevelName", s("Test World")),
                ("DataVersion", Tag::Int(data_version)),
                ("Version", c([("Name", s("26.3"))])),
                (
                    "DataPacks",
                    c([("Enabled", Tag::List(packs.iter().map(|p| s(p)).collect()))]),
                ),
                ("singleplayer_uuid", Tag::IntArray(vec![0, 0, 0, 1])),
            ]),
        )]),
    );
    write(
        dir.join("data/minecraft/world_gen_settings.dat"),
        &c([(
            "data",
            c([
                ("seed", Tag::Long(12345)),
                ("generate_structures", Tag::Byte(1)),
                (
                    "dimensions",
                    c([
                        ("minecraft:overworld", c([("generator", generator)])),
                        (
                            "minecraft:the_nether",
                            c([("generator", nether_generator())]),
                        ),
                        ("minecraft:the_end", c([("generator", end_generator())])),
                    ]),
                ),
            ]),
        )]),
    );
    write(
        dir.join("players/data/00000000-0000-0000-0000-000000000001.dat"),
        &c([
            (
                "Pos",
                Tag::List(player.into_iter().map(Tag::Double).collect()),
            ),
            ("Dimension", s("minecraft:overworld")),
        ]),
    );
}

/// The Default preset's Nether generator.
fn nether_generator() -> Tag {
    c([
        ("type", s("minecraft:noise")),
        ("settings", s("minecraft:nether")),
        (
            "biome_source",
            c([
                ("type", s("minecraft:multi_noise")),
                ("preset", s("minecraft:nether")),
            ]),
        ),
    ])
}

/// The Default preset's End generator.
fn end_generator() -> Tag {
    c([
        ("type", s("minecraft:noise")),
        ("settings", s("minecraft:end")),
        ("biome_source", c([("type", s("minecraft:the_end"))])),
    ])
}

fn default_generator() -> Tag {
    c([
        ("type", s("minecraft:noise")),
        ("settings", s("minecraft:overworld")),
        (
            "biome_source",
            c([
                ("type", s("minecraft:multi_noise")),
                ("preset", s("minecraft:overworld")),
            ]),
        ),
    ])
}

fn kinds(v: &Value) -> Vec<String> {
    v["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w["kind"].as_str().unwrap().to_string())
        .collect()
}

#[test]
#[ignore = "needs a 26.3 bundle: spyglass-extract 26.3"]
fn a_default_world() {
    let dir = tempfile::tempdir().unwrap();
    save(
        dir.path(),
        5023,
        default_generator(),
        &["vanilla"],
        [100.7, 64.0, -200.2],
    );
    let path = dir.path().to_str().unwrap();

    let info = json(&["world", "info", path]);
    assert_eq!(info["command"], "world info");
    assert_eq!(info["seed"], 12345);
    assert_eq!(info["answerable"], true);
    assert!(kinds(&info).is_empty(), "{info}");
    assert_eq!(info["player"]["position"][0], 100.7);

    let spawn = json(&["locate", "spawn", "--world", path]);
    assert_eq!(spawn["seed"], 12345);
    assert_eq!(spawn["world"]["name"], "Test World");
    let from_seed = json(&["locate", "spawn", "--seed", "12345"]);
    assert_eq!(spawn["chunk"], from_seed["chunk"]);

    // The player's block: floor of each coordinate.
    let near = json(&[
        "locate", "slime", "--world", path, "--from", "player", "--radius", "1",
    ]);
    assert_eq!(near["from"], serde_json::json!([100, 64, -201]));

    let text = spyglass(&["locate", "spawn", "--world", path]);
    assert!(String::from_utf8_lossy(&text.stdout).contains("(Test World, seed 12345"));

    // A map of a save is centred on its player: block (100, -201) in pixel (4, 4).
    let png = dir.path().join("map.png");
    let png = png.to_str().unwrap();
    let map = json(&[
        "render", "--world", path, "--size", "8x8", "--scale", "16", "-o", png,
    ]);
    assert_eq!(map["command"], "render");
    assert_eq!(map["world"]["name"], "Test World");
    assert_eq!(map["area"]["min"], serde_json::json!([32, -272]));
}

#[test]
#[ignore = "needs a 26.3 bundle: spyglass-extract 26.3"]
fn a_player_in_the_nether() {
    let dir = tempfile::tempdir().unwrap();
    save(
        dir.path(),
        5023,
        default_generator(),
        &["vanilla"],
        [0.0, 0.0, 0.0],
    );
    std::fs::write(
        dir.path()
            .join("players/data/00000000-0000-0000-0000-000000000001.dat"),
        nbt::write_compressed(
            "",
            &c([
                (
                    "Pos",
                    Tag::List(vec![
                        Tag::Double(-40.5),
                        Tag::Double(70.0),
                        Tag::Double(12.2),
                    ]),
                ),
                ("Dimension", s("minecraft:the_nether")),
            ]),
        ),
    )
    .unwrap();
    let path = dir.path().to_str().unwrap();

    let info = json(&["world", "info", path]);
    assert_eq!(
        info["answerable_dimensions"],
        serde_json::json!([
            "minecraft:overworld",
            "minecraft:the_nether",
            "minecraft:the_end"
        ])
    );
    // From the player: the player's dimension.
    let fortress = json(&[
        "locate",
        "structure",
        "minecraft:fortress",
        "--world",
        path,
        "--from",
        "player",
    ]);
    assert_eq!(fortress["dimension"], "minecraft:the_nether");
    assert_eq!(fortress["from"], serde_json::json!([-41, 70, 12]));
    let from_seed = json(&[
        "locate",
        "structure",
        "minecraft:fortress",
        "--seed",
        "12345",
        "--dimension",
        "nether",
        "--from",
        "-41,70,12",
    ]);
    assert_eq!(fortress["hits"], from_seed["hits"]);
    // Otherwise the Overworld, or what --dimension says.
    let spawn = json(&["locate", "spawn", "--world", path]);
    assert_eq!(spawn["dimension"], "minecraft:overworld");
    let map = json(&[
        "render",
        "--world",
        path,
        "--size",
        "8x8",
        "--scale",
        "16",
        "-o",
        dir.path().join("nether.png").to_str().unwrap(),
    ]);
    assert_eq!(map["dimension"], "minecraft:the_nether");
    assert_eq!(map["area"]["min"], serde_json::json!([-112, -64]));
    for args in [
        &["locate", "slime", "--world", path, "--dimension", "nether"][..],
        &["locate", "spawn", "--world", path, "--dimension", "end"],
        &[
            "locate",
            "biome",
            "plains",
            "--world",
            path,
            "--from",
            "player",
            "--dimension",
            "overworld",
        ],
    ] {
        let out = spyglass(args);
        assert!(!out.status.success(), "{args:?}");
        assert!(out.stdout.is_empty());
    }
}

#[test]
#[ignore = "needs a 26.3 bundle: spyglass-extract 26.3"]
fn worlds_spyglass_cant_predict() {
    // Superflat: refused, and named.
    let flat = tempfile::tempdir().unwrap();
    let generator = c([
        ("type", s("minecraft:flat")),
        (
            "settings",
            c([("biome", s("minecraft:desert")), ("features", Tag::Byte(0))]),
        ),
    ]);
    save(flat.path(), 5023, generator, &["vanilla"], [0.0; 3]);
    let path = flat.path().to_str().unwrap();
    let info = json(&["world", "info", path, "--quick"]);
    assert_eq!(info["answerable"], false);
    assert_eq!(kinds(&info), ["unsupported_generator"]);
    let out = spyglass(&["locate", "spawn", "--world", path]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("Default preset"));

    // No bundle for the data version.
    let old = tempfile::tempdir().unwrap();
    save(
        old.path(),
        4440,
        default_generator(),
        &["vanilla"],
        [0.0; 3],
    );
    let info = json(&["world", "info", old.path().to_str().unwrap(), "--quick"]);
    assert_eq!(kinds(&info), ["no_bundle"]);

    // Packs: one with worldgen files, one without, one missing, one from a mod.
    let packs = tempfile::tempdir().unwrap();
    save(
        packs.path(),
        5023,
        default_generator(),
        &[
            "vanilla",
            "file/terrain",
            "file/functions",
            "file/gone.zip",
            "some_mod",
        ],
        [0.0; 3],
    );
    let datapacks = packs.path().join("datapacks");
    for file in [
        "terrain/data/mine/worldgen/biome/a.json",
        "terrain/data/mine/tags/worldgen/biome/b.json",
        "functions/data/mine/function/tick.mcfunction",
    ] {
        let path = datapacks.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "{}").unwrap();
    }
    let info = json(&["world", "info", packs.path().to_str().unwrap(), "--quick"]);
    assert_eq!(info["answerable"], true);
    let messages: Vec<&str> = info["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w["message"].as_str().unwrap())
        .collect();
    assert_eq!(messages.len(), 3, "{messages:?}");
    assert!(messages[0].contains("file/terrain has 2 worldgen files"));
    assert!(messages[1].contains("file/gone.zip is enabled but missing"));
    assert!(messages[2].contains("some_mod is built in or from a mod"));
}

/// JSON as the game would store it in NBT: integers as ints, other numbers as doubles,
/// booleans as bytes.
fn to_nbt(v: &Value) -> Tag {
    match v {
        Value::Bool(b) => Tag::Byte(i8::from(*b)),
        Value::Number(n) => match n.as_i64() {
            Some(i) => Tag::Int(i as i32),
            None => Tag::Double(n.as_f64().unwrap()),
        },
        Value::String(s) => Tag::String(s.clone()),
        Value::Array(items) => Tag::List(items.iter().map(to_nbt).collect()),
        Value::Object(map) => {
            Tag::Compound(map.iter().map(|(k, v)| (k.clone(), to_nbt(v))).collect())
        }
        Value::Null => unreachable!(),
    }
}

#[test]
#[ignore = "needs a 26.3 bundle: spyglass-extract 26.3"]
fn a_preset_world_is_named() {
    let preset: Value = serde_json::from_slice(
        &std::fs::read(bundles().join("26.3/data/minecraft/worldgen/world_preset/flat.json"))
            .unwrap(),
    )
    .unwrap();
    let generator = to_nbt(&preset["dimensions"]["minecraft:overworld"]["generator"]);
    let dir = tempfile::tempdir().unwrap();
    save(dir.path(), 5023, generator, &["vanilla"], [0.0; 3]);
    let info = json(&["world", "info", dir.path().to_str().unwrap(), "--quick"]);
    let message = info["warnings"][0]["message"].as_str().unwrap();
    assert!(message.contains("the minecraft:flat preset"), "{message}");
}
