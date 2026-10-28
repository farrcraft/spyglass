//! `spyglass heights` on the hand-written fixture world (`spyglass_oracle::fixtures`), whose
//! terrain is a flat seabed: stone up to y 49, water to the sea level at 63. No game data
//! (ADR 0008), so these always run.

use std::path::Path;
use std::process::{Command, Output};

use serde_json::{Value, json};
use spyglass_oracle::fixtures;

fn spyglass(bundle: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_spyglass"))
        .arg("heights")
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
fn columns_and_regions() {
    let fixture = fixtures::overworld(&[], false);
    let path = fixture.path();
    let v: Value = serde_json::from_str(&ok(&spyglass(
        &path,
        &["--seed", "5", "--min", "-2,7", "--max", "1,8", "--json"],
    )))
    .unwrap();
    assert_eq!(v["format"], 1);
    assert_eq!(v["command"], "heights");
    assert_eq!(v["seed"], 5);
    assert_eq!(v["heightmap"], "WORLD_SURFACE_WG");
    assert_eq!(v["min"], json!([-2, 7]));
    assert_eq!(v["max"], json!([1, 8]));
    assert_eq!(v["heights"], json!([[63, 63, 63, 63], [63, 63, 63, 63]]));

    let v: Value = serde_json::from_str(&ok(&spyglass(
        &path,
        &[
            "--seed",
            "5",
            "--at",
            "-100,3",
            "--heightmap",
            "ocean-floor",
            "--json",
        ],
    )))
    .unwrap();
    assert_eq!(v["heightmap"], "OCEAN_FLOOR_WG");
    assert_eq!(v["heights"], json!([[50]]));

    let text = ok(&spyglass(&path, &["--seed", "5", "--at", "0,0"]));
    assert!(
        text.starts_with("Height at 0 0 (seed 5, Minecraft test)"),
        "{text}"
    );
    assert!(text.contains("WORLD_SURFACE_WG: 63"), "{text}");
    let text = ok(&spyglass(
        &path,
        &["--seed", "5", "--min", "0,0", "--max", "2,1"],
    ));
    assert!(text.contains("z       1   63  63  63"), "{text}");
}

#[test]
fn bad_requests_fail() {
    let fixture = fixtures::overworld(&[], false);
    for args in [
        &["--seed", "1"][..],
        &["--seed", "1", "--min", "0,0"],
        &["--seed", "1", "--at", "0,0", "--min", "0,0", "--max", "1,1"],
        &["--seed", "1", "--min", "5,0", "--max", "4,0"],
        &["--seed", "1", "--min", "0,0", "--max", "256,255"],
        &["--seed", "1", "--at", "0,0", "--heightmap", "motion"],
    ] {
        let out = spyglass(&fixture.path(), args);
        assert!(!out.status.success(), "{args:?}");
        assert!(out.stdout.is_empty(), "{args:?}");
    }
}

#[test]
fn a_closed_pipe_ends_quietly() {
    // Reading one line and closing the pipe, as `| head -1` does: the command stops with
    // success, not a panic about printing.
    use std::io::{BufRead, BufReader};
    use std::process::Stdio;
    let fixture = fixtures::overworld(&[], false);
    let mut child = Command::new(env!("CARGO_BIN_EXE_spyglass"))
        .args(["heights", "--seed", "5", "--min", "0,0", "--max", "255,255"])
        .arg("--bundle")
        .arg(fixture.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut first = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut first)
        .unwrap();
    assert!(!first.is_empty());
    let out = child.wait_with_output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{:?}: {stderr}", out.status);
    assert!(!stderr.contains("panicked"), "{stderr}");
}
