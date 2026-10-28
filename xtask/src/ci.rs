//! `cargo xtask ci --game-version <v>`: one version's CI job, the same steps in the same order
//! as the workflow runs them (ADR 0028). Everything game-derived is made fresh: the bundle is
//! re-extracted and every oracle output regenerated, then every comparison must run and pass.

use anyhow::Result;
use spyglass_extract::cache::JarCache;
use spyglass_extract::extract::{self, Step};

use crate::oracle::{self, Common, OracleCmd};
use crate::oracle_test::{self, Input};
use crate::repo_root;

/// What extraction, the golden set and `oracle locate` make: every input.
const MADE: &[Input] = &[
    Input::Bundle,
    Input::Extracted,
    Input::Probe,
    Input::Biomes,
    Input::Structures,
    Input::Spawn,
    Input::Locate,
    Input::Heights,
];

/// What `oracle test-pack` makes for `<version>+testpack`.
const MADE_BY_TEST_PACK: &[Input] = &[Input::Bundle, Input::Probe, Input::Structures];

#[derive(clap::Args)]
pub struct Ci {
    /// Game version: an id, `latest` or `latest-snapshot`.
    #[arg(long)]
    game_version: String,
    /// Parallel jobs for the golden files. Defaults to the number of CPUs.
    #[arg(long)]
    threads: Option<usize>,
    /// Accept the Minecraft EULA for `oracle locate`'s server, as `SPYGLASS_ACCEPT_EULA=1`
    /// does (ADR 0033).
    #[arg(long)]
    accept_eula: bool,
}

pub fn run(args: &Ci) -> Result<()> {
    // Asked first, so a job doesn't fail at `oracle locate` after minutes of work.
    crate::locate::require_eula(args.accept_eula)?;
    let version = step("extract the bundle", || extract_bundle(&args.game_version))?;
    let common = || Common::for_version(&version);
    step("oracle golden", || {
        oracle::run(OracleCmd::Golden {
            common: common(),
            threads: args.threads,
        })
    })?;
    step("oracle locate", || {
        oracle::run(OracleCmd::Locate {
            common: common(),
            world: Vec::new(),
            accept_eula: true,
        })
    })?;
    let data_version =
        spyglass_data::bundle::read_manifest(&repo_root().join("bundles").join(&version))?
            .data_version;
    let test_pack = oracle::test_pack_covers(data_version)?;
    if test_pack {
        step("oracle test-pack", || {
            oracle::run(OracleCmd::TestPack(common()))
        })?;
    }
    step("oracle-test", || oracle_test::run_made(&version, MADE))?;
    if test_pack {
        let variant = format!("{version}+testpack");
        step("oracle-test (test pack)", || {
            oracle_test::run_made(&variant, MADE_BY_TEST_PACK)
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
