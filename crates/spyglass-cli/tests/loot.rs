//! `spyglass loot` end to end: its answers are the library's. Tests that need a bundle are
//! ignored; they use the one for `spyglass_oracle::version` (`SPYGLASS_ORACLE_VERSION`, else the
//! newest release in `bundles/`). Run them with `cargo test -p spyglass-cli -- --ignored`.

use std::process::{Command, Output};

use serde_json::Value;

/// Seed -1's woodland mansion, whose chests are near this block in every supported version.
const MANSION: [i32; 3] = [4159, 73, -3340];

fn spyglass(args: &[&str]) -> Output {
    let bundles = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bundles");
    let version = spyglass_oracle::version();
    Command::new(env!("CARGO_BIN_EXE_spyglass"))
        .args(args)
        .arg("--bundles")
        .arg(bundles)
        .args(["--game-version", &version])
        .output()
        .unwrap()
}

fn json(args: &[&str]) -> Value {
    let mut args = args.to_vec();
    args.push("--json");
    let out = spyglass(&args);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let value: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["format"], 1);
    value
}

#[test]
fn bad_requests_fail_before_loading() {
    // An empty bundles folder: each request must be refused for what it asks, not for the
    // missing bundle, so this runs without one.
    let empty = tempfile::tempdir().unwrap();
    let refusal = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_spyglass"))
            .args(args)
            .arg("--bundles")
            .arg(empty.path())
            .output()
            .unwrap();
        assert!(!out.status.success(), "{args:?}");
        String::from_utf8_lossy(&out.stderr).into_owned()
    };
    let too_big = refusal(&[
        "loot",
        "in",
        "--min",
        "0,0",
        "--max",
        "10000,10000",
        "--seed",
        "1",
    ]);
    assert!(too_big.contains("more than 1024"), "{too_big}");
    let backwards = refusal(&["loot", "in", "--min", "10,0", "--max", "0,0", "--seed", "1"]);
    assert!(
        backwards.contains("--max must be at least --min"),
        "{backwards}"
    );
    let level_alone = refusal(&["loot", "find", "x", "--level", "2", "--seed", "1"]);
    assert!(level_alone.contains("--enchantment"), "{level_alone}");
}

#[test]
#[ignore = "needs bundles/<version> (SPYGLASS_ORACLE_VERSION, else the newest release)"]
fn loot_answers_are_the_library_s() {
    let world = spyglass::World::open(spyglass_oracle::bundle_dir()).unwrap();
    let session = world.session(-1).unwrap();
    let [x, _, z] = MANSION;
    let (min, max) = ([x - 24, z - 24], [x + 24, z + 24]);
    let ours: Vec<_> = session
        .containers_in([min[0] >> 4, min[1] >> 4], [max[0] >> 4, max[1] >> 4])
        .unwrap()
        .into_iter()
        .filter(|c| {
            (min[0]..=max[0]).contains(&c.position[0]) && (min[1]..=max[1]).contains(&c.position[2])
        })
        .collect();
    assert!(!ours.is_empty(), "no containers by seed -1's mansion");

    // `loot in`: the same containers, in order, with their contents.
    let area = |c: [i32; 2]| format!("{},{}", c[0], c[1]);
    let (min_text, max_text) = (area(min), area(max));
    let answer = json(&[
        "loot", "in", "--min", &min_text, "--max", &max_text, "--seed", "-1",
    ]);
    assert_eq!(answer["command"], "loot in");
    let listed = answer["containers"].as_array().unwrap();
    assert_eq!(listed.len(), ours.len());
    for (theirs, c) in listed.iter().zip(&ours) {
        assert_eq!(theirs["position"]["x"], c.position[0]);
        assert_eq!(theirs["position"]["y"], c.position[1]);
        assert_eq!(theirs["position"]["z"], c.position[2]);
        assert_eq!(theirs["table"], c.table.as_str());
        assert_eq!(theirs["holder"], c.holder.as_str());
        assert_eq!(theirs["certain"], c.certain);
        assert_eq!(theirs["loot_seed"], serde_json::json!(c.seed));
        let slots = c.contents.as_ref().map(|s| {
            s.iter()
                .map(|s| (s.slot, s.item.id.clone(), s.item.count))
                .collect::<Vec<_>>()
        });
        let listed_slots = theirs["contents"].as_array().map(|s| {
            s.iter()
                .map(|s| {
                    (
                        s["slot"].as_u64().unwrap() as usize,
                        s["item"].as_str().unwrap().to_string(),
                        s["count"].as_i64().unwrap() as i32,
                    )
                })
                .collect::<Vec<_>>()
        });
        assert_eq!(listed_slots, slots);
    }

    // `loot at` a certain container with contents, and `loot find` an item in it from there.
    let chest = ours
        .iter()
        .find(|c| c.contents_certain() && c.contents.as_ref().is_some_and(|s| !s.is_empty()))
        .expect("a certain container with contents");
    let [cx, cy, cz] = chest.position;
    let at = format!("{cx},{cy},{cz}");
    let answer = json(&["loot", "at", &at, "--seed", "-1"]);
    let here = answer["containers"].as_array().unwrap();
    assert_eq!(here.len(), 1);
    assert_eq!(here[0]["table"], chest.table.as_str());
    let item = &chest.contents.as_ref().unwrap()[0].item.id;
    let answer = json(&[
        "loot", "find", item, "--from", &at, "--radius", "1", "--seed", "-1",
    ]);
    assert_eq!(answer["complete"], true);
    let last = answer["hits"].as_array().unwrap().last().unwrap();
    assert_eq!(last["blocks_away"], 0, "{answer}");

    // Text: the container's line names its holder, structure and table.
    let out = spyglass(&["loot", "at", &at, "--seed", "-1"]);
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains(&chest.table), "{text}");
    assert!(text.contains("slot"), "{text}");
}
