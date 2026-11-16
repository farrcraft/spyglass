//! `spyglass portal link` on the hand-written fixture bundle (`spyglass_oracle::fixtures`), whose
//! Nether has the game's coordinate scale of 8. No game data (ADR 0008), so these always run.

use std::path::Path;
use std::process::{Command, Output};

use serde_json::{Value, json};
use spyglass_oracle::fixtures;

fn spyglass(bundle: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_spyglass"))
        .args(["portal", "link"])
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

#[test]
fn a_portal_links_to_the_nearest_given_one() {
    let fixture = fixtures::overworld_and_nether();
    let path = fixture.path();
    let text = ok(&spyglass(
        &path,
        &[
            "nether",
            "100,64,100",
            "--portal",
            "overworld:800,64,800",
            "--portal",
            "minecraft:overworld:1000,64,800:z:3x4",
        ],
    ));
    assert!(text.contains("336 exit blocks, x 802 to 813"), "{text}");
    assert!(
        text.contains("all exits land at --portal 1 (the Overworld 800 64 800)"),
        "{text}"
    );

    let v: Value = serde_json::from_str(&ok(&spyglass(
        &path,
        &[
            "overworld",
            "800,64,800",
            "--axis",
            "z",
            "--portal",
            "nether:100,64,100",
            "--json",
        ],
    )))
    .unwrap();
    assert_eq!(v["format"], 1);
    assert_eq!(v["command"], "portal link");
    assert_eq!(v["version"], "test");
    assert_eq!(v["to"], "minecraft:the_nether");
    assert_eq!(v["radius"], 16);
    assert_eq!(
        v["portal"],
        json!({"dimension": "minecraft:overworld", "corner": [800, 64, 800], "axis": "z",
            "width": 2, "height": 3})
    );
    assert_eq!(
        v["exits"],
        json!({"x": [99, 100], "y": [64, 65], "z": [100, 100]})
    );
    assert_eq!(
        v["outcomes"],
        json!([{"exits": 4, "tied": false, "portals": [{"source": "given", "index": 0,
            "dimension": "minecraft:the_nether", "corner": [100, 64, 100], "axis": "x",
            "width": 2, "height": 3}]}])
    );
}

#[test]
fn a_portal_between_two_is_split_and_one_alone_builds_a_new_one() {
    let fixture = fixtures::overworld_and_nether();
    let path = fixture.path();
    // Out of the Nether, x 2 to 13: nearer -40 for the west part, 52 for the east.
    let v: Value = serde_json::from_str(&ok(&spyglass(
        &path,
        &[
            "nether",
            "0,64,0",
            "--portal",
            "overworld:-40,64,4:z",
            "--portal",
            "overworld:52,64,4:z",
            "--json",
        ],
    )))
    .unwrap();
    let outcomes = v["outcomes"].as_array().unwrap();
    assert_eq!(outcomes.len(), 2);
    let total: u64 = outcomes.iter().map(|o| o["exits"].as_u64().unwrap()).sum();
    assert_eq!(total, 336);

    let text = ok(&spyglass(&path, &["overworld", "0,64,0"]));
    assert!(
        text.contains("all exits find no portal in range: the game builds one"),
        "{text}"
    );
}

#[test]
fn bad_portals_are_errors() {
    let fixture = fixtures::overworld_and_nether();
    let path = fixture.path();
    for args in [
        &["overworld", "0,64,0", "--size", "2x1"][..],
        &["overworld", "0,64,0", "--size", "22x3"],
        &["overworld", "0,64,0", "--portal", "nether:1,2"],
        &[],
    ] {
        let out = spyglass(&path, args);
        assert!(!out.status.success(), "{args:?}");
        assert!(out.stdout.is_empty());
    }
}
