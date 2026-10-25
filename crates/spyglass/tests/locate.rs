//! Locate queries against the real game's `/locate` answers (`docs/specs/oracle.md`, ADR 0015).
//!
//! Needs `cargo xtask oracle locate` and a bundle for the same version. Run with
//! `cargo test --release -p spyglass --test locate -- --ignored --nocapture`.

use serde_json::Value;
use spyglass::World;

fn ints(v: &Value) -> Vec<Option<i32>> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|n| n.as_i64().map(|n| n as i32))
        .collect()
}

#[test]
#[ignore = "needs oracle-out/<version>/locate and bundles/<version>: cargo xtask oracle locate"]
fn locate_agrees_with_the_game() {
    let root = spyglass_oracle::default_dir();
    let mut failures = Vec::new();
    let (mut cases, mut first, mut inconclusive) = (0, 0, 0);
    for entry in std::fs::read_dir(&root).expect("no oracle-out/").flatten() {
        let version = entry.file_name().to_string_lossy().into_owned();
        let Ok(files) = std::fs::read_dir(entry.path().join("locate")) else {
            continue;
        };
        let world = World::open(root.join("../bundles").join(&version))
            .unwrap_or_else(|e| panic!("{e}; run spyglass-extract {version}"));
        for file in files.flatten() {
            let json: Value =
                serde_json::from_str(&std::fs::read_to_string(file.path()).unwrap()).unwrap();
            let seed = spyglass::parse_seed(json["seed"].as_str().unwrap()).unwrap();
            let session = world.session(seed).unwrap();
            for case in json["cases"].as_array().unwrap() {
                cases += 1;
                let target = case["target"].as_str().unwrap();
                let from: Vec<i32> = ints(&case["from"]).into_iter().flatten().collect();
                let from = [from[0], from[1], from[2]];
                let game = case["answer"].as_array().map(|_| ints(&case["answer"]));
                let name = format!("seed {seed}: {target} from {from:?}");
                // Our hits as (x, z) or (x, y, z), whether the list is complete, and whether
                // it holds a certain hit.
                let (hits, complete, certain): (Vec<[Option<i32>; 3]>, bool, bool) =
                    match case["kind"].as_str().unwrap() {
                        "structure" => {
                            let r = session
                                .locate_structure(target, from, spyglass::LOCATE_RADIUS)
                                .unwrap();
                            (
                                r.hits
                                    .iter()
                                    .map(|h| {
                                        let p = h.start.position;
                                        [Some(p[0]), None, Some(p[2])]
                                    })
                                    .collect(),
                                r.complete,
                                r.certain_answer.is_some(),
                            )
                        }
                        _ => {
                            let r = session
                                .locate_biome(target, from, spyglass::LOCATE_BIOME_RADIUS)
                                .unwrap();
                            let certain = r.hits.last().is_some_and(|h| h.certain);
                            (
                                r.hits.iter().map(|h| h.position.map(Some)).collect(),
                                true,
                                certain,
                            )
                        }
                    };
                match game {
                    None if certain => failures.push(format!(
                        "{name}: the game found nothing, we're certain of {:?}",
                        hits.last()
                    )),
                    None => {}
                    Some(answer) => {
                        let answer = [answer[0], answer[1], answer[2]];
                        match hits.iter().position(|h| *h == answer) {
                            Some(0) => first += 1,
                            Some(_) => {}
                            None if !complete => inconclusive += 1,
                            None => failures.push(format!(
                                "{name}: the game's {answer:?} isn't among our {} hits: {:?}",
                                hits.len(),
                                &hits[..hits.len().min(5)]
                            )),
                        }
                    }
                }
            }
        }
    }
    assert!(cases > 0, "no locate files; run cargo xtask oracle locate");
    println!(
        "{cases} cases: the game's answer was our first hit in {first}; \
         {inconclusive} beyond our candidate cap"
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
