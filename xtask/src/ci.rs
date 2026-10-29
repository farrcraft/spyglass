//! `cargo xtask ci --game-version <v>`: one version's CI job, the same steps in the same order
//! as the workflow runs them (ADR 0028). Everything game-derived is made fresh: the bundle is
//! re-extracted and every oracle output regenerated, then every comparison must run and pass.
//! CI runs it in parts (`--part heights`, `--part rest`), heights split by seed where they're
//! slow, so each job fits its hour (ADR 0041); each part makes its own inputs, so nothing
//! game-derived passes between jobs (ADR 0008).

use anyhow::{Result, bail};
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
    /// Run only part of the job: `heights` (the height golden files and their test) or `rest`
    /// (everything else). CI runs the parts as separate jobs, since some versions' heights take
    /// most of an hour. Without it, everything runs.
    #[arg(long, value_enum)]
    part: Option<Part>,
    /// With `--part heights`: only shard `I` of `N` of the golden seeds, written `I/N`.
    #[arg(long, value_parser = shard, requires = "part")]
    shard: Option<(usize, usize)>,
    /// Parallel jobs for the golden files. Defaults to the number of CPUs.
    #[arg(long)]
    threads: Option<usize>,
    /// Accept the Minecraft EULA for `oracle locate`'s server, as `SPYGLASS_ACCEPT_EULA=1`
    /// does (ADR 0033).
    #[arg(long)]
    accept_eula: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, clap::ValueEnum)]
enum Part {
    Heights,
    Rest,
}

/// `I/N`, a shard of N, counted from 1.
fn shard(text: &str) -> Result<(usize, usize), String> {
    text.split_once('/')
        .and_then(|(i, n)| Some((i.parse().ok()?, n.parse().ok()?)))
        .filter(|&(i, n)| (1..=n).contains(&i))
        .ok_or_else(|| format!("expected I/N with 1 <= I <= N, got {text:?}"))
}

fn needs_heights(inputs: &[Input]) -> bool {
    inputs.contains(&Input::Heights)
}

fn without_heights(inputs: &[Input]) -> bool {
    !needs_heights(inputs)
}

pub fn run(args: &Ci) -> Result<()> {
    if args.shard.is_some() && args.part != Some(Part::Heights) {
        bail!("--shard splits only --part heights");
    }
    let heights = args.part != Some(Part::Rest);
    let rest = args.part != Some(Part::Heights);
    // Asked first, so a job doesn't fail at `oracle locate` after minutes of work.
    if rest {
        crate::locate::require_eula(args.accept_eula)?;
    }
    let version = step("extract the bundle", || extract_bundle(&args.game_version))?;
    let common = || Common::for_version(&version);
    if rest {
        step("oracle golden (all but heights)", || {
            oracle::golden_but_heights(&common(), args.threads)
        })?;
    }
    if heights {
        let (i, n) = args.shard.unwrap_or((1, 1));
        let seeds = oracle::golden_seed_shard(i, n);
        step(&format!("oracle golden heights (seeds {i}/{n})"), || {
            oracle::golden_heights(&common(), args.threads, &seeds)
        })?;
    }
    if rest {
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
    let test_pack = rest && oracle::test_pack_covers(data_version)?;
    if test_pack {
        step("oracle test-pack", || {
            oracle::run(OracleCmd::TestPack(common()))
        })?;
    }
    let (made, keep): (&[Input], oracle_test::Keep) = match args.part {
        None => (MADE, |_| true),
        Some(Part::Heights) => (
            &[Input::Bundle, Input::Extracted, Input::Heights],
            needs_heights,
        ),
        Some(Part::Rest) => (MADE, without_heights),
    };
    step("oracle-test", || {
        oracle_test::run_made(&version, made, keep)
    })?;
    if test_pack {
        let variant = format!("{version}+testpack");
        step("oracle-test (test pack)", || {
            oracle_test::run_made(&variant, MADE_BY_TEST_PACK, |_| true)
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
