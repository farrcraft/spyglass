//! Compiled programs against the bound functions they're compiled from (ADR 0031): the same bits
//! everywhere. Needs a bundle; run with `cargo test --release -p spyglass-density -- --ignored`.

use spyglass_data::bundle::Bundle;
use spyglass_density::v1_21::{self, NoiseChunk, RandomState, RouterEntry};

/// The bundle for the version under test (`spyglass_oracle::version`).
fn bundle() -> Bundle {
    let version = spyglass_oracle::version();
    Bundle::open(spyglass_oracle::bundle_dir())
        .unwrap_or_else(|e| panic!("{e}; run spyglass-extract {version}"))
}

/// Each dimension's noise settings and level: minimum y and height.
const DIMENSIONS: [(&str, i32, i32); 3] = [
    ("minecraft:overworld", -64, 384),
    ("minecraft:nether", 0, 256),
    ("minecraft:end", 0, 256),
];

const SEEDS: [i64; 4] = [0, -7, 2_514_887_864_703_934_023, i64::MIN];

/// Chunks near the origin, far out, and at the world border.
const CHUNKS: [(i32, i32); 5] = [
    (0, 0),
    (-3, 5),
    (62_500, -62_500),
    (1_874_999, 1_874_999),
    (-1_875_000, 4),
];

/// Asserts `a` and `b` are the same bits, naming what was compared.
fn same(a: f64, b: f64, what: impl Fn() -> String) {
    assert_eq!(a.to_bits(), b.to_bits(), "{}: {a} against {b}", what());
}

/// 26.2: a chunk's compiled program against its bound functions, for every router entry at
/// single points, every entry positioned in its cells, the full density through the cells, the
/// climate and the preliminary surface level.
#[test]
#[ignore = "needs bundles/<version> (SPYGLASS_ORACLE_VERSION, else the newest release)"]
fn the_program_matches_the_functions() {
    let bundle = bundle();
    let overworld = bundle
        .resource("worldgen/noise_settings", "overworld")
        .unwrap();
    if !v1_21::is_v1_21(&overworld) {
        println!("not a 26.2 bundle: 26.3's family has no program yet");
        return;
    }
    let mut compared = 0_u64;
    for (settings, level_min_y, level_height) in DIMENSIONS {
        let router = v1_21::NoiseRouter::load(&bundle, settings).unwrap();
        for seed in SEEDS {
            let state: RandomState = router.bind(seed).unwrap();
            for (cx, cz) in CHUNKS {
                let program = NoiseChunk::for_chunk(&state, cx, cz, level_min_y, level_height);
                let reference =
                    NoiseChunk::for_chunk(&state, cx, cz, level_min_y, level_height).reference();
                let (min_y, height) = (program.min_y(), program.height());
                let (x0, z0) = (cx.wrapping_mul(16), cz.wrapping_mul(16));
                let at = |x, y, z| format!("{settings} seed {seed} block ({x}, {y}, {z})");
                for dz in (0..16).step_by(5) {
                    for dx in (0..16).step_by(3) {
                        let (x, z) = (x0.wrapping_add(dx), z0.wrapping_add(dz));
                        same(
                            f64::from(program.preliminary_surface_level(x, z)),
                            f64::from(reference.preliminary_surface_level(x, z)),
                            || format!("surface level, {}", at(x, 0, z)),
                        );
                        for y in (min_y..min_y + height).step_by(7) {
                            same(
                                program.interpolated_density(x, y, z),
                                reference.interpolated_density(x, y, z),
                                || format!("density, {}", at(x, y, z)),
                            );
                            for entry in RouterEntry::ALL {
                                same(
                                    program.compute(entry, x, y, z),
                                    reference.compute(entry, x, y, z),
                                    || format!("{} at a point, {}", entry.name(), at(x, y, z)),
                                );
                                same(
                                    program.compute_positioned(entry, x, y, z),
                                    reference.compute_positioned(entry, x, y, z),
                                    || format!("{} positioned, {}", entry.name(), at(x, y, z)),
                                );
                                compared += 2;
                            }
                            compared += 1;
                        }
                    }
                }
            }
        }
    }
    println!("{compared} values the same");
}
