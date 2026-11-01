//! The block golden files' chunks without carvers (`docs/specs/oracle.md#block-golden-files`)
//! against spyglass's columns: wherever a column has air, water or lava, the game's chunk has
//! the same, and wherever it's solid, so is the chunk. The material rule mostly decides only which
//! solid block a position gets, so this checks the harness's stand-ins (ADR 0049) and the noise
//! fill of whole chunks, column by column. It allows what the material rule does beyond that
//! ([`material_rule_may`]).
//!
//! Needs `cargo xtask oracle blocks` and a bundle for the same version. Run with
//! `cargo test --release -p spyglass-terrain --test blocks -- --ignored`.

use std::path::Path;

use rayon::prelude::*;
use spyglass_data::bundle::Bundle;
use spyglass_oracle::blocks::BlockFile;
use spyglass_terrain::{Block, Terrain, TerrainSettings};

/// What a position is, as far as the column decides it. 26.2's Nether carver carves to cave air.
fn kind(state: &str) -> &str {
    match state {
        "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air" => "minecraft:air",
        "minecraft:water" | "minecraft:lava" => state,
        _ => "solid",
    }
}

/// Whether the material rule may turn what the column has into what the game has, where their
/// kinds differ: icebergs and eroded badlands pillars put solid blocks over air and water, and
/// the Nether's floor holes below its lava sea are lava.
fn material_rule_may(column: &str, game: &str, nether: bool, y: i32) -> bool {
    match (column, kind(game)) {
        ("minecraft:air" | "minecraft:water", "solid") => true,
        ("solid", "minecraft:lava") => nether && y < 32,
        _ => false,
    }
}

fn column_kind(block: Block) -> &'static str {
    match block {
        Block::Air => "minecraft:air",
        Block::Water => "minecraft:water",
        Block::Lava => "minecraft:lava",
        Block::Default | Block::Vein(_) => "solid",
    }
}

/// Compares one file, returning its mismatches (one per chunk at most) and how many blocks it
/// checked.
fn check(path: &Path) -> (Vec<String>, usize) {
    let file = BlockFile::read(path).unwrap();
    let bundle = Bundle::open(spyglass_oracle::bundle_dir())
        .unwrap_or_else(|e| panic!("{e}; run spyglass-extract {}", file.version));
    let noise_settings = match file.dimension.as_str() {
        "minecraft:overworld" => "minecraft:overworld",
        "minecraft:the_nether" => "minecraft:nether",
        "minecraft:the_end" => "minecraft:end",
        other => panic!("unknown dimension {other}"),
    };
    let settings = TerrainSettings::load(&bundle, noise_settings, &file.dimension).unwrap();
    assert_eq!(file.min_y, settings.level_min_y);
    let terrain = Terrain::new(&settings, file.seed).unwrap();
    let nether = file.dimension == "minecraft:the_nether";
    let mut failures = Vec::new();
    let mut checked = 0;
    for chunk in &file.chunks {
        let [cx, cz] = chunk.chunk;
        let columns: Vec<_> = (0..256)
            .map(|i| terrain.base_column(cx * 16 + i % 16, cz * 16 + i / 16))
            .collect();
        let diff = chunk.first_difference(|[x, y, z]| {
            let column = &columns[((z & 15) * 16 + (x & 15)) as usize];
            let want = chunk.block([x, y, z]);
            // Compare kinds: report the game's state where they agree, or the rule explains it.
            let got = column_kind(column.at(y));
            if got == kind(want) || material_rule_may(got, want, nether, y) {
                want.to_string()
            } else {
                got.to_string()
            }
        });
        checked += 256 * file.height as usize;
        if let Some(diff) = diff {
            failures.push(format!(
                "seed {} {} chunk {cx}, {cz} {diff}",
                file.seed, file.dimension
            ));
        }
    }
    (failures, checked)
}

#[test]
#[ignore = "needs the oracle's block golden files and a bundle: cargo xtask oracle blocks"]
fn chunks_without_carvers_have_the_columns_air_and_fluids() {
    let files = spyglass_oracle::blocks::files(false);
    assert!(
        !files.is_empty(),
        "no block files for {}; run cargo xtask oracle blocks",
        spyglass_oracle::version()
    );
    let results: Vec<_> = files.par_iter().map(|p| check(p)).collect();
    let checked: usize = results.iter().map(|(_, n)| n).sum();
    let failures: Vec<&String> = results.iter().flat_map(|(f, _)| f).collect();
    println!(
        "{} files, {checked} blocks, {} chunks differ",
        files.len(),
        failures.len()
    );
    assert!(
        failures.is_empty(),
        "{} chunks differ:\n{}",
        failures.len(),
        failures
            .iter()
            .take(30)
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
#[ignore = "needs the oracle's block golden files: cargo xtask oracle blocks"]
fn carvers_only_carve() {
    let with = spyglass_oracle::blocks::files(true);
    let without = spyglass_oracle::blocks::files(false);
    assert!(
        !with.is_empty() && with.len() == without.len(),
        "block files for {} with and without carvers; run cargo xtask oracle blocks",
        spyglass_oracle::version()
    );
    let results: Vec<(usize, Vec<String>)> = with
        .par_iter()
        .zip(&without)
        .map(|(a, b)| {
            let (carved, uncarved) = (BlockFile::read(a).unwrap(), BlockFile::read(b).unwrap());
            assert_eq!(
                (carved.seed, &carved.dimension),
                (uncarved.seed, &uncarved.dimension)
            );
            let mut changed = 0;
            let mut failures = Vec::new();
            for (c, u) in carved.chunks.iter().zip(&uncarved.chunks) {
                assert_eq!(c.chunk, u.chunk);
                for ((pos, after), (_, before)) in c.blocks().zip(u.blocks()) {
                    if after == before {
                        continue;
                    }
                    changed += 1;
                    // Carving opens solid blocks, and fixes the surface next to what it opens
                    // (dirt becomes grass): only solid blocks change.
                    if kind(before) != "solid" {
                        failures.push(format!(
                            "seed {} {} at {pos:?}: {before} became {after}",
                            carved.seed, carved.dimension
                        ));
                    }
                }
            }
            (changed, failures)
        })
        .collect();
    let changed: usize = results.iter().map(|(n, _)| n).sum();
    let failures: Vec<&String> = results.iter().flat_map(|(_, f)| f).collect();
    println!("{} files, {changed} blocks carved", with.len());
    assert!(changed > 0, "the carvers carved nothing");
    assert!(
        failures.is_empty(),
        "{} changes aren't carving:\n{}",
        failures.len(),
        failures
            .iter()
            .take(30)
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    );
}
