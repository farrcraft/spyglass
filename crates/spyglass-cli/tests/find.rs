//! `spyglass find` on the hand-written world (`spyglass_oracle::fixtures`). No game data
//! (ADR 0008), so these always run.

use std::process::{Command, Output};

use serde_json::Value;
use spyglass_oracle::fixtures;

fn find(fixture: &fixtures::TestBundle, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_spyglass"))
        .arg("find")
        .args(args)
        .arg("--bundle")
        .arg(fixture.path())
        .output()
        .unwrap()
}

fn stdout(out: &Output) -> String {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout.clone()).unwrap()
}

const PAIRS: &str = "find tower within 1500\nrequire tower within 200 of here";

#[test]
fn matches_in_text_and_json() {
    let fixture = fixtures::structured_overworld(false);
    let text = stdout(&find(
        &fixture,
        &["--seed", "3", "--from", "-700,40", "-q", PAIRS],
    ));
    assert!(text.contains("matches, "), "{text}");
    assert!(
        text.contains("  find minecraft:tower within 1500\n"),
        "{text}"
    );
    assert!(text.contains("  1. "), "{text}");
    let json: Value = serde_json::from_str(&stdout(&find(
        &fixture,
        &["--seed", "3", "--from", "-700,40", "-q", PAIRS, "--json"],
    )))
    .unwrap();
    assert_eq!(json["format"], 1);
    assert_eq!(json["command"], "find");
    assert_eq!(json["dimension"], "minecraft:overworld");
    assert_eq!(json["seed"], 3);
    assert_eq!(json["origin"], serde_json::json!({"x": -700, "z": 40}));
    assert_eq!(json["complete"], true);
    let matches = json["matches"].as_array().unwrap();
    assert!(!matches.is_empty());
    let distances: Vec<u64> = matches
        .iter()
        .map(|m| m["distance"].as_u64().unwrap())
        .collect();
    assert!(distances.is_sorted(), "{distances:?}");
    let first = &matches[0];
    assert_eq!(first["target"]["kind"], "structure");
    assert_eq!(first["verdict"], "met");
    let constraint = &first["constraints"][0];
    assert_eq!(
        constraint["constraint"],
        "require minecraft:tower within 200 of here"
    );
    assert_eq!(constraint["outcome"], "met");
    // `--top` keeps the nearest.
    let one: Value = serde_json::from_str(&stdout(&find(
        &fixture,
        &[
            "--seed", "3", "--from", "-700,40", "-q", PAIRS, "--json", "--top", "1",
        ],
    )))
    .unwrap();
    assert_eq!(one["matches"].as_array().unwrap()[..], matches[..1]);
}

#[test]
fn queries_without_a_find_line_are_refused() {
    let fixture = fixtures::structured_overworld(false);
    let out = find(
        &fixture,
        &["--seed", "3", "-q", "require tower within 200 of spawn"],
    );
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("no `find` line"), "{err}");
    let out = find(&fixture, &["--seed", "3", "--from", "player", "-q", PAIRS]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("--world"), "{err}");
}
