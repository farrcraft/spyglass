//! `cargo xtask ci-plan`: CI's oracle jobs, planned from estimated costs
//! (docs/adr/0058-oracle-ci-planned-by-cost.md). Each version's work is divided into units, an
//! oracle output for one seed or for all of them, or the tests that read only the bundle, and the
//! units are packed into jobs that fit [`BUDGET`]. It prints the jobs as the `oracle` job's
//! matrix, a `matrix=` line for `$GITHUB_OUTPUT`, and a summary to stderr.

use std::fmt;

use anyhow::{Context, Result, bail};
use spyglass_data::bundle::Kind as Release;
use spyglass_data::versions::CHECKED;

use crate::oracle::{self, DUNGEON_SEED, GOLDEN_SEEDS};
use crate::oracle_test::Input;

#[derive(clap::Args)]
pub struct CiPlan {
    /// `full` checks every version in full; `pr` checks the newest in full and the others in
    /// part.
    #[arg(long, value_enum)]
    mode: Mode,
    /// What the pull request merges into, e.g. `origin/main`, for `pr`: a version whose harness
    /// adapter the pull request changes is checked in full.
    #[arg(long, required_if_eq("mode", "pr"))]
    base: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, clap::ValueEnum)]
enum Mode {
    Full,
    Pr,
}

/// A job's estimated minutes, setup included, that the planner keeps under. The timeout is 60;
/// the rest is room for runners slower than the estimates.
const BUDGET: f64 = 40.0;

/// The seed a version checked in part makes its per-seed outputs for.
const SMOKE_SEED: &str = "0";

/// What a unit makes, named as `cargo xtask oracle` names its subcommands, or `bundle`: the
/// tests that read no oracle output, and the test pack.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) enum Kind {
    Probe,
    Biomes,
    Structures,
    Spawn,
    Heights,
    Blocks,
    Features,
    Containers,
    Locate,
    Loot,
    Bundle,
}

impl Kind {
    /// Every kind, in the order a job makes them.
    pub(crate) const ALL: [Kind; 11] = [
        Kind::Probe,
        Kind::Biomes,
        Kind::Structures,
        Kind::Spawn,
        Kind::Heights,
        Kind::Blocks,
        Kind::Features,
        Kind::Containers,
        Kind::Locate,
        Kind::Loot,
        Kind::Bundle,
    ];

    fn name(self) -> &'static str {
        match self {
            Kind::Probe => "probe",
            Kind::Biomes => "biomes",
            Kind::Structures => "structures",
            Kind::Spawn => "spawn",
            Kind::Heights => "heights",
            Kind::Blocks => "blocks",
            Kind::Features => "features",
            Kind::Containers => "containers",
            Kind::Locate => "locate",
            Kind::Loot => "loot",
            Kind::Bundle => "bundle",
        }
    }

    /// The seeds it's made for one at a time, or `None` if it's made for all at once.
    pub(crate) fn seeds(self) -> Option<Vec<&'static str>> {
        match self {
            Kind::Heights | Kind::Blocks | Kind::Containers => Some(GOLDEN_SEEDS.to_vec()),
            Kind::Features => Some(oracle::feature_seeds()),
            _ => None,
        }
    }

    /// The test inputs it makes.
    pub(crate) fn inputs(self) -> &'static [Input] {
        match self {
            Kind::Probe => &[Input::Probe],
            Kind::Biomes => &[Input::Biomes],
            Kind::Structures => &[Input::Structures],
            Kind::Spawn => &[Input::Spawn],
            Kind::Heights => &[Input::Heights],
            // `oracle blocks` makes both runs.
            Kind::Blocks => &[Input::Blocks, Input::BlocksNoCarvers],
            Kind::Features => &[Input::Features],
            Kind::Containers => &[Input::Containers],
            Kind::Locate => &[Input::Locate],
            Kind::Loot => &[Input::Loot],
            Kind::Bundle => &[],
        }
    }
}

/// One piece of a version's oracle work: a kind, for one seed if it's made per seed.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) struct Unit {
    pub(crate) kind: Kind,
    pub(crate) seed: Option<&'static str>,
}

impl Unit {
    /// Every unit of a version.
    pub(crate) fn all() -> Vec<Unit> {
        Kind::ALL
            .into_iter()
            .flat_map(|kind| match kind.seeds() {
                Some(seeds) => seeds
                    .into_iter()
                    .map(|seed| Unit {
                        kind,
                        seed: Some(seed),
                    })
                    .collect(),
                None => vec![Unit { kind, seed: None }],
            })
            .collect()
    }

    /// The units of a version checked in part: everything made for all seeds at once, and the
    /// per-seed kinds for [`SMOKE_SEED`] only.
    fn smoke() -> Vec<Unit> {
        Unit::all()
            .into_iter()
            .filter(|u| u.seed.is_none_or(|s| s == SMOKE_SEED))
            .collect()
    }

    /// Its estimated minutes, the comparisons that read it included.
    fn minutes(self, costs: &Costs) -> f64 {
        match self.kind {
            Kind::Probe => costs.probe,
            Kind::Biomes => costs.biomes,
            Kind::Structures => costs.structures,
            Kind::Spawn => costs.spawn,
            Kind::Heights => costs.heights,
            Kind::Blocks => costs.blocks,
            Kind::Features if self.seed == Some(DUNGEON_SEED) => costs.features + costs.dungeon,
            Kind::Features => costs.features,
            Kind::Containers => costs.containers,
            Kind::Locate => costs.locate,
            Kind::Loot => costs.loot,
            Kind::Bundle => costs.bundle,
        }
    }

    /// `heights:0`, or `probe`, as [`fmt::Display`] writes it.
    pub(crate) fn parse(text: &str) -> Result<Unit, String> {
        let (name, seed) = match text.split_once(':') {
            Some((name, seed)) => (name, Some(seed)),
            None => (text, None),
        };
        let kind = Kind::ALL
            .into_iter()
            .find(|k| k.name() == name)
            .ok_or_else(|| format!("no unit kind {name:?}"))?;
        let seed = match (kind.seeds(), seed) {
            (Some(seeds), Some(seed)) => Some(
                seeds
                    .into_iter()
                    .find(|s| *s == seed)
                    .ok_or_else(|| format!("{seed:?} isn't one of {name}'s seeds"))?,
            ),
            (None, None) => None,
            (Some(_), None) => return Err(format!("{name} needs a seed: {name}:<seed>")),
            (None, Some(_)) => return Err(format!("{name} is made for every seed at once")),
        };
        Ok(Unit { kind, seed })
    }
}

impl fmt::Display for Unit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.seed {
            Some(seed) => write!(f, "{}:{seed}", self.kind.name()),
            None => f.write_str(self.kind.name()),
        }
    }
}

/// A version's estimated minutes on a hosted runner, from CI's logs: `cargo xtask ci` prints
/// each unit's time. Per-seed figures are a seed's.
struct Costs {
    version: &'static str,
    /// Checkout, the JDK, building xtask and the tests, extracting the bundle and compiling the
    /// harness, and the comparisons every job runs on the seeds it made (`features_in`,
    /// `placing`): what every job pays.
    setup: f64,
    probe: f64,
    biomes: f64,
    structures: f64,
    spawn: f64,
    /// A seed's heights.
    heights: f64,
    /// A seed's blocks, both runs.
    blocks: f64,
    /// A seed's features, with their comparisons.
    features: f64,
    /// The test that searches for [`DUNGEON_SEED`] and checks its dungeons, on top of the seed's
    /// features.
    dungeon: f64,
    /// A seed's containers, with their comparison. Estimated from local runs (16 threads) at
    /// four times as long, until CI's logs have them.
    containers: f64,
    /// `oracle locate`'s server, and its comparison.
    locate: f64,
    /// `oracle loot`'s fills, and their comparison.
    loot: f64,
    /// The tests that read only the bundle, and the test pack where it loads.
    bundle: f64,
}

/// Estimates, oldest version first. A version without its own uses the newest older version's,
/// or the oldest's. Per-seed figures are the slowest runner's, so the budget's slack covers
/// drift. 26.2 computes heights about five times slower than later versions, in the game and in
/// spyglass, so its dungeon search takes about 22 minutes against 4.
const COSTS: &[Costs] = &[
    Costs {
        version: "26.2",
        setup: 10.0,
        probe: 0.2,
        biomes: 7.0,
        structures: 2.1,
        spawn: 0.3,
        heights: 5.0,
        blocks: 0.5,
        features: 5.0,
        dungeon: 22.0,
        containers: 6.0,
        locate: 8.5,
        loot: 1.0,
        bundle: 14.0,
    },
    Costs {
        version: "26.3",
        setup: 7.0,
        probe: 0.2,
        biomes: 1.6,
        structures: 1.1,
        spawn: 0.4,
        heights: 1.0,
        blocks: 0.3,
        features: 3.1,
        dungeon: 4.5,
        containers: 3.0,
        locate: 2.0,
        loot: 1.0,
        bundle: 3.0,
    },
    // The test pack loads from here on.
    Costs {
        version: "26.4-snapshot-6",
        setup: 7.0,
        probe: 0.2,
        biomes: 1.6,
        structures: 1.1,
        spawn: 0.4,
        heights: 1.0,
        blocks: 0.3,
        features: 3.1,
        dungeon: 4.8,
        containers: 3.0,
        locate: 2.5,
        loot: 1.0,
        bundle: 11.0,
    },
];

/// `id`'s data version, from [`CHECKED`].
fn data_version(id: &str) -> Result<u32> {
    CHECKED
        .iter()
        .find(|c| c.id == id)
        .map(|c| c.data_version)
        .with_context(|| format!("{id} isn't in spyglass_data::versions::CHECKED"))
}

/// The estimates for `id`.
fn costs(id: &str) -> Result<&'static Costs> {
    let target = data_version(id)?;
    let mut best = &COSTS[0];
    for costs in COSTS {
        if data_version(costs.version)? <= target {
            best = costs;
        }
    }
    Ok(best)
}

/// One oracle job: a version and the units it makes.
#[derive(Debug)]
struct Job {
    version: &'static str,
    units: Vec<Unit>,
    minutes: f64,
}

/// Packs `units` of `version` into as few jobs as fit [`BUDGET`], balanced so each job has as
/// much room as it can for a slow runner. For each job count from the fewest the total could
/// fit, it deals the units out costliest first, each to the job with the least so far, and
/// keeps the first count where every job fits. A unit over the budget alone still gets a job.
fn pack(version: &'static str, mut units: Vec<Unit>) -> Result<Vec<Job>> {
    let costs = costs(version)?;
    units.sort_by(|a, b| b.minutes(costs).total_cmp(&a.minutes(costs)));
    let room = BUDGET - costs.setup;
    let total: f64 = units.iter().map(|u| u.minutes(costs)).sum();
    let fewest = ((total / room).ceil() as usize).max(1);
    for count in fewest..=units.len().max(1) {
        let mut jobs: Vec<Job> = (0..count)
            .map(|_| Job {
                version,
                units: Vec::new(),
                minutes: costs.setup,
            })
            .collect();
        for &unit in &units {
            let job = jobs
                .iter_mut()
                .min_by(|a, b| a.minutes.total_cmp(&b.minutes))
                .expect("at least one job");
            job.units.push(unit);
            job.minutes += unit.minutes(costs);
        }
        if count == units.len() || jobs.iter().all(|j| j.minutes <= BUDGET) {
            for job in &mut jobs {
                job.units.sort();
            }
            return Ok(jobs);
        }
    }
    unreachable!("a job per unit always returns")
}

/// The versions checked in full on a pull request: the newest release, the newest snapshot if
/// it's newer, and any whose harness adapters `changed` touches.
fn full_on_a_pull_request(changed: &[&str]) -> Result<Vec<&'static str>> {
    let newest_release = CHECKED
        .iter()
        .filter(|c| Release::from_id(c.id) == Release::Release)
        .map(|c| c.data_version)
        .max()
        .unwrap_or(0);
    let newest = CHECKED.iter().map(|c| c.data_version).max().unwrap_or(0);
    let mut full = Vec::new();
    for checked in CHECKED {
        let adapters = oracle::adapter_dirs(checked.data_version)?;
        let adapter_changed = changed.iter().any(|path| {
            adapters
                .iter()
                .any(|dir| path.starts_with(&format!("{dir}/")))
        });
        if checked.data_version == newest_release
            || (checked.data_version == newest && newest > newest_release)
            || adapter_changed
        {
            full.push(checked.id);
        }
    }
    Ok(full)
}

/// Every version's jobs: in full for those in `full`, in part for the others.
fn plan(full: &[&str]) -> Result<Vec<Job>> {
    let mut jobs = Vec::new();
    for checked in CHECKED {
        let units = if full.contains(&checked.id) {
            Unit::all()
        } else {
            Unit::smoke()
        };
        jobs.extend(pack(checked.id, units)?);
    }
    Ok(jobs)
}

/// A job's name in the checks list: `oracle 26.2 1/4: heights ×3, features ×2`.
fn name(job: &Job, index: usize, of: usize) -> String {
    let mut parts = Vec::new();
    for kind in Kind::ALL {
        let n = job.units.iter().filter(|u| u.kind == kind).count();
        match (n, kind.seeds()) {
            (0, _) => {}
            (_, None) => parts.push(kind.name().to_string()),
            (n, Some(_)) => parts.push(format!("{} ×{n}", kind.name())),
        }
    }
    let mut name = format!("oracle {}", job.version);
    if of > 1 {
        name.push_str(&format!(" {index}/{of}"));
    }
    format!("{name}: {}", parts.join(", "))
}

/// The jobs as the workflow's matrix, `{"include": [{"name", "version", "units"}, ...]}`.
fn matrix(jobs: &[Job]) -> serde_json::Value {
    let include: Vec<serde_json::Value> = CHECKED
        .iter()
        .flat_map(|checked| {
            let mine: Vec<&Job> = jobs.iter().filter(|j| j.version == checked.id).collect();
            let of = mine.len();
            mine.into_iter().enumerate().map(move |(i, job)| {
                let units: Vec<String> = job.units.iter().map(Unit::to_string).collect();
                serde_json::json!({
                    "name": name(job, i + 1, of),
                    "version": job.version,
                    "units": units.join(","),
                })
            })
        })
        .collect();
    serde_json::json!({ "include": include })
}

/// Prints the matrix for `args.mode`, and the plan with its estimates to stderr.
pub fn run(args: &CiPlan) -> Result<()> {
    let full: Vec<&str> = match (args.mode, &args.base) {
        (Mode::Full, _) => CHECKED.iter().map(|c| c.id).collect(),
        (Mode::Pr, Some(base)) => {
            let changed = crate::ci_scope::changed_files(base)?;
            let changed: Vec<&str> = changed.iter().map(String::as_str).collect();
            full_on_a_pull_request(&changed)?
        }
        (Mode::Pr, None) => bail!("--mode pr needs --base"),
    };
    let jobs = plan(&full)?;
    for checked in CHECKED {
        let how = if full.contains(&checked.id) {
            "in full"
        } else {
            "in part"
        };
        eprintln!("{} {how}:", checked.id);
        for job in jobs.iter().filter(|j| j.version == checked.id) {
            let units: Vec<String> = job.units.iter().map(Unit::to_string).collect();
            eprintln!("  ~{:.0} min: {}", job.minutes, units.join(" "));
        }
    }
    println!("matrix={}", matrix(&jobs));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oracle_test::TESTS;

    #[test]
    fn units_parse_as_they_print() {
        for unit in Unit::all() {
            assert_eq!(Unit::parse(&unit.to_string()), Ok(unit));
        }
        for bad in ["heights", "probe:0", "heights:2", "features:x", "nothing"] {
            assert!(Unit::parse(bad).is_err(), "{bad}");
        }
        assert!(Unit::parse("features:56").is_ok());
        assert!(Unit::parse("blocks:56").is_err());
    }

    #[test]
    fn costs_name_checked_versions_in_order() {
        let order: Vec<u32> = COSTS
            .iter()
            .map(|c| data_version(c.version).unwrap())
            .collect();
        assert!(order.is_sorted(), "COSTS must be oldest first");
        assert_eq!(costs("26.4-snapshot-5").unwrap().version, "26.3");
        assert_eq!(costs("26.2").unwrap().version, "26.2");
    }

    /// Every unit is in exactly one job, and every job but one of a single unit fits the budget.
    #[test]
    fn jobs_hold_every_unit_once_within_the_budget() {
        let all: Vec<&str> = CHECKED.iter().map(|c| c.id).collect();
        for full in [all.as_slice(), &[]] {
            let jobs = plan(full).unwrap();
            for checked in CHECKED {
                let mut planned: Vec<Unit> = jobs
                    .iter()
                    .filter(|j| j.version == checked.id)
                    .flat_map(|j| j.units.clone())
                    .collect();
                planned.sort();
                let mut expected = if full.contains(&checked.id) {
                    Unit::all()
                } else {
                    Unit::smoke()
                };
                expected.sort();
                assert_eq!(planned, expected, "{}", checked.id);
            }
            for job in &jobs {
                assert!(
                    job.minutes <= BUDGET || job.units.len() == 1,
                    "{job:?} is over the budget"
                );
            }
        }
    }

    /// In full, every comparison runs in some job, and one that reads no per-seed output runs in
    /// exactly one.
    #[test]
    fn a_full_plan_runs_every_test() {
        let jobs = plan(&["26.3"]).unwrap();
        let jobs: Vec<&Job> = jobs.iter().filter(|j| j.version == "26.3").collect();
        for (package, name, inputs) in TESTS {
            let n = jobs
                .iter()
                .filter(|j| crate::ci::runs(inputs, &j.units))
                .count();
            let per_seed = inputs.iter().any(|i| {
                Kind::ALL
                    .iter()
                    .any(|k| k.seeds().is_some() && k.inputs().contains(i))
            });
            if per_seed {
                assert!(n >= 1, "{package} {name} runs in no job");
            } else {
                assert_eq!(n, 1, "{package} {name} runs in {n} jobs");
            }
        }
    }

    #[test]
    fn a_pull_request_checks_the_newest_in_full() {
        let full = full_on_a_pull_request(&["crates/spyglass-feature/src/lib.rs"]).unwrap();
        let newest_release = CHECKED
            .iter()
            .rev()
            .find(|c| Release::from_id(c.id) == Release::Release)
            .unwrap();
        assert!(full.contains(&newest_release.id));
        let newest = CHECKED.last().unwrap();
        assert!(full.contains(&newest.id));
        assert!(!full.contains(&CHECKED[0].id) || CHECKED[0].id == newest_release.id);
        // A change to 26.2's adapter checks 26.2 in full too.
        let full = full_on_a_pull_request(&["oracle/adapters/26.2/Chunks.java"]).unwrap();
        assert!(full.contains(&"26.2"));
    }
}
