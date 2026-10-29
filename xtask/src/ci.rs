//! `cargo xtask ci --game-version <v>`: one version's CI job, the same steps in the same order
//! as the workflow runs them (ADR 0028). Everything game-derived is made fresh: the bundle is
//! re-extracted and the oracle outputs regenerated, then every comparison must run and pass.
//! CI splits a version between jobs with `--only` and `--exclude`, and heights by seed where
//! they're slow, so each job fits its hour (ADR 0041). Each job makes the outputs its tests
//! read, so nothing game-derived passes between jobs (ADR 0008).

use anyhow::{Result, bail};
use spyglass_extract::cache::JarCache;
use spyglass_extract::extract::{self, Step};

use crate::oracle::{self, Common, OracleCmd};
use crate::oracle_test::{self, Input};
use crate::repo_root;

/// What `oracle test-pack` makes for `<version>+testpack`.
const MADE_BY_TEST_PACK: &[Input] = &[Input::Bundle, Input::Probe, Input::Structures];

#[derive(clap::Args)]
pub struct Ci {
    /// Game version: an id, `latest` or `latest-snapshot`.
    #[arg(long)]
    game_version: String,
    /// Make only these oracle outputs, comma-separated, and run only the tests that read them.
    /// CI runs a version as several jobs this way, since some versions' heights take most of an
    /// hour.
    #[arg(long, value_enum, value_delimiter = ',', conflicts_with = "exclude")]
    only: Vec<Output>,
    /// Make every oracle output but these, and run every test that doesn't read them, including
    /// the ones that need only the bundle, and the test pack's.
    #[arg(long, value_enum, value_delimiter = ',')]
    exclude: Vec<Output>,
    /// With `--only heights`: only shard `I` of `N` of the golden seeds, written `I/N`.
    #[arg(long, value_parser = shard)]
    shard: Option<(usize, usize)>,
    /// Parallel jobs for the golden files. Defaults to the number of CPUs.
    #[arg(long)]
    threads: Option<usize>,
    /// Accept the Minecraft EULA for `oracle locate`'s server, as `SPYGLASS_ACCEPT_EULA=1`
    /// does (ADR 0033).
    #[arg(long)]
    accept_eula: bool,
}

/// An oracle output, named as `cargo xtask oracle` names its subcommands.
#[derive(Clone, Copy, PartialEq, Eq, Debug, clap::ValueEnum)]
enum Output {
    Probe,
    Biomes,
    Structures,
    Spawn,
    Heights,
    Locate,
}

impl Output {
    const ALL: [Output; 6] = [
        Output::Probe,
        Output::Biomes,
        Output::Structures,
        Output::Spawn,
        Output::Heights,
        Output::Locate,
    ];

    /// The test input it is.
    fn input(self) -> Input {
        match self {
            Output::Probe => Input::Probe,
            Output::Biomes => Input::Biomes,
            Output::Structures => Input::Structures,
            Output::Spawn => Input::Spawn,
            Output::Heights => Input::Heights,
            Output::Locate => Input::Locate,
        }
    }
}

/// `I/N`, a shard of N, counted from 1.
fn shard(text: &str) -> Result<(usize, usize), String> {
    text.split_once('/')
        .and_then(|(i, n)| Some((i.parse().ok()?, n.parse().ok()?)))
        .filter(|&(i, n)| (1..=n).contains(&i))
        .ok_or_else(|| format!("expected I/N with 1 <= I <= N, got {text:?}"))
}

/// The outputs to make: `only`, or all but `exclude`.
fn selected(only: &[Output], exclude: &[Output]) -> Vec<Output> {
    Output::ALL
        .into_iter()
        .filter(|o| {
            if only.is_empty() {
                !exclude.contains(o)
            } else {
                only.contains(o)
            }
        })
        .collect()
}

/// Whether a test with `inputs` runs: its oracle outputs are all `made`. A test that reads no
/// oracle output, only the bundle, runs unless `only` limits the job to particular outputs, so
/// each such test runs in exactly one of a version's jobs.
fn runs(inputs: &[Input], made: &[Input], only: bool) -> bool {
    let mut outputs = inputs
        .iter()
        .filter(|i| !matches!(i, Input::Bundle | Input::Extracted))
        .peekable();
    if outputs.peek().is_none() {
        !only
    } else {
        outputs.all(|i| made.contains(i))
    }
}

/// The bundle, and what `outputs` make, as test inputs.
fn made(outputs: &[Output]) -> Vec<Input> {
    let mut made = vec![Input::Bundle, Input::Extracted];
    made.extend(outputs.iter().map(|o| o.input()));
    made
}

pub fn run(args: &Ci) -> Result<()> {
    let only = !args.only.is_empty();
    if args.shard.is_some() && args.only != [Output::Heights] {
        bail!("--shard splits only --only heights");
    }
    let outputs = selected(&args.only, &args.exclude);
    // Asked first, so a job doesn't fail at `oracle locate` after minutes of work.
    if outputs.contains(&Output::Locate) {
        crate::locate::require_eula(args.accept_eula)?;
    }
    let version = step("extract the bundle", || extract_bundle(&args.game_version))?;
    let common = || Common::for_version(&version);
    let golden: Vec<Output> = outputs
        .iter()
        .copied()
        .filter(|&o| o != Output::Locate)
        .collect();
    if !golden.is_empty() {
        step(&format!("oracle golden: {golden:?}"), || {
            let harness = oracle::build(&common())?;
            for output in &golden {
                match output {
                    Output::Probe => harness.probe()?,
                    Output::Structures => harness.structures()?,
                    Output::Spawn => harness.spawn()?,
                    Output::Heights => {
                        let (i, n) = args.shard.unwrap_or((1, 1));
                        println!("heights for golden seeds shard {i}/{n}");
                        harness.heights(args.threads, &oracle::golden_seed_shard(i, n))?;
                    }
                    Output::Biomes => harness.golden_biomes(args.threads)?,
                    Output::Locate => {}
                }
            }
            Ok(())
        })?;
    }
    if outputs.contains(&Output::Locate) {
        step("oracle locate", || {
            oracle::run(OracleCmd::Locate {
                common: common(),
                world: Vec::new(),
                accept_eula: true,
            })
        })?;
    }
    let data_version =
        spyglass_data::bundle::read_manifest(&repo_root().join("bundles").join(&version))?
            .data_version;
    // Like the bundle-only tests: in exactly one of a version's jobs.
    let test_pack = !only && oracle::test_pack_covers(data_version)?;
    if test_pack {
        step("oracle test-pack", || {
            oracle::run(OracleCmd::TestPack(common()))
        })?;
    }
    let made = made(&outputs);
    step("oracle-test", || {
        oracle_test::run_made(&version, &made, &|inputs| runs(inputs, &made, only))
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
    fn selections() {
        use Output::*;
        assert_eq!(selected(&[], &[]), Output::ALL);
        assert_eq!(selected(&[Heights], &[]), [Heights]);
        assert_eq!(
            selected(&[], &[Heights]),
            [Probe, Biomes, Structures, Spawn, Locate]
        );
    }

    /// `--only heights` and `--exclude heights` together run every test exactly once.
    #[test]
    fn two_jobs_run_each_test_once() {
        let heights = made(&selected(&[Output::Heights], &[]));
        let others = made(&selected(&[], &[Output::Heights]));
        let tests: &[&[Input]] = &[
            &[Input::Heights, Input::Bundle],
            &[Input::Probe, Input::Bundle],
            &[Input::Locate, Input::Bundle],
            &[Input::Bundle],
            &[Input::Extracted],
        ];
        for inputs in tests {
            assert!(
                runs(inputs, &heights, true) != runs(inputs, &others, false),
                "{inputs:?}"
            );
        }
    }

    #[test]
    fn shards() {
        assert_eq!(shard("1/2"), Ok((1, 2)));
        assert_eq!(shard("3/3"), Ok((3, 3)));
        for bad in ["0/2", "3/2", "1", "a/b", "1/0"] {
            assert!(shard(bad).is_err(), "{bad}");
        }
        // Every golden seed is in exactly one shard.
        let mut all: Vec<&str> = (1..=3)
            .flat_map(|i| oracle::golden_seed_shard(i, 3))
            .collect();
        let whole = oracle::golden_seed_shard(1, 1);
        assert_eq!(all.len(), whole.len());
        all.sort_unstable();
        let mut whole = whole;
        whole.sort_unstable();
        assert_eq!(all, whole);
    }
}
