//! `cargo xtask oracle-test`: the ignored tests that compare spyglass with the game, in a release
//! build, each run only when its local inputs exist (ADR 0008), once per game version with a
//! bundle. See docs/specs/oracle.md.

use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::{command, repo_root};

/// The environment variable that tells the tests which game version to check
/// (`spyglass_oracle::VERSION_VAR`).
const VERSION_VAR: &str = "SPYGLASS_ORACLE_VERSION";

/// What a test reads: a bundle, or one of the oracle's outputs under `oracle-out/<version>/`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Input {
    Bundle,
    /// A bundle extracted into `bundles/`, which the front ends find by version; not a merged
    /// test pack's.
    Extracted,
    Probe,
    Biomes,
    Structures,
    Spawn,
    Locate,
    Heights,
}

impl Input {
    /// Whether `version` has it.
    fn exists(self, root: &Path, version: &str) -> bool {
        let sub = match self {
            Input::Extracted => {
                return root
                    .join("bundles")
                    .join(version)
                    .join("bundle.json")
                    .is_file();
            }
            Input::Bundle => {
                return BUNDLE_DIRS
                    .iter()
                    .any(|d| root.join(d).join(version).join("bundle.json").is_file());
            }
            Input::Probe => "probe",
            Input::Biomes => "biomes",
            Input::Structures => "structures",
            Input::Spawn => "spawn",
            Input::Locate => "locate",
            Input::Heights => "heights",
        };
        root.join("oracle-out").join(version).join(sub).is_dir()
    }

    /// The command that makes it.
    fn how(self) -> &'static str {
        match self {
            Input::Bundle | Input::Extracted => "cargo run -p spyglass-extract",
            Input::Probe => "cargo xtask oracle probe",
            Input::Biomes => "cargo xtask oracle golden",
            Input::Structures => "cargo xtask oracle structures",
            Input::Spawn => "cargo xtask oracle spawn",
            Input::Locate => "cargo xtask oracle locate",
            Input::Heights => "cargo xtask oracle heights",
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
        "spyglass-density",
        "the_program_matches_the_functions",
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
        "spyglass-terrain",
        "every_height_matches_the_game",
        &[Input::Heights, Input::Bundle],
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
        "spyglass-structure",
        "structures_with_terrain_match_the_oracle",
        &[Input::Structures, Input::Bundle],
    ),
    (
        "spyglass",
        "locate_agrees_with_the_game",
        &[Input::Locate, Input::Bundle],
    ),
    (
        "spyglass",
        "layouts_agree_with_the_game",
        &[Input::Structures, Input::Bundle],
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
    (
        "spyglass",
        "search::tests::the_fast_path_agrees_across_dimensions_on_the_real_game",
        &[Input::Bundle],
    ),
    ("spyglass", "each_dimension_opens", &[Input::Bundle]),
    ("spyglass", "a_game_spans_the_dimensions", &[Input::Bundle]),
    ("spyglass-cli", "structures", &[Input::Extracted]),
    ("spyglass-cli", "jigsaw_bounds", &[Input::Extracted]),
    ("spyglass-cli", "biomes", &[Input::Extracted]),
    ("spyglass-cli", "slime_and_spawn", &[Input::Extracted]),
    ("spyglass-cli", "a_default_world", &[Input::Extracted]),
    (
        "spyglass-cli",
        "a_player_in_the_nether",
        &[Input::Extracted],
    ),
    (
        "spyglass-cli",
        "worlds_spyglass_cant_predict",
        &[Input::Extracted],
    ),
    (
        "spyglass-cli",
        "a_preset_world_is_named",
        &[Input::Extracted],
    ),
];

/// Tests whose expected values are one version's, with that version: they run only for it.
const PINNED: &[(&str, &str)] = &[
    ("loads_every_overworld_structure_set", "26.3"),
    ("a_default_world", "26.3"),
    ("a_player_in_the_nether", "26.3"),
    ("worlds_spyglass_cant_predict", "26.3"),
    ("a_preset_world_is_named", "26.3"),
];

/// How one version's run went.
enum Outcome {
    Passed(usize),
    Failed,
    NoInputs,
}

/// Runs the tests for `version` (every version with a bundle, oldest first, if `None`), then
/// reports per version. A failing version doesn't stop the others.
pub fn run(version: Option<&str>) -> Result<()> {
    let root = repo_root();
    let versions = match version {
        Some(v) => vec![v.to_string()],
        None => bundled_versions(root),
    };
    if versions.is_empty() {
        bail!("no bundle in bundles/; see docs/getting-started.md#6-the-oracle");
    }
    let mut outcomes = Vec::new();
    for version in &versions {
        println!("== {version}");
        let outcome = run_version(root, version, &[])?;
        outcomes.push((version, outcome));
    }
    println!("oracle tests by version:");
    for (version, outcome) in &outcomes {
        match outcome {
            Outcome::Passed(n) => println!("  {version}: passed ({n} tests)"),
            Outcome::Failed => println!("  {version}: failed"),
            Outcome::NoInputs => println!("  {version}: skipped (no inputs)"),
        }
    }
    if outcomes.iter().any(|(_, o)| matches!(o, Outcome::Failed)) {
        bail!("oracle tests failed");
    }
    if outcomes.iter().all(|(_, o)| matches!(o, Outcome::NoInputs)) {
        bail!("no oracle test has its inputs; see docs/getting-started.md#6-the-oracle");
    }
    Ok(())
}

/// Where bundles are: extracted ones, and those `cargo xtask oracle test-pack` merges with the
/// test pack (`spyglass_oracle::bundle_dir`).
const BUNDLE_DIRS: [&str; 2] = ["bundles", "target/oracle/bundles"];

/// The game versions with a bundle, oldest data version first; a test pack's after its
/// version's.
fn bundled_versions(root: &Path) -> Vec<String> {
    let mut found: Vec<(u32, String)> = BUNDLE_DIRS
        .iter()
        .filter_map(|d| std::fs::read_dir(root.join(d)).ok())
        .flatten()
        .flatten()
        .filter_map(|e| spyglass_data::bundle::read_manifest(&e.path()).ok())
        .map(|m| (m.data_version, m.version))
        .collect();
    found.sort();
    found.into_iter().map(|(_, v)| v).collect()
}

/// Runs `version`'s tests as `cargo xtask ci` does, after making `made`: a test that needs one of
/// those and finds it missing is an error, not a skip, and so is a run with no test.
pub(crate) fn run_made(version: &str, made: &[Input]) -> Result<()> {
    match run_version(repo_root(), version, made)? {
        Outcome::Passed(n) => {
            println!("{version}: passed ({n} tests)");
            Ok(())
        }
        Outcome::Failed => bail!("oracle tests failed for {version}"),
        Outcome::NoInputs => bail!("no oracle test ran for {version}"),
    }
}

/// Runs the tests whose inputs `version` has, in one cargo invocation. A test missing any of
/// `made`, which the caller just made, is an error.
fn run_version(root: &Path, version: &str, made: &[Input]) -> Result<Outcome> {
    let mut packages = Vec::new();
    let mut names = Vec::new();
    for &(package, name, inputs) in TESTS {
        if let Some(&(_, pinned)) = PINNED.iter().find(|(n, _)| *n == name)
            && pinned != version
        {
            continue;
        }
        let missing: Vec<Input> = inputs
            .iter()
            .copied()
            .filter(|i| !i.exists(root, version))
            .collect();
        if missing.is_empty() {
            if !packages.contains(&package) {
                packages.push(package);
            }
            names.push(name);
        } else {
            let how: Vec<&str> = missing.iter().map(|i| i.how()).collect();
            if missing.iter().any(|i| made.contains(i)) {
                bail!(
                    "{package} {name} can't run for {version}: missing {}, which should have been made",
                    how.join(", ")
                );
            }
            println!("skipping {package} {name}: needs {}", how.join(", "));
        }
    }
    if names.is_empty() {
        return Ok(Outcome::NoInputs);
    }
    println!(
        "running {} of {} oracle tests for {version}",
        names.len(),
        TESTS.len()
    );
    let mut cmd = command("cargo");
    cmd.env(VERSION_VAR, version)
        .args(["test", "--release", "--no-fail-fast"]);
    for p in &packages {
        cmd.args(["-p", p]);
    }
    cmd.args(["--", "--ignored", "--exact"]).args(&names);
    Ok(if cmd.status().context("failed to run cargo")?.success() {
        Outcome::Passed(names.len())
    } else {
        Outcome::Failed
    })
}
