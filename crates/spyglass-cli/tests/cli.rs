//! The `spyglass` binary end to end: each command in both output modes. Tests that need a bundle
//! are ignored; they use the one for `spyglass_oracle::version` (`SPYGLASS_ORACLE_VERSION`, else
//! the newest release in `bundles/`). Run them with `cargo test -p spyglass-cli -- --ignored`.

use std::path::PathBuf;
use std::process::{Command, Output};

use serde_json::Value;

fn bundles() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../bundles")
}

/// The command with `args`, on the default bundle in `bundles/`: the newest release, even when a
/// snapshot there is newer (ADR 0028).
fn spyglass(args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_spyglass"));
    command.args(args).arg("--bundles").arg(bundles());
    command.output().unwrap()
}

/// [`spyglass`] on the bundle for the version under test.
fn game(args: &[&str]) -> Output {
    let version = spyglass_oracle::version();
    let mut args = args.to_vec();
    args.extend(["--game-version", &version]);
    spyglass(&args)
}

fn text(out: &Output) -> String {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout.clone()).unwrap()
}

fn json(args: &[&str]) -> Value {
    let mut args = args.to_vec();
    args.push("--json");
    let value: Value = serde_json::from_str(&text(&game(&args))).unwrap();
    assert_eq!(value["format"], 1);
    assert_eq!(value["dimension"], "minecraft:overworld");
    assert!(value["version"].is_string());
    value
}

#[test]
fn bad_arguments_fail_before_loading() {
    let out = spyglass(&["locate", "spawn", "--seed", "1", "--bundle", "nowhere"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("nowhere"));
    let out = spyglass(&["locate", "biome", "x", "--seed", "1", "--from", "1,2,3,4"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("x,y,z, x,z or player"));
    let out = spyglass(&["locate", "spawn", "--seed", "  "]);
    assert!(!out.status.success());
}

#[test]
#[ignore = "needs bundles/<version> (SPYGLASS_ORACLE_VERSION, else the newest release)"]
fn structures() {
    let v = json(&[
        "locate",
        "structure",
        "#minecraft:village",
        "--seed",
        "0",
        "--from",
        "-100,70,250",
    ]);
    assert_eq!(v["command"], "locate structure");
    assert_eq!(v["seed"], 0);
    assert_eq!(v["from"], serde_json::json!([-100, 70, 250]));
    assert_eq!(v["radius"], 100);
    let hits = v["hits"].as_array().unwrap();
    assert!(!hits.is_empty());
    for hit in hits {
        assert_eq!(hit["set"], "minecraft:villages");
        assert!(hit["position"]["x"].is_i64() && hit["position"]["z"].is_i64());
        assert!(
            hit["structures"][0]
                .as_str()
                .unwrap()
                .starts_with("minecraft:village_")
        );
    }
    // Ancient cities don't depend on terrain: one certain answer.
    let v = json(&["locate", "structure", "ancient_city", "--seed", "spyglass"]);
    assert_eq!(v["seed"], -1_556_512_960_i64);
    assert_eq!(v["complete"], true);
    assert_eq!(v["certain_answer"], 0);
    assert_eq!(v["hits"].as_array().unwrap().len(), 1);

    let out = text(&game(&[
        "locate",
        "structure",
        "minecraft:monument",
        "--seed",
        "1",
        "--unconfirmed",
    ]));
    assert!(out.starts_with("minecraft:monument from 0 64 0 (seed 1, Minecraft "));
    assert!(out.contains("candidate"));
    // Confirmed by default: terrain decides the monument, so one certain answer.
    let v = json(&["locate", "structure", "minecraft:monument", "--seed", "1"]);
    assert_eq!(v["confirmed"], true);
    assert_eq!(v["hits"].as_array().unwrap().len(), 1);
    assert_eq!(v["certain_answer"], 0);
    assert_eq!(v["hits"][0]["bounds"], Value::Null);

    let out = game(&["locate", "structure", "minecraft:nothing", "--seed", "1"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("unknown structure"));
}

#[test]
#[ignore = "needs bundles/<version> (SPYGLASS_ORACLE_VERSION, else the newest release)"]
fn jigsaw_bounds() {
    let args = [
        "locate",
        "structure",
        "#minecraft:village",
        "--seed",
        "spyglass",
        "--bounds",
    ];
    let v = json(&args);
    let hit = &v["hits"][0];
    assert_eq!(hit["certain"], true);
    let bounds = &hit["bounds"];
    assert!(bounds["pieces"].as_u64().unwrap() > 1);
    for d in 0..3 {
        assert!(bounds["min"][d].as_i64().unwrap() <= bounds["max"][d].as_i64().unwrap());
    }
    let out = text(&game(&args));
    assert!(out.contains(" pieces"), "{out}");
    assert!(out.contains("bounds "), "{out}");
}

#[test]
#[ignore = "needs bundles/<version> (SPYGLASS_ORACLE_VERSION, else the newest release)"]
fn biomes() {
    let v = json(&[
        "locate",
        "biome",
        "minecraft:plains",
        "--seed",
        "0",
        "--from",
        "0,0",
    ]);
    assert_eq!(v["from"], serde_json::json!([0, 64, 0]));
    assert_eq!(v["radius"], 6400);
    let hit = v["hits"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(hit["certain"], true);
    assert_eq!(hit["biome"], "minecraft:plains");
    assert!(hit["position"]["y"].is_i64());
    // A biome the Overworld can't have: found nowhere, not an error.
    let v = json(&["locate", "biome", "minecraft:nether_wastes", "--seed", "0"]);
    assert!(v["hits"].as_array().unwrap().is_empty());
    let out = text(&game(&[
        "locate",
        "biome",
        "minecraft:nether_wastes",
        "--seed",
        "0",
    ]));
    assert!(out.contains("none within 6400 blocks"));
}

#[test]
#[ignore = "needs bundles/<version> (SPYGLASS_ORACLE_VERSION, else the newest release)"]
fn slime_and_spawn() {
    let v = json(&["locate", "slime", "--seed", "0", "--radius", "3"]);
    let chunks = v["chunks"].as_array().unwrap();
    assert!(chunks.iter().all(|c| {
        let [x, z] = [c[0].as_i64().unwrap(), c[1].as_i64().unwrap()];
        x.abs() <= 3 && z.abs() <= 3
    }));
    let out = text(&game(&["locate", "slime", "--seed", "0", "--radius", "3"]));
    assert!(out.starts_with("Slime chunks within 3 chunks of chunk 0 0"));

    let v = json(&["locate", "spawn", "--seed", "0"]);
    assert!(v["chunk"][0].is_i64());
    assert_eq!(v["platform_dependent"], false);
    let out = text(&game(&["locate", "spawn", "--seed", "0"]));
    assert!(out.contains("the spawn search starts in chunk"));
}
