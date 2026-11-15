//! `cargo xtask oracle`: build and run the Java oracle harness in `oracle/`.
//! See docs/adr/0004-oracle-standalone-harness.md and docs/specs/oracle.md.

use std::ffi::OsString;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use spyglass_extract::extract;

use crate::repo_root;

mod build;
mod packs;

pub(crate) use build::{adapter_dirs, build, test_pack_covers};

pub(crate) use packs::blocks;
use packs::{fixture_pack, test_pack, variant_label};

#[derive(Subcommand)]
pub enum OracleCmd {
    /// Fetch the server jar, unpack its libraries and compile the harness. Other subcommands do
    /// this first when anything is stale.
    Build(Common),
    /// Write a golden biome file for one seed and region.
    Biomes {
        #[command(flatten)]
        common: Common,
        /// A number, or text, parsed as the game's world-creation screen does.
        seed: String,
        #[arg(long, default_value = "minecraft:overworld")]
        dimension: String,
        /// Region's minimum block x and z, multiples of 16.
        #[arg(long, default_value_t = 0, allow_hyphen_values = true)]
        x: i32,
        #[arg(long, default_value_t = 0, allow_hyphen_values = true)]
        z: i32,
        /// Region's width and depth in blocks, a multiple of 16. Always full height.
        #[arg(long, default_value_t = 256)]
        size: i32,
        #[arg(long)]
        out: PathBuf,
    },
    /// Compare the harness with the biomes stored in a real world save made in the same version.
    CheckSave {
        #[command(flatten)]
        common: Common,
        world: PathBuf,
        #[arg(long, default_value = "minecraft:overworld")]
        dimension: String,
    },
    /// Write probe files (raw random-source and noise values) for the golden seeds into
    /// `oracle-out/<version>/probe/`.
    Probe(Common),
    /// Write structure golden files (placement chunks, what the game tries there, ring
    /// positions, slime chunks) for the golden seeds and dimensions into
    /// `oracle-out/<version>/structures/`.
    Structures(Common),
    /// Write spawn golden files (the spawn origin and the climate search behind it) for the
    /// golden seeds and dimensions into `oracle-out/<version>/spawn/`.
    Spawn(Common),
    /// Write the JDK's `Math`, `StrictMath` and correctly rounded results for the functions
    /// worldgen calls, for the arguments spyglass's call sites take (docs/specs/java-math.md),
    /// into `oracle-out/math/<os>-<arch>/`. The JDK's values, not the game's: any version's
    /// harness gives the same ones.
    Math(Common),
    /// Write loot golden files (every container loot table filled by the game for a fixed list
    /// of seeds) into `oracle-out/<version>/loot/`.
    Loot(Common),
    /// Write height golden files (the generator's worldgen heightmaps and block columns) for the
    /// golden seeds and dimensions into `oracle-out/<version>/heights/`.
    Heights {
        #[command(flatten)]
        common: Common,
        /// Parallel seeds. Defaults to the number of CPUs.
        #[arg(long)]
        threads: Option<usize>,
    },
    /// Write block golden files (whole chunks as generation leaves them before features) for the
    /// golden seeds and dimensions into `oracle-out/<version>/blocks/`.
    Blocks {
        #[command(flatten)]
        common: Common,
        /// Parallel chunks. Defaults to the number of CPUs.
        #[arg(long)]
        threads: Option<usize>,
        /// Only these seeds, instead of the golden ones.
        #[arg(long = "seed")]
        seeds: Vec<String>,
    },
    /// Write feature golden files (each chunk decorated on its own, with every call to a target
    /// feature) for the golden seeds and seed 56, in every dimension, into
    /// `oracle-out/<version>/features/`.
    Features {
        #[command(flatten)]
        common: Common,
        /// Parallel chunks. Defaults to the number of CPUs.
        #[arg(long)]
        threads: Option<usize>,
        /// Only these seeds, instead of the golden ones and seed 56.
        #[arg(long = "seed")]
        seeds: Vec<String>,
    },
    /// Write container golden files (the nearest start of each structure, and every chunk it
    /// reaches decorated on its own with structures, with each container decoration made) for
    /// the golden seeds, in every dimension, into `oracle-out/<version>/containers/`.
    Containers {
        #[command(flatten)]
        common: Common,
        /// Parallel chunks. Defaults to the number of CPUs.
        #[arg(long)]
        threads: Option<usize>,
        /// Only these seeds, instead of the golden ones.
        #[arg(long = "seed")]
        seeds: Vec<String>,
    },
    /// Print one column of a generated chunk beside the generator's `getBaseColumn`, marking
    /// where they differ. For finding where blocks go wrong.
    Column {
        #[command(flatten)]
        common: Common,
        /// The seed, as typed into the world-creation screen.
        #[arg(allow_hyphen_values = true)]
        seed: String,
        /// Block x.
        #[arg(allow_hyphen_values = true)]
        x: i32,
        /// Block z.
        #[arg(allow_hyphen_values = true)]
        z: i32,
        #[arg(long, default_value = "minecraft:overworld")]
        dimension: String,
    },
    /// Ask a real dedicated server `/locate` questions for a fixed case list, and write the
    /// answers into `oracle-out/<version>/locate/`. Takes several minutes; not part of `golden`.
    /// Running the server means accepting the Minecraft EULA (ADR 0033).
    Locate {
        #[command(flatten)]
        common: Common,
        /// Run on a copy of this world save instead of the seed list (repeatable). The server
        /// never opens the save itself.
        #[arg(long)]
        world: Vec<PathBuf>,
        /// Accept the Minecraft EULA (<https://aka.ms/MinecraftEULA>) for the server this runs,
        /// as `SPYGLASS_ACCEPT_EULA=1` does.
        #[arg(long)]
        accept_eula: bool,
    },
    /// Record the order the game decorates chunks in around the spawn of new worlds, with one
    /// player joined on a stand-in connection (ADR 0073), into
    /// `oracle-out/<version>/order/<seed>/`. About a minute per run; not part of `golden`.
    /// Running the server means accepting the Minecraft EULA (ADR 0033).
    Order {
        #[command(flatten)]
        common: Common,
        /// Only these seeds, instead of the golden ones (repeatable).
        #[arg(long = "seed")]
        seeds: Vec<String>,
        /// Also this many seeds from a fixed generator, the same on every machine.
        #[arg(long, default_value_t = 0)]
        random: usize,
        /// Runs per seed.
        #[arg(long, default_value_t = 2)]
        runs: usize,
        /// The JVM's processor count, which sets how many workers generate chunks. Defaults to
        /// every CPU.
        #[arg(long)]
        cpus: Option<usize>,
        /// Record startup alone, without a player joining.
        #[arg(long)]
        no_join: bool,
        /// The server's view distance, in chunks, which sets how far the join decorates.
        #[arg(long, default_value_t = 10)]
        view_distance: u32,
        /// Accept the Minecraft EULA (<https://aka.ms/MinecraftEULA>) for the server this runs,
        /// as `SPYGLASS_ACCEPT_EULA=1` does.
        #[arg(long)]
        accept_eula: bool,
    },
    /// Replay the orders `order` recorded: each chunk's target features decorated on its own
    /// and in the recorded order, into `oracle-out/<version>/order-features/<seed>/`.
    Replay {
        #[command(flatten)]
        common: Common,
        /// Parallel chunks for the decorations on their own. Defaults to the number of CPUs.
        #[arg(long)]
        threads: Option<usize>,
        /// Only these seeds' recordings (repeatable).
        #[arg(long = "seed")]
        seeds: Vec<String>,
    },
    /// Load the test data pack in `oracle/testpack/` over the vanilla one and write probe and
    /// structure files into `oracle-out/<version>+testpack/`, with the pack recorded for the
    /// comparisons to apply over the plain bundle. Checks the forms the vanilla data doesn't use
    /// (docs/specs/oracle.md).
    TestPack(Common),
    /// Write the fixture pack, which overrides a noise, a density function, the noise
    /// settings, features, loot and a biome tag, from `bundles/<version>` into
    /// `target/oracle/<version>/packs/overrides/`, then regenerate the golden set with it loaded
    /// into `oracle-out/<version>+overrides/` (docs/specs/oracle.md, "The fixture pack").
    FixturePack {
        #[command(flatten)]
        common: Common,
        /// Parallel jobs. Defaults to the number of CPUs.
        #[arg(long)]
        threads: Option<usize>,
        /// Only write the pack.
        #[arg(long)]
        only_pack: bool,
    },
    /// Regenerate the whole golden set (docs/specs/oracle.md), probe files included, into
    /// `oracle-out/<version>/`.
    Golden {
        #[command(flatten)]
        common: Common,
        /// Parallel jobs. Defaults to the number of CPUs.
        #[arg(long)]
        threads: Option<usize>,
    },
}

#[derive(Args)]
pub struct Common {
    /// Game version: an id, `latest` or `latest-snapshot`.
    #[arg(long, default_value = "latest")]
    game_version: String,
    /// Jar cache directory. Defaults to the per-user cache.
    #[arg(long)]
    cache_dir: Option<PathBuf>,
    /// JDK home or executable to use first.
    #[arg(long)]
    java: Option<PathBuf>,
    /// A data pack, a folder or zip, to load over the vanilla one, as a world that enables it
    /// does. Repeatable, lowest first; needs `--name`.
    #[arg(long = "pack", requires = "name")]
    packs: Vec<PathBuf>,
    /// The packs' label: the outputs go into `oracle-out/<version>+<name>/`, with a
    /// `packs.json` naming the packs, which the comparisons apply over the plain bundle.
    #[arg(long, requires = "packs", value_parser = variant_label)]
    name: Option<String>,
}

impl Common {
    /// `version` with the default jar cache and JDK, as `cargo xtask ci` runs it.
    pub(crate) fn for_version(version: &str) -> Self {
        Self {
            game_version: version.to_string(),
            cache_dir: None,
            java: None,
            packs: Vec::new(),
            name: None,
        }
    }
}

/// The file in a variant's outputs that names its packs (`spyglass_oracle::PACKS_FILE`).
pub(crate) const PACKS_FILE: &str = "packs.json";

/// The game version of a version name: `26.3` for `26.3+overrides`
/// (`spyglass_oracle::base_version`).
pub(crate) fn base_version(version: &str) -> &str {
    version.split_once('+').map_or(version, |(base, _)| base)
}

/// Where `cargo xtask oracle math` writes one platform's results: `oracle-out/math/<platform>/`.
pub(crate) fn math_dir(platform: &str) -> PathBuf {
    repo_root().join("oracle-out").join("math").join(platform)
}

/// The golden seed set. Text seeds are hashed by the game, as typed into the world-creation screen.
pub(crate) const GOLDEN_SEEDS: &[&str] = &[
    "0",
    "1",
    "-1",
    "-9223372036854775808",
    "9223372036854775807",
    "spyglass",
    // The two real 26.3 worlds the harness was validated against.
    "-4893098694207956070",
    "2514887864703934023",
    // Fixed random seeds.
    "-8228862705229265597",
    "-1918002950002395028",
];

/// A seed whose two dungeons near the spawn a seed search finds in every supported version, for
/// the test that checks them against the game (`docs/specs/oracle.md#feature-golden-files`).
pub(crate) const DUNGEON_SEED: &str = "56";

/// The seeds feature golden files are written for: the golden seeds and [`DUNGEON_SEED`].
pub(crate) fn feature_seeds() -> Vec<&'static str> {
    let mut seeds = GOLDEN_SEEDS.to_vec();
    seeds.push(DUNGEON_SEED);
    seeds
}

/// The seeds besides [`DUNGEON_SEED`] the two-dungeon search finds under the fixture pack,
/// whose dungeons are tried 40 times a chunk, in 26.2 and 26.3: their feature files are written
/// with the pack's golden set, so the test that checks them has its inputs.
const FIXTURE_DUNGEON_SEEDS: &[&str] = &["2", "22", "38", "69"];

/// Golden regions per seed: name, minimum block x and z, and size in blocks.
const GOLDEN_REGIONS: &[(&str, i32, i32, i32)] = &[
    ("origin", -512, -512, 1024),
    ("far-pos", 1_000_000, 1_000_000, 64),
    ("far-neg", -1_000_064, -1_000_064, 64),
    // The world border is at ±29,999,984.
    ("border-pos", 29_999_920, 29_999_920, 64),
    ("border-neg", -29_999_984, -29_999_984, 64),
];

/// The dimensions golden files are made for.
const GOLDEN_DIMENSIONS: &[&str] = &[
    "minecraft:overworld",
    "minecraft:the_nether",
    "minecraft:the_end",
];

/// A dimension's folder in `oracle-out`: its id's path, e.g. `the_nether`.
fn dimension_dir(dimension: &str) -> &str {
    dimension.rsplit(':').next().unwrap_or(dimension)
}

/// Runs `cargo xtask oracle <cmd>`: builds the harness, then has it write the files asked for.
pub fn run(cmd: OracleCmd) -> Result<()> {
    match cmd {
        OracleCmd::Build(common) => build(&common).map(drop),
        OracleCmd::Biomes {
            common,
            seed,
            dimension,
            x,
            z,
            size,
            out,
        } => {
            let harness = build(&common)?;
            let job = Job {
                seed,
                dimension,
                x,
                z,
                size,
                out: std::path::absolute(out)?,
            };
            harness.biomes(&[job], None)
        }
        OracleCmd::CheckSave {
            common,
            world,
            dimension,
        } => {
            let harness = build(&common)?;
            let world = std::path::absolute(world)?;
            harness.run(&[
                "check-save".into(),
                world.into(),
                "--dimension".into(),
                dimension.into(),
            ])
        }
        OracleCmd::Probe(common) => {
            let harness = build(&common)?;
            harness.probe()
        }
        OracleCmd::Math(common) => {
            let harness = build(&common)?;
            harness.math()
        }
        OracleCmd::Structures(common) => {
            let harness = build(&common)?;
            harness.structures()
        }
        OracleCmd::Locate {
            common,
            world,
            accept_eula,
        } => {
            crate::locate::require_eula(accept_eula)?;
            // Before the build, which records the packs as a variant to compare.
            crate::locate::check_packs(&common.packs, &world)?;
            let harness = build(&common)?;
            crate::locate::run(&harness, &world)
        }
        OracleCmd::Order {
            common,
            seeds,
            random,
            runs,
            cpus,
            no_join,
            view_distance,
            accept_eula,
        } => {
            crate::locate::require_eula(accept_eula)?;
            let mut seeds: Vec<String> = seeds_or(&seeds, GOLDEN_SEEDS.to_vec())
                .into_iter()
                .map(String::from)
                .collect();
            seeds.extend(crate::order::random_seeds(random));
            let harness = build(&common)?;
            crate::order::run(
                &harness,
                &crate::order::Options {
                    seeds: &seeds,
                    runs,
                    cpus,
                    no_join,
                    view_distance,
                },
            )
        }
        OracleCmd::Replay {
            common,
            threads,
            seeds,
        } => {
            let harness = build(&common)?;
            harness.replay(threads, &seeds)
        }
        OracleCmd::Spawn(common) => {
            let harness = build(&common)?;
            harness.spawn()
        }
        OracleCmd::Loot(common) => {
            let harness = build(&common)?;
            harness.loot()
        }
        OracleCmd::Heights { common, threads } => {
            let harness = build(&common)?;
            harness.heights(threads, GOLDEN_SEEDS)
        }
        OracleCmd::Blocks {
            common,
            threads,
            seeds,
        } => {
            let mut harness = build(&common)?;
            blocks(
                &mut harness,
                threads,
                &seeds_or(&seeds, GOLDEN_SEEDS.to_vec()),
            )
        }
        OracleCmd::Features {
            common,
            threads,
            seeds,
        } => {
            let harness = build(&common)?;
            harness.features(threads, &seeds_or(&seeds, feature_seeds()))
        }
        OracleCmd::Containers {
            common,
            threads,
            seeds,
        } => {
            let harness = build(&common)?;
            harness.containers(threads, &seeds_or(&seeds, GOLDEN_SEEDS.to_vec()))
        }
        OracleCmd::Column {
            common,
            seed,
            x,
            z,
            dimension,
        } => {
            let harness = build(&common)?;
            harness.run(&[
                "column".into(),
                seed.into(),
                x.to_string().into(),
                z.to_string().into(),
                "--dimension".into(),
                dimension.into(),
            ])
        }
        OracleCmd::TestPack(common) => test_pack(&common),
        OracleCmd::Golden { common, threads } => golden(&common, threads, &feature_seeds()),
        OracleCmd::FixturePack {
            common,
            threads,
            only_pack,
        } => fixture_pack(common, threads, only_pack),
    }
}

/// The seeds given on the command line, or `default` if none were.
fn seeds_or<'s>(seeds: &'s [String], default: Vec<&'s str>) -> Vec<&'s str> {
    if seeds.is_empty() {
        default
    } else {
        seeds.iter().map(String::as_str).collect()
    }
}

/// Every golden file: probes, structures, spawn, heights, blocks, features, containers, the
/// biome grids and loot, with feature files for `feature_seeds`.
fn golden(common: &Common, threads: Option<usize>, feature_seeds: &[&str]) -> Result<()> {
    let mut harness = build(common)?;
    harness.probe()?;
    harness.structures()?;
    harness.spawn()?;
    harness.heights(threads, GOLDEN_SEEDS)?;
    blocks(&mut harness, threads, GOLDEN_SEEDS)?;
    harness.features(threads, feature_seeds)?;
    harness.containers(threads, GOLDEN_SEEDS)?;
    harness.golden_biomes(threads)?;
    harness.loot()?;
    println!(
        "golden files in {}",
        repo_root()
            .join("oracle-out")
            .join(&harness.version)
            .display()
    );
    Ok(())
}

/// One job per golden dimension, seed and region, writing under `biomes/<dimension>/<seed>/`.
fn golden_jobs(biomes: &Path) -> Vec<Job> {
    let mut jobs = Vec::new();
    for dimension in GOLDEN_DIMENSIONS {
        let dim_dir = dimension_dir(dimension);
        for seed in GOLDEN_SEEDS {
            for &(name, x, z, size) in GOLDEN_REGIONS {
                jobs.push(Job {
                    seed: seed.to_string(),
                    dimension: dimension.to_string(),
                    x,
                    z,
                    size,
                    out: biomes.join(dim_dir).join(seed).join(format!("{name}.bin")),
                });
            }
        }
    }
    jobs
}

/// One biome grid to make.
struct Job {
    /// The seed, as the game reads it: a number, or text to hash.
    seed: String,
    /// The dimension's id.
    dimension: String,
    /// The region's west edge, in blocks.
    x: i32,
    /// The region's north edge, in blocks.
    z: i32,
    /// The region's side, in blocks.
    size: i32,
    /// The grid file to write.
    out: PathBuf,
}

/// A compiled harness, ready to run.
pub(crate) struct Harness {
    /// The game version it runs; with a suffix (`+testpack`, `+no-carvers`) for variant runs,
    /// which writes their files under that name.
    pub(crate) version: String,
    /// The game's data version, which picks the adapter.
    pub(crate) data_version: u32,
    /// The JDK's `java`.
    pub(crate) java: PathBuf,
    /// The JDK's major version.
    pub(crate) major: u32,
    /// The harness's classes and the game's unpacked jars.
    pub(crate) classpath: OsString,
    /// `target/oracle/<version>/`.
    pub(crate) work: PathBuf,
    /// Data pack folders loaded over the vanilla pack.
    pub(crate) packs: Vec<PathBuf>,
}

impl Harness {
    /// Runs the harness's `Main` with `args`, the data packs first, in
    /// `target/oracle/<version>/run/`.
    fn run(&self, args: &[OsString]) -> Result<()> {
        let run_dir = self.work.join("run");
        fs::create_dir_all(&run_dir)?;
        let mut java = Command::new(&self.java);
        // The game's own libraries use sun.misc.Unsafe; don't warn about it on every run. The
        // option exists from Java 23.
        if self.major >= 23 {
            java.arg("--sun-misc-unsafe-memory-access=allow");
        }
        let status = java
            .arg(format!(
                "-Dlog4j.configurationFile={}",
                repo_root().join("oracle").join("log4j2.xml").display()
            ))
            // The outputs' version name, `<version>+<label>` with packs, whatever the packs'
            // file names.
            .arg(format!("-Dspyglass.oracle.version={}", self.version))
            .arg("-cp")
            .arg(&self.classpath)
            .arg("spyglass.oracle.Main")
            .args(
                self.packs
                    .iter()
                    .flat_map(|p| [OsString::from("--pack"), p.into()]),
            )
            .args(args)
            // The game writes logs/ into the working directory.
            .current_dir(&run_dir)
            .status()
            .with_context(|| format!("failed to run {}", self.java.display()))?;
        if !status.success() {
            bail!("the oracle harness failed ({status})");
        }
        Ok(())
    }

    /// Writes `oracle-out/math/<os>-<arch>/`, replacing what was there.
    pub(crate) fn math(&self) -> Result<()> {
        let dir = math_dir(&format!(
            "{}-{}",
            std::env::consts::OS,
            std::env::consts::ARCH
        ));
        if dir.exists() {
            fs::remove_dir_all(&dir)?;
        }
        self.run(&["math".into(), dir.into()])
    }

    /// Writes `oracle-out/<version>/probe/`, replacing what was there.
    pub(crate) fn probe(&self) -> Result<()> {
        let dir = repo_root()
            .join("oracle-out")
            .join(&self.version)
            .join("probe");
        if dir.exists() {
            fs::remove_dir_all(&dir)?;
        }
        let mut args: Vec<OsString> = vec!["probe".into(), dir.into()];
        args.extend(GOLDEN_SEEDS.iter().map(OsString::from));
        self.run(&args)
    }

    /// Writes `oracle-out/<version>/structures/<dimension>/` for each golden dimension, replacing
    /// what was there.
    pub(crate) fn structures(&self) -> Result<()> {
        self.per_dimension("structures", &[], GOLDEN_SEEDS)
    }

    /// Writes `oracle-out/<version>/spawn/<dimension>/` for each golden dimension, replacing
    /// what was there.
    pub(crate) fn spawn(&self) -> Result<()> {
        self.per_dimension("spawn", &[], GOLDEN_SEEDS)
    }

    /// Writes `oracle-out/<version>/loot/`, replacing what was there.
    pub(crate) fn loot(&self) -> Result<()> {
        let dir = repo_root()
            .join("oracle-out")
            .join(&self.version)
            .join("loot");
        if dir.exists() {
            fs::remove_dir_all(&dir)?;
        }
        self.run(&["loot".into(), dir.into()])
    }

    /// Writes `oracle-out/<version>/heights/<dimension>/` for each golden dimension, replacing
    /// what was there.
    pub(crate) fn heights(&self, threads: Option<usize>, seeds: &[&str]) -> Result<()> {
        let mut extra: Vec<OsString> = Vec::new();
        if let Some(threads) = threads {
            extra.extend(["--threads".into(), threads.to_string().into()]);
        }
        self.per_dimension("heights", &extra, seeds)
    }

    /// Writes `oracle-out/<version>/blocks/<dimension>/` for each golden dimension, replacing
    /// what was there.
    pub(crate) fn blocks(&self, threads: Option<usize>, seeds: &[&str]) -> Result<()> {
        let mut extra: Vec<OsString> = Vec::new();
        if let Some(threads) = threads {
            extra.extend(["--threads".into(), threads.to_string().into()]);
        }
        self.per_dimension("blocks", &extra, seeds)
    }

    /// Writes `oracle-out/<version>/features/<dimension>/` for each golden dimension, replacing
    /// what was there.
    pub(crate) fn features(&self, threads: Option<usize>, seeds: &[&str]) -> Result<()> {
        let mut extra: Vec<OsString> = Vec::new();
        if let Some(threads) = threads {
            extra.extend(["--threads".into(), threads.to_string().into()]);
        }
        self.per_dimension("features", &extra, seeds)
    }

    /// Replays every order file `oracle order` recorded, or those of `seeds`, into
    /// `oracle-out/<version>/order-features/<seed>/`, each chunk with its target features' calls
    /// decorated on its own and in the recorded order.
    pub(crate) fn replay(&self, threads: Option<usize>, seeds: &[String]) -> Result<()> {
        let version_dir = repo_root().join("oracle-out").join(&self.version);
        let order_dir = version_dir.join("order");
        let mut files = Vec::new();
        for entry in fs::read_dir(&order_dir).with_context(|| {
            format!(
                "no recordings in {}; run cargo xtask oracle order first",
                order_dir.display()
            )
        })? {
            let seed_dir = entry?.path();
            let seed = seed_dir.file_name().unwrap_or_default().to_string_lossy();
            if !seed_dir.is_dir() || !(seeds.is_empty() || seeds.iter().any(|s| *s == seed)) {
                continue;
            }
            for file in fs::read_dir(&seed_dir)? {
                let file = file?.path();
                // Startup alone decorates too little to replay.
                let name = file.file_name().unwrap_or_default().to_string_lossy();
                if name.ends_with(".json") && !name.ends_with("-startup.json") {
                    files.push(file);
                }
            }
        }
        if files.is_empty() {
            bail!("no order files to replay in {}", order_dir.display());
        }
        files.sort();
        let mut args: Vec<OsString> =
            vec!["replay".into(), version_dir.join("order-features").into()];
        if let Some(threads) = threads {
            args.extend(["--threads".into(), threads.to_string().into()]);
        }
        args.extend(files.into_iter().map(OsString::from));
        self.run(&args)
    }

    /// Writes `oracle-out/<version>/containers/<dimension>/` for each golden dimension, replacing
    /// what was there.
    pub(crate) fn containers(&self, threads: Option<usize>, seeds: &[&str]) -> Result<()> {
        let mut extra: Vec<OsString> = Vec::new();
        if let Some(threads) = threads {
            extra.extend(["--threads".into(), threads.to_string().into()]);
        }
        self.per_dimension("containers", &extra, seeds)
    }

    /// Runs harness command `kind` for `seeds` once per golden dimension, into
    /// `oracle-out/<version>/<kind>/<dimension>/`, after removing `<kind>/`.
    fn per_dimension(&self, kind: &str, extra: &[OsString], seeds: &[&str]) -> Result<()> {
        let dir = repo_root()
            .join("oracle-out")
            .join(&self.version)
            .join(kind);
        if dir.exists() {
            fs::remove_dir_all(&dir)?;
        }
        for dimension in GOLDEN_DIMENSIONS {
            let mut args: Vec<OsString> = vec![
                kind.into(),
                dir.join(dimension_dir(dimension)).into(),
                "--dimension".into(),
                (*dimension).into(),
            ];
            args.extend(extra.iter().cloned());
            args.extend(seeds.iter().map(OsString::from));
            self.run(&args)?;
        }
        Ok(())
    }

    /// The golden biome grids, after removing any from an earlier seed or region set, so tests
    /// don't pick them up.
    pub(crate) fn golden_biomes(&self, threads: Option<usize>) -> Result<()> {
        let biomes = repo_root()
            .join("oracle-out")
            .join(&self.version)
            .join("biomes");
        if biomes.exists() {
            fs::remove_dir_all(&biomes)?;
        }
        self.biomes(&golden_jobs(&biomes), threads)
    }

    /// Makes the biome grids of `jobs`: written to a jobs file, a line each, which the
    /// harness works through on `threads` threads.
    fn biomes(&self, jobs: &[Job], threads: Option<usize>) -> Result<()> {
        let mut lines = String::new();
        for job in jobs {
            // Job lines are whitespace-separated.
            if job.seed.trim().is_empty() || job.seed.contains(char::is_whitespace) {
                bail!(
                    "seeds containing whitespace aren't supported: {:?}",
                    job.seed
                );
            }
            if job.x % 16 != 0 || job.z % 16 != 0 || job.size % 16 != 0 || job.size <= 0 {
                bail!("regions must be chunk-aligned: x, z and size multiples of 16");
            }
            // Harness job lines are in quarts (4 blocks).
            writeln!(
                lines,
                "{} {} {} {} {} {} {}",
                job.seed,
                job.dimension,
                job.x / 4,
                job.z / 4,
                job.size / 4,
                job.size / 4,
                job.out.display()
            )?;
        }
        let jobs_file = self.work.join("jobs.txt");
        fs::write(&jobs_file, lines)?;
        let mut args: Vec<OsString> = vec!["biomes".into(), jobs_file.into()];
        if let Some(threads) = threads {
            args.extend(["--threads".into(), threads.to_string().into()]);
        }
        self.run(&args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_flags_a_command_refuses_are_refused_before_anything_is_written() {
        let mut common = Common::for_version("26.3");
        common.packs = vec![PathBuf::from("a-pack")];
        common.name = Some("refused-before-build".to_string());
        let out = repo_root()
            .join("oracle-out")
            .join("26.3+refused-before-build");
        assert!(test_pack(&common).is_err());
        assert!(!out.exists());
        let worlds = [PathBuf::from("a-world")];
        assert!(crate::locate::check_packs(&common.packs, &worlds).is_err());
        assert!(crate::locate::check_packs(&common.packs, &[]).is_ok());
        assert!(crate::locate::check_packs(&[], &worlds).is_ok());
    }

    #[test]
    fn base_versions_drop_the_label() {
        assert_eq!(base_version("26.3"), "26.3");
        assert_eq!(base_version("26.3+overrides"), "26.3");
        assert_eq!(base_version("26.3+overrides+no-carvers"), "26.3");
    }
}
