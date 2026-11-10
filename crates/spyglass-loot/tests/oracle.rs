//! Every container table's fills against the game's: the oracle's loot golden files
//! (docs/specs/oracle.md), slot for slot. Ignored: needs them (`cargo xtask oracle loot`) and a
//! bundle; `cargo xtask oracle-test` runs it for every version that has both.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::Value;
use spyglass_data::bundle::Bundle;
use spyglass_loot::{Loot, MapRequest, World};
use spyglass_oracle::stacks;

/// The fill's one biome, everywhere, and no structure for any map, as the oracle's level gives.
struct Oracle {
    biome: String,
}

impl World for Oracle {
    fn biome(&self, _x: i32, _y: i32, _z: i32) -> Option<String> {
        Some(self.biome.clone())
    }

    fn map_target(&self, _request: &MapRequest) -> Option<[i32; 2]> {
        None
    }
}

#[test]
#[ignore = "needs the oracle's loot golden files and a bundle: cargo xtask oracle loot"]
fn every_fill_matches_the_game() {
    let dir = spyglass_oracle::out_dir().join("loot");
    let mut files = Vec::new();
    collect(&dir, &mut files);
    assert!(
        !files.is_empty(),
        "no loot golden files in {}",
        dir.display()
    );
    let bundle = Bundle::open(spyglass_oracle::bundle_dir()).unwrap();
    let mut loot = Loot::new();
    let (mut fills, mut stacks, mut wrong) = (0, 0, Vec::new());
    for file in &files {
        let golden: Value = serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
        let table = golden["table"].as_str().unwrap();
        let slots = golden["slots"].as_u64().unwrap() as usize;
        let at: Vec<i32> = serde_json::from_value(golden["at"].clone()).unwrap();
        loot.load(&bundle, table)
            .unwrap_or_else(|e| panic!("{table}: {e}"));
        for fill in golden["fills"].as_array().unwrap() {
            let seed = fill["seed"].as_i64().unwrap();
            let world = Oracle {
                biome: fill["biome"].as_str().unwrap().into(),
            };
            let ours = loot
                .fill(table, seed, [at[0], at[1], at[2]], slots, &world)
                .unwrap();
            let mut theirs: BTreeMap<usize, &Value> = BTreeMap::new();
            for slot in fill["slots"].as_array().unwrap() {
                theirs.insert(slot["slot"].as_u64().unwrap() as usize, slot);
            }
            for (slot, ours) in ours.iter().enumerate() {
                let want = theirs.get(&slot).map(|v| stacks::game(v));
                let got = ours.as_ref().map(|s| stacks::ours(s, &bundle));
                if want != got {
                    wrong.push(format!(
                        "{table} seed {seed} slot {slot}: game {want:?}, spyglass {got:?}"
                    ));
                }
                stacks += usize::from(got.is_some());
            }
            fills += 1;
        }
    }
    eprintln!(
        "{} tables, {fills} fills, {stacks} stacks, {} differences",
        files.len(),
        wrong.len()
    );
    assert!(
        wrong.is_empty(),
        "{}",
        wrong
            .iter()
            .take(20)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

fn collect(dir: &Path, files: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, files);
        } else if path.extension().is_some_and(|e| e == "json") {
            files.push(path);
        }
    }
    files.sort();
}
