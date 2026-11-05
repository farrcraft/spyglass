//! Every container loot table of a real bundle loads and fills. Ignored: needs a bundle
//! (`cargo run -p spyglass-extract`); `cargo xtask oracle-test` runs it for every version.

use spyglass_data::bundle::Bundle;
use spyglass_loot::{Loot, MapRequest, World};

/// The loot table directories whose tables fill containers.
const CONTAINER_TABLES: &[&str] = &["chests/", "barrels/", "pots/", "dispensers/"];

/// A world where every block is in one biome and no map finds its structure.
struct Plains;

impl World for Plains {
    fn biome(&self, _x: i32, _y: i32, _z: i32) -> Option<String> {
        Some("minecraft:plains".into())
    }

    fn map_target(&self, _request: &MapRequest) -> Option<[i32; 2]> {
        None
    }
}

#[test]
#[ignore = "needs a bundle: spyglass-extract"]
fn every_container_table_loads_and_fills() {
    let bundle = Bundle::open(spyglass_oracle::bundle_dir()).unwrap();
    let prefix = "data/minecraft/loot_table/";
    let tables: Vec<String> = bundle
        .manifest()
        .files
        .keys()
        .filter_map(|path| path.strip_prefix(prefix)?.strip_suffix(".json"))
        .filter(|id| CONTAINER_TABLES.iter().any(|dir| id.starts_with(dir)))
        .map(|id| format!("minecraft:{id}"))
        .collect();
    assert!(tables.len() >= 60, "only {} container tables", tables.len());

    let mut loot = Loot::new();
    for table in &tables {
        loot.load(&bundle, table)
            .unwrap_or_else(|e| panic!("{table}: {e}"));
    }
    let mut stacks = 0;
    for table in &tables {
        for seed in 1..=1_000 {
            let contents = loot.fill(table, seed, [0, 64, 0], 27, &Plains).unwrap();
            assert_eq!(contents.len(), 27);
            for stack in contents.iter().flatten() {
                assert!(stack.count >= 1, "{table} {seed}: {stack:?}");
                stacks += 1;
            }
        }
    }
    assert!(stacks > 0);
}
