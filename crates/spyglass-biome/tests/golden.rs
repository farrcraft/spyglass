//! Biome lookup against the oracle's golden files, cell by cell, by the tie rule in
//! `docs/specs/oracle.md`, and the search tree against brute force on the probed climates.
//!
//! Needs `cargo xtask oracle golden`, `cargo xtask oracle probe` and a bundle for the same
//! version. Run with `cargo test --release -p spyglass-biome -- --ignored`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use spyglass_biome::{BiomeSource, Lookup};
use spyglass_data::bundle::Bundle;
use spyglass_oracle::probe::{ProbeFile, probe_files};
use spyglass_oracle::{BiomeGrid, Cell, biome_files};

fn bundle_for(version: &str) -> Bundle {
    let dir: PathBuf = spyglass_oracle::default_dir()
        .join("../bundles")
        .join(version);
    Bundle::open(&dir).unwrap_or_else(|e| panic!("{e}; run spyglass-extract {version}"))
}

/// The Default preset's dimensions, by their folders in `oracle-out/<version>/biomes/`.
const DIMENSIONS: [&str; 3] = ["overworld", "the_nether", "the_end"];

/// Golden files by version and dimension folder, read from
/// `oracle-out/<version>/biomes/<dimension>/`.
fn golden_files() -> BTreeMap<(String, &'static str), Vec<PathBuf>> {
    let root = spyglass_oracle::default_dir();
    let mut out = BTreeMap::new();
    for entry in std::fs::read_dir(&root)
        .expect("no oracle-out/; run cargo xtask oracle golden")
        .flatten()
    {
        let version = entry.file_name().to_string_lossy().into_owned();
        for dimension in DIMENSIONS {
            let files = biome_files(&root, &version, dimension);
            if !files.is_empty() {
                out.insert((version.clone(), dimension), files);
            }
        }
    }
    assert!(
        !out.is_empty(),
        "no golden files; run cargo xtask oracle golden"
    );
    out
}

/// What one file's comparison found.
#[derive(Default)]
struct Outcome {
    cells: usize,
    ties: usize,
    failures: Vec<String>,
    failure_count: usize,
}

/// How many failures to describe per file.
const SHOWN: usize = 10;

fn check_file(source: &BiomeSource, path: &Path) -> Outcome {
    let grid = BiomeGrid::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let header = &grid.header;
    let biomes = source.with_seed(header.seed).unwrap();
    let region = biomes.biomes_in(header.min, header.size);
    let names = |l: &Lookup| -> Vec<&str> {
        let mut v: Vec<&str> = l.candidates().iter().map(|b| source.name(*b)).collect();
        v.sort_unstable();
        v
    };
    let mut outcome = Outcome::default();
    for (i, ([qx, qy, qz], cell)) in grid.cells().enumerate() {
        outcome.cells += 1;
        let got = region.get(qx, qy, qz).expect("the region covers the grid");
        outcome.ties += usize::from(got.is_tied());
        let want: Vec<&str> = match &cell {
            Cell::Unique(b) => vec![b],
            Cell::Tied(set) => set.clone(),
        };
        // The rule: the answer is in the golden set. Stricter: the tie set is the same too.
        let answer = source.name(got.biome());
        let mut problem = None;
        if !cell.accepts(answer) {
            problem = Some("outside the rule");
        } else if names(&got) != want {
            problem = Some("tie set differs");
        } else if i.is_multiple_of(97) && biomes.biome_at(qx, qy, qz) != got {
            // Spot check: the point path agrees with the region fill.
            problem = Some("biome_at differs from biomes_in");
        }
        if let Some(problem) = problem {
            outcome.failure_count += 1;
            if outcome.failures.len() < SHOWN {
                outcome.failures.push(format!(
                    "seed {} quart ({qx}, {qy}, {qz}): {problem}\n    target:   {:?}\n    game:     {want:?}\n    spyglass: {:?}",
                    header.seed,
                    biomes.climate_at(qx, qy, qz).0,
                    names(&got),
                ));
            }
        }
    }
    if outcome.ties != grid.tie_count() {
        outcome.failure_count += 1;
        outcome.failures.push(format!(
            "{}: {} tied cells, but the file has {}",
            path.display(),
            outcome.ties,
            grid.tie_count()
        ));
    }
    outcome
}

#[test]
#[ignore = "needs golden files and a bundle: cargo xtask oracle golden"]
fn every_golden_cell_matches_the_game() {
    for ((version, dimension), files) in golden_files() {
        let start = Instant::now();
        let source = BiomeSource::load(
            &bundle_for(&version),
            "minecraft:normal",
            &format!("minecraft:{dimension}"),
        )
        .unwrap();
        // One thread per seed directory.
        let mut by_seed: BTreeMap<PathBuf, Vec<PathBuf>> = BTreeMap::new();
        for f in files {
            by_seed
                .entry(f.parent().unwrap().to_path_buf())
                .or_default()
                .push(f);
        }
        let results: Vec<(PathBuf, Outcome)> = std::thread::scope(|scope| {
            let handles: Vec<_> = by_seed
                .values()
                .map(|files| {
                    let source = &source;
                    scope.spawn(move || {
                        files
                            .iter()
                            .map(|f| (f.clone(), check_file(source, f)))
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            handles
                .into_iter()
                .flat_map(|h| h.join().unwrap())
                .collect()
        });
        let (mut cells, mut ties, mut failed) = (0, 0, 0);
        let mut report = Vec::new();
        for (path, outcome) in &results {
            cells += outcome.cells;
            ties += outcome.ties;
            failed += outcome.failure_count;
            if outcome.failure_count > 0 {
                report.push(format!(
                    "{}: {} failures\n{}",
                    path.display(),
                    outcome.failure_count,
                    outcome.failures.join("\n")
                ));
            }
        }
        assert!(
            failed == 0,
            "{version} {dimension}: {failed} of {cells} cells fail:\n{}",
            report.join("\n")
        );
        println!(
            "{version} {dimension}: {} files, {cells} cells, {ties} tied, all match ({:.1?})",
            results.len(),
            start.elapsed()
        );
    }
}

#[test]
#[ignore = "needs the oracle's probe files and a bundle: cargo xtask oracle probe"]
fn tree_equals_brute_force_on_probed_climates() {
    let files = probe_files(&spyglass_oracle::default_dir(), "density.json");
    assert!(
        !files.is_empty(),
        "no probe files; run cargo xtask oracle probe"
    );
    for path in files {
        let file = ProbeFile::read(&path).unwrap();
        let source = BiomeSource::overworld(&bundle_for(&file.version)).unwrap();
        let mut checked = 0;
        for case in &file.cases {
            if case.test != "climate" || case.method.as_deref() != Some("target") {
                continue;
            }
            let target = spyglass_biome::TargetPoint(case.i64s().try_into().expect("six values"));
            let tree = source
                .tree()
                .expect("the overworld's source is multi-noise");
            assert_eq!(
                tree.lookup(&target),
                tree.brute_force(&target),
                "{}",
                case.label()
            );
            checked += 1;
        }
        assert!(checked > 0, "{}: no climate targets", path.display());
        println!("{}: {checked} targets agree", path.display());
    }
}

#[test]
#[ignore = "needs a bundle: spyglass-extract 26.3"]
fn region_fill_equals_point_lookups() {
    let source = BiomeSource::overworld(&bundle_for("26.3")).unwrap();
    for (seed, min) in [
        (0, [-3, -16, -5]),
        (-7, [250_000, 20, -250_000]),
        (i64::MIN, [-7, 60, 3]),
    ] {
        let biomes = source.with_seed(seed).unwrap();
        let size = [5, 20, 4];
        let region = biomes.biomes_in(min, size);
        for y in 0..size[1] as i32 {
            for z in 0..size[2] as i32 {
                for x in 0..size[0] as i32 {
                    let (qx, qy, qz) = (min[0] + x, min[1] + y, min[2] + z);
                    assert_eq!(
                        region.get(qx, qy, qz),
                        Some(biomes.biome_at(qx, qy, qz)),
                        "seed {seed} quart ({qx}, {qy}, {qz})"
                    );
                }
            }
        }
    }
}

#[test]
#[ignore = "needs a bundle: spyglass-extract 26.3"]
fn grid_equals_point_lookups() {
    let source = BiomeSource::overworld(&bundle_for("26.3")).unwrap();
    // Map tiles' grids: every quart, then one in 16 and one in 64 (scales 4, 64 and 256).
    for (seed, origin, step) in [
        (0, [-40, 25], 1),
        (-4_893_098_694_207_956_070, [-1024, 512], 16),
        (i64::MIN, [7_000_000, -7_000_000], 64),
    ] {
        let biomes = source.with_seed(seed).unwrap();
        let size = [24, 18];
        let grid = biomes.biome_grid(origin, 16, step, size);
        for (i, cell) in grid.iter().enumerate() {
            let (x, z) = ((i % 24) as i32, (i / 24) as i32);
            let (qx, qz) = (origin[0] + x * step, origin[1] + z * step);
            assert_eq!(
                *cell,
                biomes.biome_at(qx, 16, qz),
                "seed {seed} quart ({qx}, 16, {qz})"
            );
        }
    }
}
