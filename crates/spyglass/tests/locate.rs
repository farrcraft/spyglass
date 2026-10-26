//! Locate queries against the real game's `/locate` answers (`docs/specs/oracle.md`, ADR 0015),
//! structures both from the climate alone and confirmed with terrain.
//!
//! Needs `cargo xtask oracle locate` and a bundle for the same version. Run with
//! `cargo test --release -p spyglass --test locate -- --ignored --nocapture`.

use serde_json::Value;
use spyglass::{Dimension, World};

fn ints(v: &Value) -> Vec<Option<i32>> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|n| n.as_i64().map(|n| n as i32))
        .collect()
}

/// A case's dimension: format 1 files have only Overworld cases, and no dimension per case.
fn dimension(case: &Value) -> Dimension {
    case["dimension"]
        .as_str()
        .map_or(Dimension::Overworld, |d| {
            Dimension::from_id(d).expect("a dimension")
        })
}

#[test]
#[ignore = "needs oracle-out/<version>/locate and bundles/<version>: cargo xtask oracle locate"]
fn locate_agrees_with_the_game() {
    let root = spyglass_oracle::default_dir();
    let mut failures = Vec::new();
    let (mut cases, mut first, mut inconclusive) = (0, 0, 0);
    // Confirmed structure cases, and those whose hits were all certain, so that spyglass's
    // answer was exactly the game's.
    let (mut confirmed, mut decided) = (0, 0);
    let mut by_dimension: std::collections::BTreeMap<Dimension, usize> = Default::default();
    for entry in std::fs::read_dir(&root).expect("no oracle-out/").flatten() {
        let version = entry.file_name().to_string_lossy().into_owned();
        let Ok(files) = std::fs::read_dir(entry.path().join("locate")) else {
            continue;
        };
        let worlds = Dimension::ALL.map(|d| {
            World::open_dimension(root.join("../bundles").join(&version), d)
                .unwrap_or_else(|e| panic!("{e}; run spyglass-extract {version}"))
        });
        for file in files.flatten() {
            let json: Value =
                serde_json::from_str(&std::fs::read_to_string(file.path()).unwrap()).unwrap();
            let seed = spyglass::parse_seed(json["seed"].as_str().unwrap()).unwrap();
            // A save's cases go through the save, as `--world` does.
            let from_save = json["world_dir"].as_str().map(|dir| {
                let report =
                    spyglass::inspect_save(dir, root.join("../bundles"), spyglass::Scan::Quick)
                        .unwrap();
                assert_eq!(report.save.seed, seed, "{dir}");
                report.open_world(spyglass::Dimension::Overworld).unwrap()
            });
            // The save's Overworld goes through the save; its other dimensions aren't opened
            // from saves yet, so their cases are skipped.
            let sessions = Dimension::ALL.map(|d| {
                let world = &worlds[d as usize];
                let world = match (&from_save, d) {
                    (Some(save), Dimension::Overworld) => save,
                    _ => world,
                };
                world.session(seed).unwrap()
            });
            let session_for = |case: &Value| {
                let d = dimension(case);
                (from_save.is_none() || d == Dimension::Overworld).then(|| &sessions[d as usize])
            };
            let structure_cases = json["cases"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|c| c["kind"] == "structure");
            for case in structure_cases {
                let Some(session) = session_for(case) else {
                    continue;
                };
                let target = case["target"].as_str().unwrap();
                let from: Vec<i32> = ints(&case["from"]).into_iter().flatten().collect();
                let from = [from[0], from[1], from[2]];
                let game = case["answer"].as_array().map(|_| ints(&case["answer"]));
                let name = format!(
                    "seed {seed}: {target} from {from:?} in {}, confirmed",
                    dimension(case)
                );
                let query = session.world().structure_query(target).unwrap();
                let r = session
                    .locate_structure(&query, from, &spyglass::LocateOptions::default())
                    .unwrap();
                confirmed += 1;
                let hits: Vec<[Option<i32>; 3]> = r
                    .hits
                    .iter()
                    .map(|h| [Some(h.start.position[0]), None, Some(h.start.position[2])])
                    .collect();
                let ours = r.certain_answer.map(|i| hits[i]);
                let all_certain = r.complete && r.hits.iter().all(|h| h.start.certain);
                let game = game.map(|a| [a[0], a[1], a[2]]);
                if all_certain {
                    decided += 1;
                    if ours != game {
                        failures.push(format!("{name}: the game's {game:?}, ours {ours:?}"));
                    }
                } else if let Some(answer) = game
                    && r.complete
                    && !hits.contains(&answer)
                {
                    failures.push(format!(
                        "{name}: the game's {answer:?} isn't among our {} hits",
                        hits.len()
                    ));
                } else if game.is_none() && ours.is_some() {
                    failures.push(format!(
                        "{name}: the game found nothing, we're certain of {ours:?}"
                    ));
                }
            }
            for case in json["cases"].as_array().unwrap() {
                let Some(session) = session_for(case) else {
                    continue;
                };
                *by_dimension.entry(dimension(case)).or_default() += 1;
                cases += 1;
                let target = case["target"].as_str().unwrap();
                let from: Vec<i32> = ints(&case["from"]).into_iter().flatten().collect();
                let from = [from[0], from[1], from[2]];
                let game = case["answer"].as_array().map(|_| ints(&case["answer"]));
                let name = format!("seed {seed}: {target} from {from:?} in {}", dimension(case));
                // Our hits as (x, z) or (x, y, z), whether the list is complete, and whether
                // it holds a certain hit.
                let (hits, complete, certain): (Vec<[Option<i32>; 3]>, bool, bool) =
                    match case["kind"].as_str().unwrap() {
                        "structure" => {
                            let query = session.world().structure_query(target).unwrap();
                            let r = session
                                .locate_structure(
                                    &query,
                                    from,
                                    &spyglass::LocateOptions {
                                        radius: spyglass::LOCATE_RADIUS,
                                        confirm: false,
                                    },
                                )
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
                            let query = session.world().biome_query(target).unwrap();
                            let r = session
                                .locate_biome(&query, from, spyglass::LOCATE_BIOME_RADIUS)
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
        "{cases} cases {by_dimension:?}: the game's answer was our first hit in {first}; \
         {inconclusive} beyond our candidate cap. {confirmed} structure cases confirmed, \
         {decided} with every hit certain, all equal to the game's"
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
#[ignore = "needs oracle-out/<version>/structures and bundles/<version>: cargo xtask oracle structures"]
fn layouts_agree_with_the_game() {
    let root = spyglass_oracle::default_dir();
    let version = "26.3";
    let world = World::open(root.join("../bundles").join(version)).unwrap();
    let dir = root.join(version).join("structures/overworld");
    // One file is enough here: the structure crate compares every start.
    let file = std::fs::read_dir(&dir)
        .expect("no structure golden files")
        .flatten()
        .next()
        .unwrap();
    let json: Value = serde_json::from_str(&std::fs::read_to_string(file.path()).unwrap()).unwrap();
    let session = world.session(json["seed"].as_i64().unwrap()).unwrap();
    let box_of = |v: &Value| {
        let b: Vec<i32> = ints(v).into_iter().flatten().collect();
        spyglass::BlockBox {
            min: [b[0], b[1], b[2]],
            max: [b[3], b[4], b[5]],
        }
    };
    let mut layouts = 0;
    for set in json["sets"].as_array().unwrap() {
        for c in set["chunks"].as_array().unwrap() {
            let chunk: Vec<i32> = ints(&c["chunk"]).into_iter().flatten().collect();
            for t in c["tries"].as_array().unwrap() {
                let Some(bounds) = t.get("bounds") else {
                    continue;
                };
                let id = t["structure"].as_str().unwrap();
                let Some(layout) = session.structure_layout(id, [chunk[0], chunk[1]]).unwrap()
                else {
                    continue; // not a jigsaw
                };
                let pieces: Vec<_> = t["pieces"].as_array().unwrap().iter().map(box_of).collect();
                assert_eq!(layout.bounds, box_of(bounds), "{id} at {chunk:?}");
                assert_eq!(layout.pieces, pieces, "{id} at {chunk:?}");
                layouts += 1;
            }
        }
    }
    assert!(layouts > 10, "only {layouts} jigsaw layouts");
    assert!(
        session
            .structure_layout("minecraft:swamp_hut", [0, 0])
            .unwrap()
            .is_none()
    );
    assert!(
        session
            .structure_layout("minecraft:nothing", [0, 0])
            .is_err()
    );
    println!("{layouts} layouts equal to the game's");
}
