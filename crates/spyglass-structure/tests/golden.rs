//! Structure placement against the oracle's structure golden files (`docs/specs/oracle.md`):
//! placement chunks, ring positions, slime chunks, the biome check at the game's own check
//! positions, jigsaw check positions, and the game's choice among each set's structures.
//!
//! Needs `cargo xtask oracle structures` (or `golden`) and a bundle for the same version. Run with
//! `cargo test --release -p spyglass-structure -- --ignored`.

use std::collections::BTreeSet;
use std::path::PathBuf;

use serde_json::Value;
use spyglass_biome::BiomeSource;
use spyglass_data::bundle::Bundle;
use spyglass_rng::worldgen;
use spyglass_structure::{Check, JigsawCheck, Starts, StructureSets, is_slime_chunk};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn ints(v: &Value) -> Vec<i64> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|n| n.as_i64().unwrap())
        .collect()
}

fn chunk(v: &Value) -> [i32; 2] {
    let c = ints(v);
    [c[0] as i32, c[1] as i32]
}

/// Problems found, printed together at the end.
#[derive(Default)]
struct Report {
    failures: Vec<String>,
    /// Counters for the summary.
    chunks: usize,
    tries: usize,
    stubs: usize,
    tied_stubs: usize,
    ring_flagged: usize,
    /// Flagged mismatches whose alternatives spyglass didn't enumerate (too many ties).
    ring_unknown: usize,
    /// Ring positions flagged as tie- or platform-dependent, matching or not.
    rings_flagged_total: usize,
    rings: usize,
    /// Chunks where we said a start is certain, and how many the game didn't start.
    definite: usize,
    definite_not_started: usize,
    jigsaw_positions: usize,
}

impl Report {
    fn fail(&mut self, msg: String) {
        if self.failures.len() < 200 {
            self.failures.push(msg);
        }
    }
}

#[test]
#[ignore = "needs oracle-out/<version>/structures and bundles/<version>"]
fn structures_match_the_oracle() {
    let version = "26.3";
    let dir = root()
        .join("oracle-out")
        .join(version)
        .join("structures/overworld");
    let files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}; run cargo xtask oracle structures", dir.display()))
        .map(|e| e.unwrap().path())
        .collect();
    assert!(
        !files.is_empty(),
        "no structure golden files in {}",
        dir.display()
    );
    let bundle = Bundle::open(root().join("bundles").join(version)).unwrap();
    let source = BiomeSource::overworld(&bundle).unwrap();
    let sets = StructureSets::overworld(&bundle, &source).unwrap_or_else(|e| panic!("{e}"));
    let mut report = Report::default();

    for path in &files {
        let golden: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let seed = golden["seed"].as_i64().unwrap();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let biomes = source.with_seed(seed).unwrap();
        let placements = sets.with_seed(seed, &biomes);
        let starts = Starts::new(&placements, &biomes);
        let regions: Vec<(String, [i32; 2], [i32; 2])> = golden["regions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| {
                (
                    r["name"].as_str().unwrap().to_string(),
                    chunk(&r["min"]),
                    chunk(&r["max"]),
                )
            })
            .collect();

        // The sets the game generates are exactly the ones we call possible.
        let game_sets: BTreeSet<&str> = golden["sets"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["set"].as_str().unwrap())
            .collect();
        let ours: BTreeSet<&str> = sets
            .sets()
            .iter()
            .filter(|s| s.possible)
            .map(|s| s.id.as_str())
            .collect();
        if game_sets != ours {
            report.fail(format!(
                "{name}: possible sets differ: game {game_sets:?}, ours {ours:?}"
            ));
        }

        // Ring positions, in order.
        for ring in golden["rings"].as_array().unwrap() {
            let set = sets.set_index(ring["set"].as_str().unwrap()).unwrap();
            let game: Vec<[i32; 2]> = ring["chunks"]
                .as_array()
                .unwrap()
                .iter()
                .map(chunk)
                .collect();
            let ours = placements.ring_positions(set).unwrap();
            if ours.len() != game.len() {
                report.fail(format!(
                    "{name}: {} ring count {} vs {}",
                    ring["set"],
                    ours.len(),
                    game.len()
                ));
                continue;
            }
            for (i, (p, g)) in ours.iter().zip(&game).enumerate() {
                report.rings += 1;
                if p.platform_dependent || p.tie_dependent {
                    report.rings_flagged_total += 1;
                }
                if p.chunk != *g {
                    if !(p.platform_dependent || p.tie_dependent) {
                        report.fail(format!("{name}: ring {i}: ours {:?}, game {g:?}", p.chunk));
                    } else if p.alternatives_known && !p.alternatives.contains(g) {
                        report.fail(format!(
                            "{name}: ring {i}: the game's {g:?} isn't ours {:?} or an alternative {:?}",
                            p.chunk, p.alternatives
                        ));
                    } else {
                        report.ring_flagged += 1;
                        report.ring_unknown += usize::from(!p.alternatives_known);
                    }
                }
            }
        }

        for s in golden["sets"].as_array().unwrap() {
            let set_id = s["set"].as_str().unwrap();
            let set = sets.set_index(set_id).unwrap();
            let game_chunks: Vec<&Value> = s["chunks"].as_array().unwrap().iter().collect();

            // Placement chunks, region by region.
            for (region, min, max) in &regions {
                let game: BTreeSet<[i32; 2]> = game_chunks
                    .iter()
                    .map(|c| chunk(&c["chunk"]))
                    .filter(|c| {
                        (min[0]..=max[0]).contains(&c[0]) && (min[1]..=max[1]).contains(&c[1])
                    })
                    .collect();
                let ours: BTreeSet<[i32; 2]> = placements
                    .structure_chunks_in(set, *min, *max)
                    .into_iter()
                    .collect();
                if game != ours {
                    let missing: Vec<_> = game.difference(&ours).take(5).collect();
                    let extra: Vec<_> = ours.difference(&game).take(5).collect();
                    report.fail(format!(
                        "{name}: {set_id} {region}: {} game vs {} ours placement chunks; missing {missing:?}, extra {extra:?}",
                        game.len(),
                        ours.len()
                    ));
                }
            }

            for c in game_chunks {
                report.chunks += 1;
                let at = chunk(&c["chunk"]);
                let tries = c["tries"].as_array().unwrap();
                let selection = starts.select(set, at);
                let our_structures: Vec<usize> = selection.tries.iter().map(|t| t.0).collect();
                if let Some(s) = selection.definite() {
                    report.definite += 1;
                    let started = tries.iter().any(|t| {
                        t["start"].as_bool() == Some(true)
                            && sets.structure_index(t["structure"].as_str().unwrap()) == Some(s)
                    });
                    if !started {
                        report.definite_not_started += 1;
                    }
                }

                for t in tries {
                    report.tries += 1;
                    let id = t["structure"].as_str().unwrap();
                    let structure = sets.structure_index(id).unwrap();
                    let started = t["start"].as_bool().unwrap();
                    let s_def = &sets.structures()[structure];

                    // Soundness: a start the game made is one we allow.
                    if started && !our_structures.contains(&structure) {
                        report.fail(format!(
                            "{name}: {set_id} {at:?}: the game started {id}, which we rejected; ours {:?}",
                            selection.tries
                        ));
                    }
                    let check = starts.check(structure, at);
                    if check == Check::Passes
                        && t.get("valid_biome").and_then(Value::as_bool) == Some(false)
                    {
                        report.fail(format!(
                            "{name}: {set_id} {at:?}: {id}: we pass, the game's biome check fails"
                        ));
                    }

                    let Some(stub) = t.get("stub") else { continue };
                    let pos = ints(stub);
                    let (x, y, z) = (pos[0] as i32, pos[1] as i32, pos[2] as i32);

                    // The biome check at the game's own check position.
                    report.stubs += 1;
                    let lookup = biomes.biome_at(x >> 2, y >> 2, z >> 2);
                    let game_biome = t["biome"].as_str().unwrap();
                    let candidates: Vec<&str> = lookup
                        .candidates()
                        .iter()
                        .map(|&b| source.name(b))
                        .collect();
                    if lookup.is_tied() {
                        report.tied_stubs += 1;
                    }
                    if !candidates.contains(&game_biome) {
                        report.fail(format!(
                            "{name}: {set_id} {at:?}: {id} at {pos:?}: game biome {game_biome}, ours {candidates:?}"
                        ));
                    }
                    let game_valid = t["valid_biome"].as_bool().unwrap();
                    let ours = starts.check_at(structure, [x, y, z]);
                    let agrees = match ours {
                        Check::Passes => game_valid,
                        Check::Rejected => !game_valid,
                        Check::Candidate => lookup.is_tied(),
                    };
                    if !agrees {
                        report.fail(format!(
                            "{name}: {set_id} {at:?}: {id} at {pos:?}: biome check ours {ours:?}, game {game_valid}"
                        ));
                    }

                    // Jigsaw check positions: exactly, or x and z where projected.
                    if let Some(jigsaw) = &s_def.jigsaw {
                        let mut random = worldgen::large_feature(seed, at[0], at[1]);
                        report.jigsaw_positions += 1;
                        match jigsaw.check_position(at, sets.heights(), &mut random) {
                            JigsawCheck::At(p) if p == [x, y, z] => {}
                            JigsawCheck::Projected { x: px, z: pz, .. } if [px, pz] == [x, z] => {}
                            other => report.fail(format!(
                                "{name}: {set_id} {at:?}: {id}: game stub {pos:?}, ours {other:?}"
                            )),
                        }
                    }
                }
            }
        }

        // Slime chunks.
        for s in golden["slime"].as_array().unwrap() {
            let region = s["region"].as_str().unwrap();
            let (_, min, max) = regions.iter().find(|r| r.0 == region).unwrap();
            let game: BTreeSet<[i32; 2]> =
                s["chunks"].as_array().unwrap().iter().map(chunk).collect();
            let ours: BTreeSet<[i32; 2]> = (min[1]..=max[1])
                .flat_map(|z| (min[0]..=max[0]).map(move |x| [x, z]))
                .filter(|&[x, z]| is_slime_chunk(seed, x, z))
                .collect();
            if game != ours {
                report.fail(format!(
                    "{name}: slime chunks differ in {region}: {} vs {}",
                    game.len(),
                    ours.len()
                ));
            }
        }
    }

    println!(
        "{} files: {} placement chunks, {} tries, {} check positions ({} tied), {} jigsaw positions; {} rings, {} flagged, {} flagged mismatches ({} with unknown alternatives); {} definite starts, {} not started by the game",
        files.len(),
        report.chunks,
        report.tries,
        report.stubs,
        report.tied_stubs,
        report.jigsaw_positions,
        report.rings,
        report.rings_flagged_total,
        report.ring_flagged,
        report.ring_unknown,
        report.definite,
        report.definite_not_started
    );
    if !report.failures.is_empty() {
        panic!(
            "{} problems:\n{}",
            report.failures.len(),
            report.failures.join("\n")
        );
    }
}
