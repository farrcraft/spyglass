//! `cargo xtask oracle order`: the order the game decorates chunks in around the spawn of a new
//! world, with one player joined.
//!
//! Each run starts the stock server in the harness's JVM through `spyglass.oracle.Join`, which
//! joins a player on a stand-in connection and writes the `FEATURES` steps from the game's own
//! JFR events (docs/adr/0073-the-oracle-joins-a-stand-in-player.md). See docs/specs/oracle.md.

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

use anyhow::{Context, Result, bail};

use crate::oracle::Harness;
use crate::repo_root;

/// What to record.
pub struct Options<'a> {
    /// The seeds, as typed into the world-creation screen.
    pub seeds: &'a [String],
    /// Runs per seed.
    pub runs: usize,
    /// The JVM's processor count (`-XX:ActiveProcessorCount`), or all of them.
    pub cpus: Option<usize>,
    /// Record startup alone, without a join.
    pub no_join: bool,
    /// The server's view distance, in chunks.
    pub view_distance: u32,
}

/// `count` seeds from a fixed generator (SplitMix64 from a fixed start), so that `--random N`
/// names the same seeds on every machine. Each is the first `count` of the same sequence.
pub fn random_seeds(count: usize) -> Vec<String> {
    let mut state: u64 = 0x5370_7967_6c61_7373; // "Spyglass"
    (0..count)
        .map(|_| {
            state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
            let mut z = state;
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
            z ^= z >> 31;
            (z.cast_signed()).to_string()
        })
        .collect()
}

/// Records every run of every seed into `oracle-out/<version>/order/<seed>/<run name>.json`,
/// replacing a run's file if it's there.
pub fn run(harness: &Harness, options: &Options<'_>) -> Result<()> {
    if !harness.packs.is_empty() {
        bail!("oracle order doesn't take --pack: it records vanilla worlds");
    }
    let out = repo_root()
        .join("oracle-out")
        .join(&harness.version)
        .join("order");
    for seed in options.seeds {
        for run in 1..=options.runs {
            let name = run_name(run, options);
            let file = out.join(seed).join(format!("{name}.json"));
            let started = Instant::now();
            record(harness, seed, &name, &file, options)?;
            println!(
                "{}: {:.0} s",
                file.display(),
                started.elapsed().as_secs_f64()
            );
        }
    }
    Ok(())
}

/// A run's file name: its number, the processor count if it's set, and `startup` without a
/// join, so runs of different kinds sit side by side.
fn run_name(run: usize, options: &Options<'_>) -> String {
    let mut name = format!("run{run}");
    if let Some(cpus) = options.cpus {
        name.push_str(&format!("-cpus{cpus}"));
    }
    if options.no_join {
        name.push_str("-startup");
    }
    name
}

/// One run: a fresh server folder, `Join` in it, and its order file. The folder is deleted once
/// the file is written, since each holds a whole world; a failed run's is kept, with its log.
fn record(
    harness: &Harness,
    seed: &str,
    name: &str,
    file: &Path,
    options: &Options<'_>,
) -> Result<()> {
    let dir = server_dir(harness, seed, name, options.view_distance)?;
    let log = dir.join("join.log");
    let mut java = Command::new(&harness.java);
    java.args([
        "-Xmx4G",
        "--enable-native-access=ALL-UNNAMED",
        // Join finds the server through the server thread's task (ADR 0073).
        "--add-opens",
        "java.base/java.lang=ALL-UNNAMED",
    ]);
    // The game's libraries use sun.misc.Unsafe; the option exists from Java 23.
    if harness.major >= 23 {
        java.arg("--sun-misc-unsafe-memory-access=allow");
    }
    if let Some(cpus) = options.cpus {
        java.arg(format!("-XX:ActiveProcessorCount={cpus}"));
    }
    java.arg(format!("-Dspyglass.oracle.version={}", harness.version))
        .arg("-cp")
        .arg(&harness.classpath)
        .arg("spyglass.oracle.Join")
        .arg(file);
    if options.no_join {
        java.arg("--no-join");
    }
    let status = java
        .args(["--", "--nogui", "--jfrProfile"])
        .current_dir(&dir)
        .stdin(Stdio::null())
        .stdout(File::create(&log)?)
        .stderr(Stdio::inherit())
        .status()
        .with_context(|| format!("failed to run {}", harness.java.display()))?;
    if !status.success() {
        let text = fs::read_to_string(&log).unwrap_or_default();
        let lines: Vec<&str> = text.lines().collect();
        let tail = lines[lines.len().saturating_sub(20)..].join("\n");
        bail!(
            "the join run for seed {seed} failed ({status}); {} ends:\n{tail}",
            log.display()
        );
    }
    fs::remove_dir_all(&dir).with_context(|| format!("failed to remove {}", dir.display()))?;
    Ok(())
}

/// A fresh server folder for one run: the EULA and properties, with the seed and view distance.
fn server_dir(harness: &Harness, seed: &str, name: &str, view_distance: u32) -> Result<PathBuf> {
    let dir = harness.work.join("order-server").join(seed).join(name);
    if dir.exists() {
        fs::remove_dir_all(&dir)?;
    }
    fs::create_dir_all(&dir)?;
    // Accepted by whoever ran this: `require_eula` checked before any server started.
    fs::write(dir.join("eula.txt"), "eula=true\n")?;
    // As for locate: any free port, no pause when empty, no watchdog. The view distance sets
    // how far the join decorates.
    fs::write(
        dir.join("server.properties"),
        format!(
            "level-seed={seed}\n\
             view-distance={view_distance}\n\
             server-ip=127.0.0.1\n\
             server-port=0\n\
             online-mode=false\n\
             pause-when-empty-seconds=-1\n\
             max-tick-time=-1\n\
             sync-chunk-writes=false\n\
             spawn-protection=0\n"
        ),
    )?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_seeds_are_fixed_and_prefixes_of_each_other() {
        let five = random_seeds(5);
        assert_eq!(five.len(), 5);
        assert_eq!(random_seeds(2), five[..2]);
        let unique: std::collections::BTreeSet<_> = five.iter().collect();
        assert_eq!(unique.len(), 5);
    }

    #[test]
    fn run_names_say_what_differs() {
        let seeds = [];
        let mut options = Options {
            seeds: &seeds,
            runs: 2,
            cpus: None,
            no_join: false,
            view_distance: 10,
        };
        assert_eq!(run_name(1, &options), "run1");
        options.cpus = Some(2);
        options.no_join = true;
        assert_eq!(run_name(2, &options), "run2-cpus2-startup");
    }
}
