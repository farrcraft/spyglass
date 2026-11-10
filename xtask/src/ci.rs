//! `cargo xtask ci --game-version <v>`: one version's CI job, the same steps in the same order
//! as the workflow runs them (ADR 0028). Everything game-derived is made fresh: the bundle is
//! re-extracted and the oracle outputs regenerated, then every comparison must run and pass.
//! CI divides a version between jobs with `--units`, as `cargo xtask ci-plan` plans them, so
//! each job fits its hour (ADR 0058). Each job makes the outputs its tests read, so nothing
//! game-derived passes between jobs (ADR 0008).

use std::time::Instant;

use anyhow::Result;
use spyglass_extract::cache::JarCache;
use spyglass_extract::extract::{self, Step};

use crate::ci_plan::{Kind, Unit};
use crate::oracle::{self, Common, DUNGEON_SEED, OracleCmd};
use crate::oracle_test::{self, Input};
use crate::repo_root;

/// What `oracle test-pack` makes for `<version>+testpack`.
const MADE_BY_TEST_PACK: &[Input] = &[Input::Bundle, Input::Probe, Input::Structures];

#[derive(clap::Args)]
pub struct Ci {
    /// Game version: an id, `latest` or `latest-snapshot`.
    #[arg(long)]
    game_version: String,
    /// Make only these units of the version's oracle work, comma-separated, and run only the
    /// tests that read what they make: `probe`, `biomes`, `structures`, `spawn`, `locate`,
    /// `loot`, `bundle` (the tests that read only the bundle, and the test pack), or `heights`, `blocks`
    /// or `features` for one seed, as `heights:0`. `cargo xtask ci-plan` divides a version this
    /// way between CI's jobs. Every unit by default.
    #[arg(long, value_parser = Unit::parse, value_delimiter = ',')]
    units: Vec<Unit>,
    /// Parallel jobs for the golden files. Defaults to the number of CPUs.
    #[arg(long)]
    threads: Option<usize>,
    /// Accept the Minecraft EULA for `oracle locate`'s server, as `SPYGLASS_ACCEPT_EULA=1`
    /// does (ADR 0033).
    #[arg(long)]
    accept_eula: bool,
}

/// The test inputs `units` make, with the bundle, which every job extracts.
fn made(units: &[Unit]) -> Vec<Input> {
    let mut made = vec![Input::Bundle, Input::Extracted];
    for unit in units {
        made.extend(unit.kind.inputs());
    }
    if units.iter().any(|u| u.seed == Some(DUNGEON_SEED)) {
        made.push(Input::DungeonSeed);
    }
    made
}

/// Whether a test with `inputs` runs in a job of `units`: its oracle outputs are all made. A test
/// that reads no oracle output, only the bundle, runs in the job with the `bundle` unit, so each
/// such test runs in exactly one of a version's jobs.
pub(crate) fn runs(inputs: &[Input], units: &[Unit]) -> bool {
    let made = made(units);
    let mut outputs = inputs
        .iter()
        .filter(|i| !matches!(i, Input::Bundle | Input::Extracted))
        .peekable();
    if outputs.peek().is_none() {
        units.iter().any(|u| u.kind == Kind::Bundle)
    } else {
        outputs.all(|i| made.contains(i))
    }
}

/// The seeds of `units` that are `kind`'s.
fn seeds(units: &[Unit], kind: Kind) -> Vec<&'static str> {
    units
        .iter()
        .filter(|u| u.kind == kind)
        .filter_map(|u| u.seed)
        .collect()
}

/// Runs one CI job: extracts the bundle, makes the units' oracle outputs (golden files with one
/// harness build, then `/locate` answers from a server, then the test pack's), and runs the
/// comparisons whose inputs the job made.
pub fn run(args: &Ci) -> Result<()> {
    let units = if args.units.is_empty() {
        Unit::all()
    } else {
        args.units.clone()
    };
    let has = |kind: Kind| units.iter().any(|u| u.kind == kind);
    // Asked first, so a job doesn't fail at `oracle locate` after minutes of work.
    if has(Kind::Locate) {
        crate::locate::require_eula(args.accept_eula)?;
    }
    let version = step("extract the bundle", || extract_bundle(&args.game_version))?;
    let common = || Common::for_version(&version);
    let golden: Vec<Kind> = Kind::ALL
        .into_iter()
        .filter(|&k| has(k) && !matches!(k, Kind::Locate | Kind::Bundle))
        .collect();
    if !golden.is_empty() {
        step(&format!("oracle golden: {golden:?}"), || {
            let mut harness = oracle::build(&common())?;
            for &kind in &golden {
                let seeds = seeds(&units, kind);
                timed(kind, seeds.len(), || match kind {
                    Kind::Probe => harness.probe(),
                    Kind::Biomes => harness.golden_biomes(args.threads),
                    Kind::Structures => harness.structures(),
                    Kind::Spawn => harness.spawn(),
                    Kind::Heights => harness.heights(args.threads, &seeds),
                    Kind::Blocks => oracle::blocks(&mut harness, args.threads, &seeds),
                    Kind::Features => harness.features(args.threads, &seeds),
                    Kind::Containers => harness.containers(args.threads, &seeds),
                    Kind::Loot => harness.loot(),
                    Kind::Locate | Kind::Bundle => Ok(()),
                })?;
            }
            Ok(())
        })?;
    }
    if has(Kind::Locate) {
        step("oracle locate", || {
            timed(Kind::Locate, 0, || {
                oracle::run(OracleCmd::Locate {
                    common: common(),
                    world: Vec::new(),
                    accept_eula: true,
                })
            })
        })?;
    }
    let data_version =
        spyglass_data::bundle::read_manifest(&repo_root().join("bundles").join(&version))?
            .data_version;
    // Like the bundle-only tests: in exactly one of a version's jobs.
    let test_pack = has(Kind::Bundle) && oracle::test_pack_covers(data_version)?;
    if test_pack {
        step("oracle test-pack", || {
            oracle::run(OracleCmd::TestPack(common()))
        })?;
    }
    let made = made(&units);
    step("oracle-test", || {
        oracle_test::run_made(&version, &made, &|inputs| runs(inputs, &units))
    })?;
    if test_pack {
        let variant = format!("{version}+testpack");
        step("oracle-test (test pack)", || {
            oracle_test::run_made(&variant, MADE_BY_TEST_PACK, &|_| true)
        })?;
    }
    println!("ci: {version} passed");
    Ok(())
}

/// Runs `f`, which makes `kind` for `seeds` seeds (0 if it's made for all at once), and prints
/// how long it took, to compare with `cargo xtask ci-plan`'s estimates.
fn timed(kind: Kind, seeds: usize, f: impl FnOnce() -> Result<()>) -> Result<()> {
    let start = Instant::now();
    f()?;
    let minutes = start.elapsed().as_secs_f64() / 60.0;
    if seeds == 0 {
        println!("ci: {kind:?} took {minutes:.1} min");
    } else {
        println!(
            "ci: {kind:?} for {seeds} seeds took {minutes:.1} min, {:.1} a seed",
            minutes / seeds as f64
        );
    }
    Ok(())
}

/// Runs one step, announced so a log shows where a failure happened.
fn step<T>(name: &str, f: impl FnOnce() -> Result<T>) -> Result<T> {
    println!("\n### ci: {name}");
    f()
}

/// Extracts `version` into `bundles/`, replacing any bundle there, and returns its id.
fn extract_bundle(version: &str) -> Result<String> {
    let cache = JarCache::open_default()?;
    let out = repo_root().join("bundles");
    let options = extract::Options {
        version,
        cache: &cache,
        java: None,
        out: &out,
        replace: true,
    };
    let done = extract::extract(&options, |step| {
        if let Step::Writing(dir) = step {
            println!("writing {}", dir.display());
        }
    })?;
    println!(
        "bundle for {} (data version {}): {} files",
        done.manifest.version,
        done.manifest.data_version,
        done.manifest.files.len()
    );
    Ok(done.manifest.version)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tests_run_where_their_inputs_are_made() {
        let unit = |text| Unit::parse(text).unwrap();
        let features = [unit("features:0"), unit("heights:1")];
        assert!(runs(&[Input::Features, Input::Bundle], &features));
        assert!(runs(&[Input::Heights], &features));
        assert!(!runs(&[Input::Bundle], &features));
        assert!(!runs(
            &[Input::Features, Input::DungeonSeed, Input::Bundle],
            &features
        ));
        let dungeon = [unit("features:56"), unit("bundle")];
        assert!(runs(
            &[Input::Features, Input::DungeonSeed, Input::Bundle],
            &dungeon
        ));
        assert!(runs(&[Input::Extracted], &dungeon));
        assert!(!runs(&[Input::Probe, Input::Bundle], &dungeon));
    }
}
