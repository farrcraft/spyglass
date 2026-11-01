//! `spyglass blocks` on the hand-written fixture world (`spyglass_oracle::fixtures`): a flat
//! seabed of stone up to y 49 with sand on top and deepslate below y 0, under water to the sea
//! level at 63. No game data (ADR 0008), so these always run.

use std::path::Path;
use std::process::{Command, Output};

use serde_json::{Value, json};
use spyglass_oracle::fixtures;

fn spyglass(bundle: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_spyglass"))
        .arg("blocks")
        .args(args)
        .arg("--bundle")
        .arg(bundle)
        .output()
        .unwrap()
}

fn ok(out: &Output) -> String {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout.clone()).unwrap()
}

fn json(bundle: &Path, args: &[&str]) -> Value {
    let mut args = args.to_vec();
    args.extend(["--seed", "5", "--no-carvers", "--json"]);
    serde_json::from_str(&ok(&spyglass(bundle, &args))).unwrap()
}

#[test]
fn blocks_columns_and_chunks() {
    let fixture = fixtures::overworld(&[], false);
    let path = fixture.path();

    let v = json(&path, &["--at", "3,49,-20"]);
    assert_eq!(v["format"], 1);
    assert_eq!(v["command"], "blocks");
    assert_eq!(v["carvers"], false);
    assert_eq!(v["position"], json!([3, 49, -20]));
    assert_eq!(v["block"], "minecraft:sand");
    assert_eq!(v["certain"], true);

    let seabed = json!([
        ["minecraft:deepslate", 64],
        ["minecraft:stone", 49],
        ["minecraft:sand", 1],
        ["minecraft:water", 13],
        ["minecraft:air", 257]
    ]);
    let v = json(&path, &["--column", "-7,100"]);
    assert_eq!(v["min_y"], -64);
    assert_eq!(
        v["columns"],
        json!([{"x": -7, "z": 100, "blocks": seabed, "uncertain": []}])
    );

    let v = json(&path, &["--chunk", "-1,2"]);
    let columns = v["columns"].as_array().unwrap();
    assert_eq!(columns.len(), 256);
    assert_eq!(
        (&columns[0]["x"], &columns[0]["z"]),
        (&json!(-16), &json!(32))
    );
    assert_eq!(
        (&columns[17]["x"], &columns[17]["z"]),
        (&json!(-15), &json!(33))
    );
    assert!(columns.iter().all(|c| c["blocks"] == seabed));

    let text = ok(&spyglass(
        &path,
        &["--seed", "5", "--column", "0,0", "--no-carvers"],
    ));
    assert!(
        text.starts_with("Blocks at 0 0, top down (seed 5, Minecraft test)"),
        "{text}"
    );
    assert!(text.contains("        49  minecraft:sand"), "{text}");
    assert!(text.contains("  -1 to -64  minecraft:deepslate"), "{text}");
}

#[test]
fn carvers_are_on_by_default() {
    // The fixture's biomes have no carvers, so both answers are the same seabed.
    let fixture = fixtures::overworld(&[], false);
    let path = fixture.path();
    let out = spyglass(&path, &["--seed", "5", "--at", "3,49,-20", "--json"]);
    let v: Value = serde_json::from_str(&ok(&out)).unwrap();
    assert_eq!(
        (&v["carvers"], &v["block"]),
        (&json!(true), &json!("minecraft:sand"))
    );
}

#[test]
fn bad_requests_fail() {
    let fixture = fixtures::overworld(&[], false);
    let path = fixture.path();
    let fails = |args: &[&str]| {
        let out = spyglass(&path, args);
        assert!(!out.status.success(), "{args:?} succeeded");
        assert!(out.stdout.is_empty());
        String::from_utf8(out.stderr).unwrap()
    };
    // One of --at, --column and --chunk.
    fails(&["--seed", "5", "--no-carvers"]);
    fails(&[
        "--seed",
        "5",
        "--no-carvers",
        "--at",
        "0,0,0",
        "--chunk",
        "0,0",
    ]);
    fails(&["--seed", "5", "--no-carvers", "--at", "0,0"]);
}
