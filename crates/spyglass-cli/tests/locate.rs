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
