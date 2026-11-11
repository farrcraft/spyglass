//! The real saves on this machine: each world spyglass can answer for, and that no older version
//! generated part of, must have its saved spawn in the 11 × 11 chunks around spyglass's spawn
//! origin (`docs/specs/spawn.md`). A spawn moved by `/setworldspawn` would fail this, so it's a
//! sanity check of the user's own worlds, ignored by default. Run with
//! `cargo test --release -p spyglass --test saves -- --ignored --nocapture`.

use spyglass::{SaveWarning, Scan};

#[test]
#[ignore = "reads the saves on this machine"]
fn saved_spawns_are_near_the_spawn_origin() {
    let bundles = spyglass_oracle::default_dir().join("../bundles");
    let (mut checked, mut failures) = (0, Vec::new());
    for found in spyglass::launcher_saves() {
        let Ok(quick) = spyglass::inspect_save(&found.dir, &bundles, Scan::Quick) else {
            continue;
        };
        if !quick.answerable_in(spyglass::Dimension::Overworld) {
            continue;
        }
        let report = spyglass::inspect_save(&found.dir, &bundles, Scan::Chunks).unwrap();
        let upgraded = report.warnings.iter().any(|w| {
            matches!(
                w,
                SaveWarning::OlderChunks {
                    dimension: spyglass::Dimension::Overworld,
                    ..
                }
            )
        });
        let Some(spawn) = report.save.spawn.as_ref().filter(|_| !upgraded) else {
            continue;
        };
        let world = report.open_world(spyglass::Dimension::Overworld).unwrap();
        let origin = world.session(report.save.seed).unwrap().spawn_origin();
        let [x, _, z] = spawn.block();
        let chunk = [x >> 4, z >> 4];
        let offset = [chunk[0] - origin.chunk[0], chunk[1] - origin.chunk[1]];
        println!(
            "{}: spawn chunk {chunk:?}, origin {:?}",
            found.dir.display(),
            origin.chunk
        );
        checked += 1;
        if offset.iter().any(|d| d.abs() > 5) {
            failures.push(format!(
                "{}: spawn chunk {chunk:?} is outside the 11 x 11 around {:?} (moved by a command?)",
                found.dir.display(),
                origin.chunk
            ));
        }
    }
    println!("{checked} saves checked");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// In each answerable save that no older version generated part of, the structure starts the
/// save holds near its spawn are the ones spyglass predicts (ADR 0071): a saved start is a
/// certain predicted start of the same structure, and a certain predicted start in a chunk the
/// save holds is saved. A check of the predictions against the game's own record, with no
/// oracle run. Prints every disagreement.
#[test]
#[ignore = "reads the saves on this machine"]
fn saved_structure_starts_are_the_predicted_ones() {
    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::Arc;

    use spyglass::Dimension;

    let bundles = spyglass_oracle::default_dir().join("../bundles");
    let (mut saves, mut agreed, mut failures) = (0, 0, Vec::new());
    for found in spyglass::launcher_saves() {
        let Ok(quick) = spyglass::inspect_save(&found.dir, &bundles, Scan::Quick) else {
            continue;
        };
        if !quick.answerable_in(Dimension::Overworld) {
            continue;
        }
        let report = spyglass::inspect_save(&found.dir, &bundles, Scan::Chunks).unwrap();
        let older = report.warnings.iter().any(|w| {
            matches!(
                w,
                SaveWarning::OlderChunks {
                    dimension: Dimension::Overworld,
                    ..
                }
            )
        });
        if older {
            continue;
        }
        let world = report.open_world(Dimension::Overworld).unwrap();
        let chunks = Arc::new(report.chunks(Dimension::Overworld));
        let seed = report.save.seed;
        let predicted = world.session(seed).unwrap();
        let from_save = world
            .session(seed)
            .unwrap()
            .with_save(Arc::clone(&chunks))
            .unwrap();
        let centre = predicted.spawn_origin().chunk;
        let (min, max) = (
            [centre[0] - 48, centre[1] - 48],
            [centre[0] + 48, centre[1] + 48],
        );
        let held: BTreeSet<[i32; 2]> = chunks
            .generated_in(min, max)
            .into_iter()
            .filter(|c| c.readable)
            .map(|c| c.chunk)
            .collect();
        saves += 1;
        for structure in world.structures().map(str::to_string).collect::<Vec<_>>() {
            if !world.structure_generates(&structure) {
                continue;
            }
            let query = world.structure_query(&structure).unwrap();
            let by_chunk = |starts: Vec<spyglass::StructureStart>| -> BTreeMap<_, _> {
                starts.into_iter().map(|s| (s.chunk, s)).collect()
            };
            let guess = by_chunk(predicted.structures_in(&query, min, max, true).unwrap());
            let saved = by_chunk(from_save.structures_in(&query, min, max, true).unwrap());
            for (chunk, start) in &saved {
                if !start.from_save {
                    continue;
                }
                match guess.get(chunk) {
                    Some(g) if g.certain && g.structures == start.structures => agreed += 1,
                    other => failures.push(format!(
                        "{}: {structure} saved in {chunk:?}, predicted {other:?}",
                        report.save.name
                    )),
                }
            }
            for (chunk, g) in &guess {
                if g.certain && held.contains(chunk) && !saved.contains_key(chunk) {
                    failures.push(format!(
                        "{}: {structure} predicted certain in {chunk:?}, not in the save",
                        report.save.name
                    ));
                }
            }
        }
        println!("{}: checked", report.save.name);
    }
    println!(
        "{saves} saves, {agreed} saved starts agreed, {} disagreements",
        failures.len()
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// In the same saves, every predicted container the save holds unopened has the predicted
/// loot table and seed (ADR 0071), in the 25 × 25 chunks around the spawn origin and around
/// the player. Prints how many were unopened, filled and gone.
#[test]
#[ignore = "reads the saves on this machine"]
fn saved_containers_have_the_predicted_seeds() {
    use std::sync::Arc;

    use spyglass::{Dimension, InSave};

    let bundles = spyglass_oracle::default_dir().join("../bundles");
    let mut failures = Vec::new();
    let (mut unopened, mut filled, mut gone, mut not_placed) = (0, 0, 0, 0);
    for found in spyglass::launcher_saves() {
        let Ok(report) = spyglass::inspect_save(&found.dir, &bundles, Scan::Chunks) else {
            continue;
        };
        let older = report.warnings.iter().any(|w| {
            matches!(
                w,
                SaveWarning::OlderChunks {
                    dimension: Dimension::Overworld,
                    ..
                }
            )
        });
        if !report.answerable_in(Dimension::Overworld) || older {
            continue;
        }
        let world = report.open_world(Dimension::Overworld).unwrap();
        let session = world
            .session(report.save.seed)
            .unwrap()
            .with_save(Arc::new(report.chunks(Dimension::Overworld)))
            .unwrap();
        let mut centres = vec![session.spawn_origin().chunk];
        if let Some(player) = report
            .save
            .player
            .as_ref()
            .filter(|p| p.dimension_kind() == Some(Dimension::Overworld))
        {
            let [x, _, z] = player.block();
            centres.push([x >> 4, z >> 4]);
        }
        for [cx, cz] in centres {
            let containers = session
                .containers_in([cx - 12, cz - 12], [cx + 12, cz + 12])
                .unwrap();
            for c in containers {
                match &c.saved {
                    Some(InSave::Unopened { as_predicted }) => {
                        unopened += 1;
                        if !as_predicted && c.certain {
                            failures.push(format!(
                                "{}: {} at {:?} ({}) isn't as predicted",
                                report.save.name, c.holder, c.position, c.table
                            ));
                        }
                    }
                    Some(InSave::Filled { .. }) => filled += 1,
                    Some(InSave::Gone) => {
                        gone += 1;
                        if c.certain {
                            println!(
                                "  gone, though certain: {} at {:?} ({:?})",
                                c.holder, c.position, c.source
                            );
                        }
                    }
                    Some(_) => {}
                    None => not_placed += 1,
                }
            }
        }
        println!("{}: checked", report.save.name);
    }
    println!(
        "{unopened} unopened, {filled} filled, {gone} gone, {not_placed} not placed in the save; {} disagreements",
        failures.len()
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
