//! `spyglass locate`'s text output on the hand-written world (`spyglass_oracle::fixtures`). No
//! game data (ADR 0008), so these always run.

use std::process::Command;

use spyglass_oracle::fixtures;

#[test]
fn candidates_are_explained_in_plain_text() {
    // Huts need terrain, so unconfirmed they're only ever candidates, and the text says why.
    let fixture = fixtures::structured_overworld(false);
    let out = Command::new(env!("CARGO_BIN_EXE_spyglass"))
        .args(["locate", "structure", "minecraft:hut", "--seed", "5"])
        .arg("--unconfirmed")
        .arg("--bundle")
        .arg(fixture.path())
        .output()
        .unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        text.contains(
            "Candidates depend on terrain, which --unconfirmed leaves undecided, or a tied \
             biome. The game's answer is the first of these it generates, usually the first \
             listed"
        ),
        "{text}"
    );
    // Prose (the unindented lines; the table's are aligned with spaces) has one space between
    // words: no source indentation leaked into a message.
    for line in text.lines().filter(|l| !l.starts_with(' ')) {
        assert!(!line.contains("  "), "{line:?}");
    }
}

#[test]
fn nearest_is_named_in_text_and_json() {
    let fixture = fixtures::structured_overworld(false);
    let run = |extra: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_spyglass"))
            .args(["locate", "structure", "minecraft:hut", "--seed", "5"])
            .args(["--unconfirmed", "--nearest"])
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
        String::from_utf8(out.stdout).unwrap()
    };
    let text = run(&[]);
    assert!(
        text.starts_with("nearest minecraft:hut from 0 64 0"),
        "{text}"
    );
    assert!(
        text.contains("The nearest is the first of these that generates"),
        "{text}"
    );
    let json: serde_json::Value = serde_json::from_str(&run(&["--json"])).unwrap();
    assert_eq!(json["mode"], "nearest");
    // Nearest first.
    let away: Vec<i64> = json["hits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["blocks_away"].as_i64().unwrap())
        .collect();
    assert!(away.is_sorted(), "{away:?}");
}
