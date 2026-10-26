//! `spyglass search` on the hand-written fixture world (`spyglass_oracle::fixtures`): no game
//! data (ADR 0008), so these always run. The command must report what the library does for
//! the same query and options.

use std::ops::ControlFlow;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;
use spyglass::{SearchOptions, SeedSource, World};
use spyglass_oracle::fixtures;

const QUERY: &str = "require 2 tower within 400 of -200,0\n\
                     prefer 3 hut within 300 of -100,0 weight 2\n\
                     prefer slime within 64 of spawn";

fn spyglass(bundle: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_spyglass"))
        .arg("search")
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
fn the_command_reports_what_the_library_finds() {
    let fixture = fixtures::structured_overworld(false);
    let out = spyglass(
        &fixture.path(),
        &[
            "-q",
            QUERY,
            "--range",
            "-500..500",
            "--from",
            "3",
            "--count",
            "900",
            "--top",
            "7",
            "--threads",
            "3",
            "--json",
        ],
    );
    let json: Value = serde_json::from_str(&ok(&out)).unwrap();

    let world = World::open(fixture.path()).unwrap();
    let query = world.search_query(QUERY).unwrap();
    let options = SearchOptions {
        source: SeedSource::Range {
            first: -500,
            last: 500,
        },
        from: 3,
        seeds: Some(900),
        top: 7,
        ..SearchOptions::default()
    };
    let expected = world
        .search(&query, &options, |_| ControlFlow::Continue(()))
        .unwrap();

    assert_eq!(json["format"], 1);
    assert_eq!(json["command"], "search");
    assert_eq!(json["query"], query.to_string());
    assert_eq!(
        json["source"],
        serde_json::json!({"kind": "range", "first": -500, "last": 500})
    );
    assert_eq!(json["next"], 903);
    assert_eq!(json["evaluated"], 900);
    assert_eq!(json["reported"], expected.progress.reported);
    let hits = json["hits"].as_array().unwrap();
    assert_eq!(hits.len(), expected.hits.len());
    assert!(!hits.is_empty());
    for (got, want) in hits.iter().zip(&expected.hits) {
        assert_eq!(got["seed"], want.seed);
        assert_eq!(got["index"], want.index);
        assert_eq!(got["score"], want.score);
        let constraints = got["constraints"].as_array().unwrap();
        assert_eq!(constraints.len(), 3);
        assert_eq!(constraints[0]["required"], true);
        assert_eq!(
            constraints[0]["constraint"],
            "require 2 minecraft:tower within 400 of -200,0"
        );
        for (c, r) in constraints.iter().zip(&want.evaluation.constraints) {
            assert_eq!(c["best"], r.best);
            assert_eq!(c["features"].as_array().unwrap().len(), r.features.len());
        }
    }
    let kinds: Vec<&str> = hits
        .iter()
        .flat_map(|h| h["constraints"].as_array().unwrap())
        .flat_map(|c| c["features"].as_array().unwrap())
        .map(|f| f["kind"].as_str().unwrap())
        .collect();
    assert!(kinds.contains(&"structure"));
}

#[test]
fn confirmed_searches_are_the_library_s() {
    let fixture = fixtures::structured_overworld(false);
    let query = "require 2 hut within 400 of 0,0";
    let json: Value = serde_json::from_str(&ok(&spyglass(
        &fixture.path(),
        &["-q", query, "--range", "0..299", "--confirm", "--json"],
    )))
    .unwrap();
    let world = World::open(fixture.path()).unwrap();
    let options = SearchOptions {
        source: SeedSource::Range {
            first: 0,
            last: 299,
        },
        confirm: true,
        ..SearchOptions::default()
    };
    let expected = world
        .search(&world.search_query(query).unwrap(), &options, |_| {
            ControlFlow::Continue(())
        })
        .unwrap();
    assert_eq!(json["confirm"], true);
    assert_eq!(json["reported"], expected.progress.reported);
    let hits = json["hits"].as_array().unwrap();
    assert_eq!(hits.len(), expected.hits.len());
    assert!(!hits.is_empty());
    for (got, want) in hits.iter().zip(&expected.hits) {
        assert_eq!(got["seed"], want.seed);
        assert_eq!(got["score"], want.score);
        assert_eq!(got["verdict"], "met");
    }
    let text = ok(&spyglass(
        &fixture.path(),
        &["-q", query, "--range", "0..9", "--confirm"],
    ));
    assert!(text.contains(", confirmed):"), "{text}");
}

#[test]
fn text_output_and_errors() {
    let fixture = fixtures::structured_overworld(false);
    let text = ok(&spyglass(
        &fixture.path(),
        &["-q", QUERY, "--range", "0..99", "--top", "2"],
    ));
    assert!(
        text.contains("require 2 minecraft:tower within 400 of -200,0"),
        "{text}"
    );
    assert!(
        text.contains("every seed in the source was searched"),
        "{text}"
    );
    assert!(text.contains("  1. seed "), "{text}");

    let out = spyglass(
        &fixture.path(),
        &[
            "-q",
            "require tower\nprefer hut within 5 of nowhere",
            "--count",
            "1",
        ],
    );
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("line 1, column 14: expected `within` or `in` at the end of the line"),
        "{err}"
    );

    let dir = tempfile::tempdir().unwrap();
    let seeds = dir.path().join("seeds.txt");
    std::fs::write(&seeds, "1\n\nspyglass\n-7\n").unwrap();
    let json: Value = serde_json::from_str(&ok(&spyglass(
        &fixture.path(),
        &["-q", QUERY, "--seeds", seeds.to_str().unwrap(), "--json"],
    )))
    .unwrap();
    assert_eq!(
        json["source"],
        serde_json::json!({"kind": "list", "count": 3})
    );
    assert_eq!(json["evaluated"], 3);
    assert_eq!(json["next"], Value::Null);
}

#[test]
fn searches_span_dimensions() {
    let fixture = fixtures::structured_overworld_and_nether();
    let query =
        "require 2 fort within 200 of spawn\nprefer tower within 300 of fort in -50,-50 to 50,50";
    let json: Value = serde_json::from_str(&ok(&spyglass(
        &fixture.path(),
        &["-q", query, "--range", "0..199", "--top", "5", "--json"],
    )))
    .unwrap();
    let game = spyglass::Game::open(fixture.path()).unwrap();
    let resolved = game.search_query(query).unwrap();
    let options = SearchOptions {
        source: SeedSource::Range {
            first: 0,
            last: 199,
        },
        top: 5,
        ..SearchOptions::default()
    };
    let expected = game
        .search(&resolved, &options, |_| ControlFlow::Continue(()))
        .unwrap();
    assert_eq!(json["dimension"], "minecraft:the_nether");
    assert_eq!(json["query"], resolved.to_string());
    let hits = json["hits"].as_array().unwrap();
    assert!(!hits.is_empty());
    assert_eq!(hits.len(), expected.hits.len());
    for (got, want) in hits.iter().zip(&expected.hits) {
        assert_eq!(got["seed"], want.seed);
        assert_eq!(got["score"], want.score);
        let dimensions: Vec<&str> = got["constraints"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["dimension"].as_str().unwrap())
            .collect();
        assert_eq!(dimensions, ["minecraft:the_nether", "minecraft:overworld"]);
    }
    // A dimension that can't have the target is an error.
    let out = spyglass(
        &fixture.path(),
        &["-q", "require nether tower within 5 of 0,0"],
    );
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("isn't placed in the Nether"));
}
