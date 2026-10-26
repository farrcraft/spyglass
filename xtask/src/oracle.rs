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
    /// positions, slime chunks) for the golden seeds into `oracle-out/<version>/structures/`.
    Structures(Common),
    /// Write spawn golden files (the spawn origin and the climate search behind it) for the
    /// golden seeds into `oracle-out/<version>/spawn/`.
    Spawn(Common),
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

const GOLDEN_DIMENSIONS: &[&str] = &["minecraft:overworld"];

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
        OracleCmd::Golden { common, threads } => {
            let harness = build(&common)?;
            harness.probe()?;
            harness.structures()?;
            harness.spawn()?;
            let dir = repo_root().join("oracle-out").join(&harness.version);
            // Leave nothing from an earlier seed or region set for tests to pick up.
            let biomes = dir.join("biomes");
            if biomes.exists() {
                fs::remove_dir_all(&biomes)?;
            }
            let mut jobs = Vec::new();
            for dimension in GOLDEN_DIMENSIONS {
                let dim_dir = dimension.rsplit(':').next().unwrap_or(dimension);
                for seed in GOLDEN_SEEDS {
                    for &(name, x, z, size) in GOLDEN_REGIONS {
                        jobs.push(Job {
                            seed: seed.to_string(),
                            dimension: dimension.to_string(),
                            x,
                            z,
                            size,
                            out: dir
                                .join("biomes")
                                .join(dim_dir)
                                .join(seed)
                                .join(format!("{name}.bin")),
                        });
                    }
                }
            }
            harness.biomes(&jobs, threads)?;
            println!("golden files in {}", dir.display());
            Ok(())
        }
    }
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

    /// Writes `oracle-out/<version>/structures/overworld/`, replacing what was there.
    fn structures(&self) -> Result<()> {
        let dir = repo_root()
            .join("oracle-out")
            .join(&self.version)
            .join("structures");
        if dir.exists() {
            fs::remove_dir_all(&dir)?;
        }
        let mut args: Vec<OsString> = vec!["structures".into(), dir.join("overworld").into()];
        args.extend(GOLDEN_SEEDS.iter().map(OsString::from));
        self.run(&args)
    }

    /// Writes `oracle-out/<version>/spawn/overworld/`, replacing what was there.
    fn spawn(&self) -> Result<()> {
        let dir = repo_root()
            .join("oracle-out")
            .join(&self.version)
            .join("spawn");
        if dir.exists() {
            fs::remove_dir_all(&dir)?;
        }
        let mut args: Vec<OsString> = vec!["spawn".into(), dir.join("overworld").into()];
        args.extend(GOLDEN_SEEDS.iter().map(OsString::from));
        self.run(&args)
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
    compile(&bin, major, &server.sha1, &jars, &classes)?;

    let classpath = std::env::join_paths(std::iter::once(classes).chain(jars))?;
    Ok(Harness {
        version: resolved.version,
        java: jdk.executable,
        major,
        classpath,
        work,
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

/// Compiles `oracle/src` into `classes` unless the sources, jar and JDK are unchanged since the
/// last build.
fn compile(
    jdk_bin: &Path,
    major: u32,
    jar_sha1: &str,
    jars: &[PathBuf],
    classes: &Path,
) -> Result<()> {
    let src = repo_root().join("oracle").join("src");
    let mut sources = Vec::new();
    collect_java(&src, &mut sources)?;
    sources.sort();
    if sources.is_empty() {
        bail!("no Java sources under {}", src.display());
    }

    let mut fingerprint = format!("jar {jar_sha1}\njdk {}\n", jdk_bin.display());
    for source in &sources {
        writeln!(
            fingerprint,
            "{} {}",
            source.strip_prefix(&src)?.display(),
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
        .args(["--release", &major.to_string(), "-Xlint:all", "-Werror"])
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
