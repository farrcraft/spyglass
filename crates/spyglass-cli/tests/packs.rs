//! `--pack` for a seed, on the hand-written world (`spyglass_oracle::fixtures`) with a pack built
//! in the test. No game data (ADR 0008), so these always run.

use std::path::Path;
use std::process::Command;

use serde_json::Value;
use spyglass_oracle::fixtures;

/// A pack in `dir` that turns the fixture's temperature gradient round: desert west of x = 0,
/// plains east of it.
fn turned_pack(fixture: &fixtures::TestBundle, dir: &Path) {
    let mut settings = fixture
        .open()
        .resource("worldgen/noise_settings", "overworld")
        .unwrap();
    let temperature = &mut settings["noise_router"]["temperature"];
    let (from, to) = (
        temperature["from_value"].clone(),
        temperature["to_value"].clone(),
    );
    temperature["from_value"] = to;
    temperature["to_value"] = from;
    let file = dir.join("data/minecraft/worldgen/noise_settings/overworld.json");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(file, settings.to_string()).unwrap();
    std::fs::write(
        dir.join("pack.mcmeta"),
        r#"{"pack": {"description": "", "min_format": 121, "max_format": 121}}"#,
    )
    .unwrap();
}

/// `locate biome minecraft:desert` from x = 0, `--json`, with `extra` arguments.
fn locate_desert(fixture: &fixtures::TestBundle, extra: &[&str]) -> Value {
    let out = Command::new(env!("CARGO_BIN_EXE_spyglass"))
        .args([
            "locate",
            "biome",
            "minecraft:desert",
            "--seed",
            "1",
            "--json",
        ])
        .args(extra)
        .arg("--bundle")
        .arg(fixture.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
fn packs_apply_to_a_seed() {
    let fixture = fixtures::overworld(&[], false);
    let pack = tempfile::tempdir().unwrap();
    turned_pack(&fixture, pack.path());
    let plain = locate_desert(&fixture, &[]);
    let turned = locate_desert(&fixture, &["--pack", pack.path().to_str().unwrap()]);
    let x = |v: &Value| v["hits"][0]["position"]["x"].as_i64().unwrap();
    // Desert is east of x = 0 in the fixture, and west of it with the pack.
    assert!(x(&plain) > 0, "{plain:#}");
    assert!(x(&turned) <= 0, "{turned:#}");
}

#[test]
fn a_pack_that_cant_be_read_is_named() {
    let fixture = fixtures::overworld(&[], false);
    let dir = tempfile::tempdir().unwrap();
    // A folder without pack.mcmeta isn't a pack.
    let out = Command::new(env!("CARGO_BIN_EXE_spyglass"))
        .args(["locate", "spawn", "--seed", "1", "--pack"])
        .arg(dir.path())
        .arg("--bundle")
        .arg(fixture.path())
        .output()
        .unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("reading the data pack") && err.contains("pack.mcmeta"),
        "{err}"
    );
}
