//! Heights and block columns against the oracle's height golden files (`docs/specs/oracle.md`):
//! every `getBaseHeight` in every region, both heightmaps, and every sampled `getBaseColumn`.
//!
//! Needs `cargo xtask oracle heights` (or `golden`) and a bundle for the same version. Run with
//! `cargo test --release -p spyglass-terrain --test heights -- --ignored`.

use std::fs;
use std::path::{Path, PathBuf};

use rayon::prelude::*;
use serde_json::Value;
use spyglass_data::bundle::Bundle;
use spyglass_terrain::{Block, Heightmap, Terrain, TerrainSettings};

/// The height files for the version under test, in `oracle-out/<version>/heights/<dimension>/`.
fn height_files() -> Vec<PathBuf> {
    let dir = spyglass_oracle::out_dir().join("heights");
    let mut out: Vec<PathBuf> = ["overworld", "the_nether", "the_end"]
        .into_iter()
        .flat_map(|d| fs::read_dir(dir.join(d)))
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    out.sort();
    out
}

fn int(v: &Value) -> i32 {
    v.as_i64().expect("an integer") as i32
}

fn block_id(block: Block, default_block: &str) -> &str {
    match block {
        Block::Default => default_block,
        Block::Air => "minecraft:air",
        Block::Water => "minecraft:water",
        Block::Lava => "minecraft:lava",
    }
}

/// Compares one file, returning its mismatches and how many values it checked.
fn check(path: &Path) -> (Vec<String>, usize) {
    let file: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    let version = file["version"].as_str().unwrap();
    let seed = file["seed"].as_i64().unwrap();
    assert_eq!(version, spyglass_oracle::version(), "{}", path.display());
    let bundle = Bundle::open(spyglass_oracle::bundle_dir())
        .unwrap_or_else(|e| panic!("{e}; run spyglass-extract {version}"));
    // The Default preset's noise settings for the file's dimension; its type has the
    // dimension's id.
    let dimension = file["dimension"].as_str().unwrap();
    let noise_settings = match dimension {
        "minecraft:overworld" => "minecraft:overworld",
        "minecraft:the_nether" => "minecraft:nether",
        "minecraft:the_end" => "minecraft:end",
        other => panic!("unknown dimension {other}"),
    };
    let settings = TerrainSettings::load(&bundle, noise_settings, dimension).unwrap();
    assert_eq!(file["min_y"], settings.level_min_y);
    assert_eq!(file["height"], settings.level_height);
    let terrain = Terrain::new(&settings, seed).unwrap();
    let mut failures = Vec::new();
    let mut checked = 0;
    for region in file["regions"].as_array().unwrap() {
        let name = region["name"].as_str().unwrap();
        let (x0, z0) = (int(&region["min"][0]), int(&region["min"][1]));
        let size = int(&region["size"]);
        for heightmap in Heightmap::ALL {
            let want: Vec<i32> = region[heightmap.key()]
                .as_array()
                .unwrap()
                .iter()
                .map(int)
                .collect();
            assert_eq!(want.len(), (size * size) as usize);
            // z outer, x inner, as the file lists them: every column chunk by chunk, in rows of
            // 16 so rows run in parallel, and every 13th through its own column as well.
            let got: Vec<i32> = (0..size)
                .step_by(16)
                .collect::<Vec<_>>()
                .into_par_iter()
                .flat_map_iter(|dz| {
                    let rows = 16.min(size - dz);
                    terrain.base_heights([x0, z0 + dz], [size, rows], heightmap)
                })
                .collect();
            let alone: Vec<(usize, i32)> = (0..want.len())
                .step_by(13)
                .collect::<Vec<_>>()
                .into_par_iter()
                .map(|i| {
                    let (x, z) = (x0 + i as i32 % size, z0 + i as i32 / size);
                    (i, terrain.base_height(x, z, heightmap))
                })
                .collect();
            // A sparse grid, as maps sample terrain: every 5th column from the second.
            let xs: Vec<i32> = (x0 + 1..x0 + size).step_by(5).collect();
            let zs: Vec<i32> = (z0 + 1..z0 + size).step_by(5).collect();
            let grid = terrain.base_heights_on(&xs, &zs, heightmap);
            let sparse = grid.iter().enumerate().map(|(i, &h)| {
                let (x, z) = (xs[i % xs.len()], zs[i / xs.len()]);
                (((z - z0) * size + (x - x0)) as usize, h)
            });
            checked += want.len() + alone.len() + grid.len();
            let found = got.iter().copied().enumerate();
            for (i, g) in found.chain(alone).chain(sparse) {
                let w = want[i];
                if w != g {
                    let (x, z) = (x0 + i as i32 % size, z0 + i as i32 / size);
                    failures.push(format!(
                        "seed {seed} {name} {} at ({x}, {z}): game {w}, spyglass {g}",
                        heightmap.key()
                    ));
                }
            }
        }
    }
    for column in file["columns"].as_array().unwrap() {
        let (x, z) = (int(&column["pos"][0]), int(&column["pos"][1]));
        let mut want = Vec::new();
        for run in column["blocks"].as_array().unwrap() {
            let id = run[0].as_str().unwrap();
            want.extend(std::iter::repeat_n(id, int(&run[1]) as usize));
        }
        let blocks = terrain.base_column(x, z);
        let min_y = int(&file["min_y"]);
        let got: Vec<&str> = (0..want.len() as i32)
            .map(|i| block_id(blocks.at(min_y + i), &settings.default_block))
            .collect();
        checked += want.len();
        if let Some(i) = want.iter().zip(&got).position(|(w, g)| w != g) {
            failures.push(format!(
                "seed {seed} column ({x}, {z}) at y {}: game {}, spyglass {}",
                min_y + i as i32,
                want[i],
                got[i]
            ));
        }
    }
    (failures, checked)
}

#[test]
#[ignore = "needs the oracle's height golden files and a bundle: cargo xtask oracle heights"]
fn every_height_matches_the_game() {
    let files = height_files();
    assert!(
        !files.is_empty(),
        "no height files for {}; run cargo xtask oracle heights",
        spyglass_oracle::version()
    );
    let mut failures = Vec::new();
    let mut checked = 0;
    for path in &files {
        let (f, n) = check(path);
        println!("{}: {} values, {} differ", path.display(), n, f.len());
        failures.extend(f);
        checked += n;
    }
    assert!(
        failures.is_empty(),
        "{} of {checked} values differ:\n{}",
        failures.len(),
        failures[..failures.len().min(30)].join("\n")
    );
    println!("all {checked} values in {} files match", files.len());
}
