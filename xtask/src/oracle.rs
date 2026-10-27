//! `cargo xtask oracle`: build and run the Java oracle harness in `oracle/`.
//! See docs/adr/0004-oracle-standalone-harness.md and docs/specs/oracle.md.

use std::ffi::OsString;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use spyglass_extract::bundler::Bundler;
use spyglass_extract::cache::{self, JarCache};
use spyglass_extract::{extract, java};

use crate::repo_root;

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
    /// Write height golden files (the generator's worldgen heightmaps and block columns) for the
    /// golden seeds and dimensions into `oracle-out/<version>/heights/`.
    Heights {
        #[command(flatten)]
        common: Common,
        /// Parallel seeds. Defaults to the number of CPUs.
        #[arg(long)]
        threads: Option<usize>,
    },
    /// Ask a real dedicated server `/locate` questions for a fixed case list, and write the
    /// answers into `oracle-out/<version>/locate/`. Takes several minutes; not part of `golden`.
    Locate {
        #[command(flatten)]
        common: Common,
        /// Run on a copy of this world save instead of the seed list (repeatable). The server
        /// never opens the save itself.
        #[arg(long)]
        world: Vec<PathBuf>,
    },
    /// Load the test data pack in `oracle/testpack/` over the vanilla one and write probe and
    /// structure files into `oracle-out/<version>+testpack/`, with the bundle merged with the
    /// pack in `target/oracle/bundles/<version>+testpack/` for the comparisons. Checks the forms
    /// the vanilla data doesn't use (docs/specs/oracle.md).
    TestPack(Common),
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
}

/// The golden seed set. Text seeds are hashed by the game, as typed into the world-creation screen.
const GOLDEN_SEEDS: &[&str] = &[
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

/// Golden regions per seed: name, minimum block x and z, and size in blocks.
const GOLDEN_REGIONS: &[(&str, i32, i32, i32)] = &[
    ("origin", -512, -512, 1024),
    ("far-pos", 1_000_000, 1_000_000, 64),
    ("far-neg", -1_000_064, -1_000_064, 64),
    // The world border is at ±29,999,984.
    ("border-pos", 29_999_920, 29_999_920, 64),
    ("border-neg", -29_999_984, -29_999_984, 64),
];

const GOLDEN_DIMENSIONS: &[&str] = &[
    "minecraft:overworld",
    "minecraft:the_nether",
    "minecraft:the_end",
];

/// A dimension's folder in `oracle-out`: its id's path, e.g. `the_nether`.
fn dimension_dir(dimension: &str) -> &str {
    dimension.rsplit(':').next().unwrap_or(dimension)
}

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
        OracleCmd::Structures(common) => {
            let harness = build(&common)?;
            harness.structures()
        }
        OracleCmd::Locate { common, world } => {
            let harness = build(&common)?;
            crate::locate::run(&harness, &world)
        }
        OracleCmd::Spawn(common) => {
            let harness = build(&common)?;
            harness.spawn()
        }
        OracleCmd::Heights { common, threads } => {
            let harness = build(&common)?;
            harness.heights(threads)
        }
        OracleCmd::TestPack(common) => test_pack(&common),
        OracleCmd::Golden { common, threads } => golden(&common, threads),
    }
}

/// The test pack's probes and structures, under the version name `<version>+testpack`, and the
/// bundle merged with the pack that spyglass's comparisons read.
fn test_pack(common: &Common) -> Result<()> {
    let mut harness = build(common)?;
    let pack = repo_root().join("oracle").join(TEST_PACK);
    let variant = format!("{}+{TEST_PACK}", harness.version);
    let bundle = merge_bundle(&harness.version, &pack, &variant)?;
    harness.version = variant;
    harness.packs = vec![pack];
    harness.probe()?;
    harness.structures()?;
    println!(
        "test pack files in {}, bundle in {}",
        repo_root()
            .join("oracle-out")
            .join(&harness.version)
            .display(),
        bundle.display()
    );
    Ok(())
}

/// The test data pack's folder in `oracle/`, and the suffix of its version name.
const TEST_PACK: &str = "testpack";

/// `bundles/<version>` with `pack`'s `data/` files added, as `target/oracle/bundles/<variant>/`,
/// its manifest naming `variant` and listing the added files. Rebuilt every time.
fn merge_bundle(version: &str, pack: &Path, variant: &str) -> Result<PathBuf> {
    let source = repo_root().join("bundles").join(version);
    let manifest_path = source.join("bundle.json");
    let mut manifest: serde_json::Value = serde_json::from_slice(
        &fs::read(&manifest_path)
            .with_context(|| format!("no bundle for {version}; extract it first"))?,
    )?;
    let out = repo_root()
        .join("target")
        .join("oracle")
        .join("bundles")
        .join(variant);
    if out.exists() {
        fs::remove_dir_all(&out)?;
    }
    let files = manifest["files"]
        .as_object_mut()
        .context("bundle.json has no files")?;
    for path in files.keys() {
        let to = out.join(path);
        fs::create_dir_all(to.parent().expect("a file in the bundle"))?;
        fs::copy(source.join(path), &to)?;
    }
    let mut added = Vec::new();
    collect_files(&pack.join("data"), &mut added)?;
    for file in added {
        let rel = file
            .strip_prefix(pack)?
            .to_str()
            .context("a non-UTF-8 pack path")?
            .replace('\\', "/");
        let to = out.join(&rel);
        fs::create_dir_all(to.parent().expect("a file in the pack"))?;
        fs::copy(&file, &to)?;
        let hash = spyglass_data::bundle::sha1_hex(fs::File::open(&file)?)?;
        files.insert(rel, hash.into());
    }
    manifest["version"] = variant.into();
    fs::write(
        out.join("bundle.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    Ok(out)
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_files(&path, out)?;
        } else {
            out.push(path);
        }
    }
    Ok(())
}

/// Every golden file: probes, structures, spawn, heights, and the biome grids.
fn golden(common: &Common, threads: Option<usize>) -> Result<()> {
    let harness = build(common)?;
    harness.probe()?;
    harness.structures()?;
    harness.spawn()?;
    harness.heights(threads)?;
    let dir = repo_root().join("oracle-out").join(&harness.version);
    // Leave nothing from an earlier seed or region set for tests to pick up.
    let biomes = dir.join("biomes");
    if biomes.exists() {
        fs::remove_dir_all(&biomes)?;
    }
    harness.biomes(&golden_jobs(&biomes), threads)?;
    println!("golden files in {}", dir.display());
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

struct Job {
    seed: String,
    dimension: String,
    x: i32,
    z: i32,
    size: i32,
    out: PathBuf,
}

/// A compiled harness, ready to run.
pub(crate) struct Harness {
    pub(crate) version: String,
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

    /// Writes `oracle-out/<version>/probe/`, replacing what was there.
    fn probe(&self) -> Result<()> {
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
    fn structures(&self) -> Result<()> {
        self.per_dimension("structures", &[])
    }

    /// Writes `oracle-out/<version>/spawn/<dimension>/` for each golden dimension, replacing
    /// what was there.
    fn spawn(&self) -> Result<()> {
        self.per_dimension("spawn", &[])
    }

    /// Writes `oracle-out/<version>/heights/<dimension>/` for each golden dimension, replacing
    /// what was there.
    fn heights(&self, threads: Option<usize>) -> Result<()> {
        let mut extra: Vec<OsString> = Vec::new();
        if let Some(threads) = threads {
            extra.extend(["--threads".into(), threads.to_string().into()]);
        }
        self.per_dimension("heights", &extra)
    }

    /// Runs harness command `kind` for the golden seeds once per golden dimension, into
    /// `oracle-out/<version>/<kind>/<dimension>/`, after removing `<kind>/`.
    fn per_dimension(&self, kind: &str, extra: &[OsString]) -> Result<()> {
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
            args.extend(GOLDEN_SEEDS.iter().map(OsString::from));
            self.run(&args)?;
        }
        Ok(())
    }

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
        if let Some(n) = threads {
            args.extend(["--threads".into(), n.to_string().into()]);
        }
        self.run(&args)
    }
}

fn build(common: &Common) -> Result<Harness> {
    let resolved = extract::resolve(&common.game_version)?;
    let (server, major) = (&resolved.download, resolved.java_major);

    let cache = match &common.cache_dir {
        Some(dir) => JarCache::new(dir),
        None => JarCache::open_default()?,
    };
    println!("server jar for {}: {}", resolved.version, server.url);
    let jar = cache.get(server)?;

    let work = repo_root()
        .join("target")
        .join("oracle")
        .join(&resolved.version);
    let jars = unpack(&jar, &server.sha1, &work.join("classpath"))?;

    let jdk = java::discover(common.java.as_deref())
        .into_iter()
        .find(|j| j.major == major && j.is_jdk)
        .with_context(|| {
            format!(
                "no JDK {major} found; scripts/setup.ps1 -Oracle (Windows) or scripts/setup.sh --oracle installs one"
            )
        })?;
    let bin = jdk.home.join("bin");
    let classes = work.join("classes");
    let game_jar = jars
        .iter()
        .find(|p| p.components().any(|c| c.as_os_str() == "versions"))
        .context("the bundler has no game jar under versions/")?;
    let data_version = spyglass_extract::write::game_version(game_jar)?.world_version;
    let adapter = adapter(data_version)?;
    compile(&bin, major, &server.sha1, &jars, &adapter, &classes)?;

    let classpath = std::env::join_paths(std::iter::once(classes).chain(jars))?;
    Ok(Harness {
        version: resolved.version,
        java: jdk.executable,
        major,
        classpath,
        work,
        packs: Vec::new(),
    })
}

/// Unpacks the bundler into `dest` unless it already holds this jar's libraries.
fn unpack(jar: &Path, sha1: &str, dest: &Path) -> Result<Vec<PathBuf>> {
    let stamp = dest.join("jar.sha1");
    let list = dest.join("classpath.txt");
    if fs::read_to_string(&stamp).is_ok_and(|s| s.trim() == sha1)
        && let Ok(text) = fs::read_to_string(&list)
    {
        return Ok(text.lines().map(PathBuf::from).collect());
    }
    if dest.exists() {
        fs::remove_dir_all(dest)?;
    }
    println!("unpacking {} into {}", jar.display(), dest.display());
    let classpath = Bundler::open(jar)?.unpack(dest)?;
    let jars: Vec<PathBuf> = classpath.jars().map(Path::to_path_buf).collect();
    let text: String = jars.iter().map(|p| format!("{}\n", p.display())).collect();
    fs::write(&list, text)?;
    fs::write(&stamp, sha1)?;
    Ok(jars)
}

/// The adapter in `oracle/adapters/` whose data-version ranges hold `data_version` (ADR 0027).
/// Each adapter lists its ranges in a `data-versions` file, one `from..` or `from..until` per
/// line (`until` exclusive), with `#` comments.
fn adapter(data_version: u32) -> Result<PathBuf> {
    let dir = repo_root().join("oracle").join("adapters");
    let mut found = Vec::new();
    for entry in fs::read_dir(&dir).with_context(|| format!("reading {}", dir.display()))? {
        let path = entry?.path();
        let ranges = path.join("data-versions");
        if !ranges.is_file() {
            continue;
        }
        let text = fs::read_to_string(&ranges)?;
        for line in text
            .lines()
            .map(|l| l.split('#').next().unwrap_or("").trim())
        {
            if line.is_empty() {
                continue;
            }
            let (from, until) = line
                .split_once("..")
                .with_context(|| format!("{}: {line:?} isn't from..[until]", ranges.display()))?;
            let from: u32 = from.trim().parse()?;
            let until: Option<u32> = match until.trim() {
                "" => None,
                n => Some(n.parse()?),
            };
            if data_version >= from && until.is_none_or(|u| data_version < u) {
                found.push(path.clone());
                break;
            }
        }
    }
    match found.as_slice() {
        [one] => Ok(one.clone()),
        [] => bail!(
            "no oracle adapter covers data version {data_version}; add one to {} (ADR 0027)",
            dir.display()
        ),
        many => bail!(
            "several oracle adapters cover data version {data_version}: {}",
            many.iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// An adapter's Java files with its bases' (ADR 0029): an `extends` file names the base adapter's
/// directory, and a file in the adapter replaces the base's file of the same relative path.
fn adapter_sources(adapter: &Path) -> Result<Vec<PathBuf>> {
    let mut chain = vec![adapter.to_path_buf()];
    loop {
        let extends = chain.last().expect("never empty").join("extends");
        if !extends.is_file() {
            break;
        }
        let base = adapter
            .parent()
            .expect("adapters live in oracle/adapters")
            .join(fs::read_to_string(&extends)?.trim());
        if chain.contains(&base) || !base.is_dir() {
            bail!("{} names no adapter, or a cycle", extends.display());
        }
        chain.push(base);
    }
    let mut by_name = std::collections::BTreeMap::new();
    for dir in chain.iter().rev() {
        let mut files = Vec::new();
        collect_java(dir, &mut files)?;
        for file in files {
            by_name.insert(file.strip_prefix(dir)?.to_path_buf(), file);
        }
    }
    Ok(by_name.into_values().collect())
}

/// Compiles `oracle/src` with `adapter` into `classes` unless the sources, adapter, jar and JDK
/// are unchanged since the last build.
fn compile(
    jdk_bin: &Path,
    major: u32,
    jar_sha1: &str,
    jars: &[PathBuf],
    adapter: &Path,
    classes: &Path,
) -> Result<()> {
    let oracle = repo_root().join("oracle");
    let mut sources = Vec::new();
    collect_java(&oracle.join("src"), &mut sources)?;
    sources.extend(adapter_sources(adapter)?);
    sources.sort();
    if sources.is_empty() {
        bail!("no Java sources under {}", oracle.display());
    }

    let mut fingerprint = format!("jar {jar_sha1}\njdk {}\n", jdk_bin.display());
    for source in &sources {
        writeln!(
            fingerprint,
            "{} {}",
            source.strip_prefix(&oracle)?.display(),
            cache::sha1_hex(source)?
        )?;
    }
    let stamp = classes.join("build.stamp");
    if fs::read_to_string(&stamp).is_ok_and(|s| s == fingerprint) {
        return Ok(());
    }

    if classes.exists() {
        fs::remove_dir_all(classes)?;
    }
    fs::create_dir_all(classes)?;
    println!("compiling the oracle harness");
    let javac = jdk_bin.join(java::JAVAC_EXE);
    let status = Command::new(&javac)
        // `classfile` warns about annotations the game's classes refer to but don't ship (JetBrains'
        // `Contract`, from 26.4), which say nothing about the harness.
        .args([
            "--release",
            &major.to_string(),
            "-Xlint:all,-classfile",
            "-Werror",
        ])
        .arg("-d")
        .arg(classes)
        .arg("-cp")
        .arg(std::env::join_paths(jars)?)
        .args(&sources)
        .status()
        .with_context(|| format!("failed to run {}", javac.display()))?;
    if !status.success() {
        bail!("javac failed");
    }
    fs::write(&stamp, fingerprint)?;
    Ok(())
}

fn collect_java(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let path = entry?.path();
        if path.is_dir() {
            collect_java(&path, out)?;
        } else if path.extension().is_some_and(|e| e == "java") {
            out.push(path);
        }
    }
    Ok(())
}
