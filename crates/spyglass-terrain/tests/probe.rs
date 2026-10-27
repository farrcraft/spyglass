//! The aquifer against the oracle's probe files, block for block: 26.3's `aquifer_column`
//! cases, and 26.2's `noise_chunk_aquifer_column`, asked through the column's `NoiseChunk`.
//!
//! Needs `cargo xtask oracle probe` and a bundle for the same version. Run with
//! `cargo test --release -p spyglass-terrain -- --ignored`.

use std::collections::HashMap;

use spyglass_data::bundle::Bundle;
use spyglass_density::Volume;
use spyglass_oracle::probe::{ProbeFile, probe_file};
use spyglass_terrain::{Aquifer, Block, BoundTerrain, Terrain, TerrainSettings};

/// The bundle for the version under test (`spyglass_oracle::version`).
fn bundle() -> Bundle {
    let version = spyglass_oracle::version();
    Bundle::open(spyglass_oracle::bundle_dir())
        .unwrap_or_else(|e| panic!("{e}; run spyglass-extract {version}"))
}

/// The harness's letters: `s` for the default block, else air, water or lava.
fn letter(block: Block) -> char {
    match block {
        Block::Default => 's',
        Block::Air => 'a',
        Block::Water => 'w',
        Block::Lava => 'l',
        Block::Vein(v) => panic!("the aquifer gave a vein block, {v:?}"),
    }
}

/// The first difference between the game's column and spyglass's, or `None`.
fn difference(
    label: &str,
    density: Option<f64>,
    min_y: i32,
    want: &str,
    got: &str,
) -> Option<String> {
    let i = want.chars().zip(got.chars()).position(|(w, g)| w != g)?;
    Some(format!(
        "{label} density {density:?}: first difference at y {}\n    game:     {}\n    spyglass: {}",
        min_y + i as i32,
        &want[i.saturating_sub(8)..(i + 24).min(want.len())],
        &got[i.saturating_sub(8)..(i + 24).min(got.len())],
    ))
}

#[test]
#[ignore = "needs the oracle's probe files and a bundle: cargo xtask oracle probe"]
fn every_aquifer_block_matches_the_game() {
    let path = probe_file("terrain.json");
    let file = ProbeFile::read(&path).unwrap();
    let bundle = bundle();
    let settings =
        TerrainSettings::load(&bundle, "minecraft:overworld", "minecraft:overworld").unwrap();
    let mut bound: HashMap<i64, BoundTerrain> = HashMap::new();
    let mut terrains: HashMap<i64, Terrain> = HashMap::new();
    let mut failures = Vec::new();
    for case in &file.cases {
        if case.test == "noise_chunk_aquifer_column" {
            // 26.2: the column's own NoiseChunk and aquifer, as iterateNoiseColumn makes them.
            let seed = case.seed.expect("a seed");
            let [x, min_y, z] = case.pos.expect("a position");
            let terrain = terrains
                .entry(seed)
                .or_insert_with(|| Terrain::new(&settings, seed).unwrap());
            let density = match case.method.as_deref() {
                Some("final_density") => None,
                Some("fixed") => Some(case.density_f64().expect("a density")),
                other => panic!("{}: unknown method {other:?}", case.label()),
            };
            let column = terrain.aquifer_column(x, z, density);
            assert_eq!(column.min_y, min_y, "{}", case.label());
            assert_eq!(column.blocks.len(), case.height.unwrap() as usize);
            let got: String = column.blocks.iter().map(|&b| letter(b)).collect();
            failures.extend(difference(&case.label(), density, min_y, case.text(), &got));
            continue;
        }
        assert_eq!(case.test, "aquifer_column", "{}", case.label());
        let seed = case.seed.expect("a seed");
        let [x, min_y, z] = case.pos.expect("a position");
        let height = case.height.expect("a height");
        let terrain = bound
            .entry(seed)
            .or_insert_with(|| settings.bind(seed).unwrap());
        let samplers = terrain
            .aquifer
            .as_ref()
            .expect("the overworld has aquifers");
        let column = Volume::blocks([1, height, 1], [x, min_y, z]);
        let mut aquifer = Aquifer::new(
            samplers,
            terrain.aquifer_random,
            &column,
            terrain.fluid_picker,
        );
        let densities: Vec<f64> = match case.method.as_deref() {
            Some("final_density") => terrain
                .final_density
                .sample_volume(&column)
                .into_iter()
                .map(f64::from)
                .collect(),
            Some("fixed") => vec![case.density_f64().expect("a density"); height as usize],
            other => panic!("{}: unknown method {other:?}", case.label()),
        };
        // From the top down, as the column asks.
        let mut got = vec![' '; height as usize];
        for i in (0..height as usize).rev() {
            let y = min_y + i as i32;
            got[i] = letter(aquifer.substance(x, y, z, densities[i]));
        }
        let got: String = got.into_iter().collect();
        let density = case.density_f64();
        failures.extend(difference(&case.label(), density, min_y, case.text(), &got));
    }
    assert!(
        failures.is_empty(),
        "{}: {} of {} cases differ:\n{}",
        path.display(),
        failures.len(),
        file.cases.len(),
        failures[..failures.len().min(20)].join("\n")
    );
    println!("{}: all {} cases match", path.display(), file.cases.len());
}

/// The aquifer samples surface levels over its region as a volume and elsewhere at points, so
/// it only answers the same for every region if the two agree.
#[test]
#[ignore = "needs bundles/<version> (SPYGLASS_ORACLE_VERSION, else the newest release)"]
fn surface_levels_over_a_volume_are_their_points() {
    let bundle = bundle();
    let settings =
        TerrainSettings::load(&bundle, "minecraft:overworld", "minecraft:overworld").unwrap();
    if settings.router_v1_21().is_some() {
        // 26.2 has no surface volume: its surface levels are points.
        return;
    }
    for seed in [0, 1, -4_893_098_694_207_956_070] {
        let terrain = settings.bind(seed).unwrap();
        let surface = &terrain.aquifer.as_ref().unwrap().surface_level;
        for origin in [[0, 0], [-4096, 1024], [1_000_000, -2_000_000]] {
            let volume = Volume::new([24, 1, 24], [origin[0], 0, origin[1]], [4, 1, 4]);
            let values = surface.sample_volume(&volume);
            for iz in 0..24 {
                for ix in 0..24 {
                    let (x, z) = (volume.block_x(ix), volume.block_z(iz));
                    assert_eq!(
                        values[volume.index(ix, 0, iz)].to_bits(),
                        surface.sample(x, 0, z).to_bits(),
                        "seed {seed} at ({x}, {z})"
                    );
                }
            }
        }
    }
}
