//! `cargo xtask java-report`: compiles the extractor's block behaviour report,
//! `extractor-java/BlockReport.java`, into the committed `BlockReport.class`, or with `--check`,
//! fails if the committed class isn't what the source compiles to (ADR 0054).

use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use spyglass_extract::java;

use crate::repo_root;

/// The JDK major whose `javac` builds the class: its bytes depend on the compiler.
const JAVAC_MAJOR: u32 = 25;

/// The class file version it targets: the oldest Java the game versions spyglass supports run on.
const RELEASE: &str = "21";

#[derive(clap::Args)]
pub struct JavaReport {
    /// Only check that the committed class matches its source; skipped without a JDK 25.
    #[arg(long)]
    check: bool,
}

pub fn run(args: &JavaReport) -> Result<()> {
    let dir = repo_root().join("extractor-java");
    let committed = dir.join("BlockReport.class");
    let Some(jdk) = java::discover(None)
        .into_iter()
        .find(|j| j.is_jdk && j.major == JAVAC_MAJOR)
    else {
        if args.check {
            println!(
                "skipped: no JDK {JAVAC_MAJOR} to compile extractor-java/BlockReport.java with"
            );
            return Ok(());
        }
        bail!(
            "no JDK {JAVAC_MAJOR} found; scripts/setup.ps1 -Oracle (Windows) or scripts/setup.sh \
             --oracle installs one"
        );
    };
    let out = tempfile::tempdir()?;
    compile(
        &jdk.home.join("bin"),
        &dir.join("BlockReport.java"),
        out.path(),
    )?;
    let built = fs::read(out.path().join("BlockReport.class"))?;
    if args.check {
        let current = fs::read(&committed)
            .with_context(|| format!("failed to read {}", committed.display()))?;
        if current != built {
            bail!(
                "extractor-java/BlockReport.class isn't what BlockReport.java compiles to; run \
                 `cargo xtask java-report` and commit the class"
            );
        }
        return Ok(());
    }
    fs::write(&committed, built)?;
    println!("wrote {}", committed.display());
    Ok(())
}

/// Compiles `source` with the JDK in `jdk_bin` into the folder `out`.
fn compile(jdk_bin: &Path, source: &Path, out: &Path) -> Result<()> {
    let javac = jdk_bin.join(if cfg!(windows) { "javac.exe" } else { "javac" });
    let output = Command::new(&javac)
        .args([
            "--release",
            RELEASE,
            "-encoding",
            "UTF-8",
            "-Xlint:all",
            "-Werror",
            "-d",
        ])
        .arg(out)
        .arg(source)
        .output()
        .with_context(|| format!("failed to run {}", javac.display()))?;
    if !output.status.success() {
        bail!(
            "javac failed:\n{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}
