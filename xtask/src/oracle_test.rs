//! `cargo xtask oracle-test`: the ignored tests that compare spyglass with the game, in a release
//! build, each run only when its local inputs exist (ADR 0008). See docs/specs/oracle.md.

use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::{command, repo_root};

/// What a test reads: a bundle, or one of the oracle's outputs under `oracle-out/<version>/`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Input {
    Bundle,
    Probe,
    Biomes,
    Structures,
    Spawn,
    Locate,
}

impl Input {
    /// Whether some version has it.
    fn exists(self, root: &Path) -> bool {
        let (base, sub) = match self {
            Input::Bundle => ("bundles", None),
            Input::Probe => ("oracle-out", Some("probe")),
            Input::Biomes => ("oracle-out", Some("biomes")),
            Input::Structures => ("oracle-out", Some("structures")),
            Input::Spawn => ("oracle-out", Some("spawn")),
            Input::Locate => ("oracle-out", Some("locate")),
        };
        let Ok(versions) = std::fs::read_dir(root.join(base)) else {
            return false;
        };
        versions.flatten().any(|v| match sub {
            Some(sub) => v.path().join(sub).is_dir(),
            None => v.path().join("bundle.json").is_file(),
        })
    }

    /// The command that makes it.
    fn how(self) -> &'static str {
        match self {
            Input::Bundle => "cargo run -p spyglass-extract",
            Input::Probe => "cargo xtask oracle probe",
            Input::Biomes => "cargo xtask oracle golden",
            Input::Structures => "cargo xtask oracle structures",
            Input::Spawn => "cargo xtask oracle spawn",
            Input::Locate => "cargo xtask oracle locate",
        }
    }
}

/// Every ignored test that checks against the game or a bundle: package, test name, inputs.
/// Tests that read this machine's saves or download the server jar aren't here.
const TESTS: &[(&str, &str, &[Input])] = &[
    (
        "spyglass-rng",
        "every_random_source_value_matches_the_game",
        &[Input::Probe],
    ),
    (
        "spyglass-noise",
        "every_noise_sample_matches_the_game",
        &[Input::Probe, Input::Bundle],
    ),
    (
        "spyglass-density",
        "every_density_value_matches_the_game",
        &[Input::Probe, Input::Bundle],
    ),
    (
        "spyglass-density",
        "every_bundled_density_function_loads",
        &[Input::Bundle],
    ),
    (
        "spyglass-terrain",
        "every_aquifer_block_matches_the_game",
        &[Input::Probe, Input::Bundle],
    ),
    (
        "spyglass-terrain",
        "surface_levels_over_a_volume_are_their_points",
        &[Input::Bundle],
    ),
    (
        "spyglass-oracle",
        "tests::golden_files_round_trip",
        &[Input::Biomes],
    ),
    (
        "spyglass-biome",
        "every_golden_cell_matches_the_game",
        &[Input::Biomes, Input::Bundle],
    ),
    (
        "spyglass-biome",
        "tree_equals_brute_force_on_probed_climates",
        &[Input::Probe, Input::Bundle],
    ),
    (
        "spyglass-biome",
        "region_fill_equals_point_lookups",
        &[Input::Bundle],
    ),
    (
        "spyglass-biome",
        "grid_equals_point_lookups",
        &[Input::Bundle],
    ),
    (
        "spyglass-biome",
        "spawn_origin_matches_the_game",
        &[Input::Spawn, Input::Bundle],
    ),
    (
        "spyglass-structure",
        "loads_every_overworld_structure_set",
        &[Input::Bundle],
    ),
    (
        "spyglass-structure",
        "computes_stronghold_rings",
        &[Input::Bundle],
    ),
    (
        "spyglass-structure",
        "selects_starts_near_the_origin",
        &[Input::Bundle],
    ),
    (
        "spyglass-structure",
        "seed_placements_decide_random_spread_from_48_bits",
        &[Input::Bundle],
    ),
    (
        "spyglass-structure",
        "ring_chunks_in_a_box_settle_only_what_they_need",
        &[Input::Bundle],
    ),
    (
        "spyglass-structure",
        "structures_match_the_oracle",
        &[Input::Structures, Input::Bundle],
    ),
    (
        "spyglass",
        "locate_agrees_with_the_game",
        &[Input::Locate, Input::Bundle],
    ),
    (
        "spyglass",
        "the_real_world_renders_the_same_in_tiles",
        &[Input::Bundle],
    ),
    (
        "spyglass",
        "the_real_world_resolves_and_counts_like_structures_in",
        &[Input::Bundle],
    ),
    (
        "spyglass",
        "search::tests::the_fast_path_agrees_with_evaluate_on_the_real_world",
        &[Input::Bundle],
    ),
    ("spyglass-cli", "structures", &[Input::Bundle]),
    ("spyglass-cli", "biomes", &[Input::Bundle]),
    ("spyglass-cli", "slime_and_spawn", &[Input::Bundle]),
    ("spyglass-cli", "a_default_world", &[Input::Bundle]),
    (
        "spyglass-cli",
        "worlds_spyglass_cant_predict",
        &[Input::Bundle],
    ),
    ("spyglass-cli", "a_preset_world_is_named", &[Input::Bundle]),
];

pub fn run() -> Result<()> {
    let root = repo_root();
    let mut packages = Vec::new();
    let mut names = Vec::new();
    for &(package, name, inputs) in TESTS {
        let missing: Vec<Input> = inputs.iter().copied().filter(|i| !i.exists(root)).collect();
        if missing.is_empty() {
            if !packages.contains(&package) {
                packages.push(package);
            }
            names.push(name);
        } else {
            let how: Vec<&str> = missing.iter().map(|i| i.how()).collect();
            println!("skipping {package} {name}: needs {}", how.join(", "));
        }
    }
    if names.is_empty() {
        bail!("no oracle test has its inputs; see docs/getting-started.md#6-the-oracle");
    }
    println!("running {} of {} oracle tests", names.len(), TESTS.len());
    let mut cmd = command("cargo");
    cmd.args(["test", "--release"]);
    for p in &packages {
        cmd.args(["-p", p]);
    }
    cmd.args(["--", "--ignored", "--exact"]).args(&names);
    if !cmd.status().context("failed to run cargo")?.success() {
        bail!("oracle tests failed");
    }
    Ok(())
}
