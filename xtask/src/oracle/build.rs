//! Building the harness: the server jar fetched and unpacked, the adapter for its data
//! version chosen, and the harness compiled against it.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use spyglass_extract::bundler::Bundler;
use spyglass_extract::cache::{self, JarCache};
use spyglass_extract::{extract, java};

use super::packs::{TEST_PACK, use_packs};
use super::{Common, Harness};
use crate::repo_root;

/// Builds the harness for `common`'s version: resolves the version, fetches and unpacks its
/// server jar (both kept between runs), finds a JDK of the major version the game needs, picks
/// the adapter by the game's data version, and compiles `oracle/src` with it against the game's
/// jars, unless nothing changed since the last build.
pub(crate) fn build(common: &Common) -> Result<Harness> {
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
    let mut harness = Harness {
        version: resolved.version,
        data_version,
        java: jdk.executable,
        major,
        classpath,
        work,
        packs: Vec::new(),
    };
    if let Some(label) = &common.name {
        use_packs(&mut harness, &common.packs, label)?;
    }
    Ok(harness)
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

/// Whether a `data-versions` file holds `data_version`: one `from..` or `from..until` per line
/// (`until` exclusive), with `#` comments.
fn covers(ranges: &Path, data_version: u32) -> Result<bool> {
    let text =
        fs::read_to_string(ranges).with_context(|| format!("reading {}", ranges.display()))?;
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
            return Ok(true);
        }
    }
    Ok(false)
}

/// Whether the test pack loads in `data_version`: its `data-versions` file holds it. The pack
/// uses forms older versions reject.
pub(crate) fn test_pack_covers(data_version: u32) -> Result<bool> {
    covers(
        &repo_root()
            .join("oracle")
            .join(TEST_PACK)
            .join("data-versions"),
        data_version,
    )
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
        if ranges.is_file() && covers(&ranges, data_version)? {
            found.push(path);
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

/// An adapter and its bases, nearest first (ADR 0029): an `extends` file names the base
/// adapter's directory.
fn adapter_chain(adapter: &Path) -> Result<Vec<PathBuf>> {
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
    Ok(chain)
}

/// The adapter directories `data_version`'s harness is built from, its own and its bases', as
/// paths from the repo root (`oracle/adapters/26.3`).
pub(crate) fn adapter_dirs(data_version: u32) -> Result<Vec<String>> {
    let root = repo_root();
    adapter_chain(&adapter(data_version)?)?
        .iter()
        .map(|dir| Ok(dir.strip_prefix(root)?.to_string_lossy().replace('\\', "/")))
        .collect()
}

/// An adapter's Java files with its bases' (ADR 0029): a file in the adapter replaces the base's
/// file of the same relative path.
fn adapter_sources(adapter: &Path) -> Result<Vec<PathBuf>> {
    let chain = adapter_chain(adapter)?;
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

/// Every `.java` file under `dir`, into `out`.
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
