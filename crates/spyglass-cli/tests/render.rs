//! `spyglass render` on the hand-written fixture world (`spyglass_oracle::fixtures`): no game
//! data (ADR 0008), so these always run. The command must write exactly the bytes the library
//! does for the same request (`docs/specs/render.md#testing`).

use std::path::Path;
use std::process::{Command, Output};

use serde_json::{Value, json};
use spyglass::{Layers, MapArea, World};
use spyglass_oracle::fixtures::{self, TestBundle, resource};

fn bundle() -> TestBundle {
    fixtures::overworld(
        &[
            resource(
                "worldgen/structure_set",
                "towers",
                json!({"placement": {"type": "minecraft:random_spread", "salt": 1,
                        "spacing": 3, "separation": 1},
                    "structures": [{"structure": "minecraft:tower", "weight": 1}]}),
            ),
            resource(
                "worldgen/structure",
                "tower",
                json!({"type": "minecraft:stronghold", "biomes": "minecraft:plains"}),
            ),
        ],
        true,
    )
}

fn spyglass(bundle: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_spyglass"))
        .arg("render")
        .args(args)
        .arg("--bundle")
        .arg(bundle)
        .output()
        .unwrap()
}

fn ok(out: &Output) -> &[u8] {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    &out.stdout
}

#[test]
fn the_command_writes_the_same_image_as_the_library() {
    let fixture = bundle();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("map.png");
    let out = spyglass(
        &fixture.path(),
        &[
            "--seed",
            "42",
            "--centre",
            "-40,8",
            "--size",
            "120x90",
            "--scale",
            "8",
            "--y",
            "70",
            "--structures",
            "minecraft:tower",
            "--highlight",
            "minecraft:forest",
            "--slime",
            "--spawn",
            "--ties",
            "-o",
            file.to_str().unwrap(),
            "--json",
        ],
    );
    let answer: Value = serde_json::from_slice(ok(&out)).unwrap();

    let world = World::open(fixture.path()).unwrap();
    let session = world.session(42).unwrap();
    let area = MapArea::centred_on([-40, 8], [120, 90], 8, 70).unwrap();
    let layers = Layers {
        biomes: true,
        ties: true,
        highlight: vec![world.biome_query("minecraft:forest").unwrap()],
        structures: vec![world.structure_query("minecraft:tower").unwrap()],
        slime_chunks: true,
        spawn: true,
    };
    let mut expected = Vec::new();
    session
        .render(&area, &layers)
        .unwrap()
        .write_png(&mut expected)
        .unwrap();
    assert_eq!(std::fs::read(&file).unwrap(), expected);

    let features = session.map_features(&area, &layers).unwrap();
    assert_eq!(answer["format"], 1);
    assert_eq!(answer["command"], "render");
    assert_eq!(answer["seed"], 42);
    assert_eq!(
        answer["area"],
        json!({"min": area.min, "size": [120, 90], "scale": 8, "y": 70})
    );
    assert_eq!(answer["layers"]["structures"], json!(["minecraft:tower"]));
    let marks = answer["structures"].as_array().unwrap();
    assert!(!marks.is_empty());
    assert_eq!(marks.len(), features.structures.len());
    for (mark, start) in marks.iter().zip(&features.structures) {
        assert_eq!(mark["chunk"], json!(start.chunk));
        assert_eq!(mark["certain"], start.certain);
    }
    assert_eq!(answer["slime_chunks"], json!(features.slime_chunks));
    assert_eq!(
        answer["spawn"]["chunk"],
        json!(session.spawn_origin().chunk)
    );
}

#[test]
fn corners_and_defaults() {
    let fixture = bundle();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("map.png");
    let path = file.to_str().unwrap();
    // The default: 1024 × 1024 at scale 4, centred on 0, 0, biomes only.
    let answer: Value = serde_json::from_slice(ok(&spyglass(
        &fixture.path(),
        &["--seed", "1", "-o", path, "--json"],
    )))
    .unwrap();
    assert_eq!(
        answer["area"],
        json!({"min": [-2048, -2048], "size": [1024, 1024], "scale": 4, "y": 64})
    );
    assert_eq!(answer["structures"], json!([]));
    assert_eq!(answer["spawn"], Value::Null);

    let text = ok(&spyglass(
        &fixture.path(),
        &[
            "--seed", "1", "--min", "-64,128", "--size", "16x8", "--scale", "16", "-o", path,
        ],
    ))
    .to_vec();
    let text = String::from_utf8(text).unwrap();
    assert!(
        text.contains("16 × 8 pixels, 16 blocks per pixel, x -64 to 191, z 128 to 255"),
        "{text}"
    );
}

#[test]
fn bad_requests_fail_without_writing() {
    let fixture = bundle();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("map.png");
    let path = file.to_str().unwrap();
    for args in [
        &["--seed", "1", "--scale", "3", "-o", path][..],
        &["--seed", "1", "--min", "1,0", "--scale", "4", "-o", path],
        &["--seed", "1", "--scale", "32", "--slime", "-o", path],
        &[
            "--seed",
            "1",
            "--structures",
            "minecraft:nothing",
            "-o",
            path,
        ],
        &["--seed", "1", "--size", "big", "-o", path],
        &["--seed", "1", "--centre", "player", "-o", path],
        &["--seed", "1", "--centre", "0,0", "--min", "0,0", "-o", path],
    ] {
        let out = spyglass(&fixture.path(), args);
        assert!(!out.status.success(), "{args:?}");
        assert!(out.stdout.is_empty());
        assert!(!file.exists(), "{args:?}");
    }
}
