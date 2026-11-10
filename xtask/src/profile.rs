//! `cargo xtask profile`: the bench's lines under Linux `perf`, in a privileged Docker container
//! made from the dev container's base image. Windows' sampling profilers need an elevated
//! shell; `perf` in Docker Desktop's Linux VM doesn't.
//!
//! The container is kept between runs, with the toolchain, the build and `perf` in it, so a
//! second profile only rebuilds what changed. Its sources are this working tree, mounted
//! read-only, so a profile is of the code as it is now, committed or not.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::process::Output;

use anyhow::{Context, Result, bail, ensure};
use clap::Args;

use crate::linux_check::{IMAGE, build_image, pinned_channel, sh_quote};
use crate::{command, first_line, repo_root};

/// The container, kept running between profiles.
const CONTAINER: &str = "spyglass-profile";
/// The volume holding the container's `/root`: rustup, cargo, the build and the recordings.
const VOLUME: &str = "spyglass-profile-root";
/// Where the build goes in the container, off the read-only mount.
const TARGET_DIR: &str = "/root/target";

#[derive(Args)]
pub struct Profile {
    /// Game version: a bundle in `bundles/`.
    version: String,
    /// The bench lines to profile: those whose names contain this text (the bench's `--only`).
    #[arg(long)]
    only: String,
    /// How many times each line runs (the bench's `--runs`), so a short line outweighs the
    /// world loading on first use.
    #[arg(long)]
    runs: Option<usize>,
    /// Milliseconds after starting before sampling begins, to leave out opening the world.
    #[arg(long, default_value_t = 1500)]
    delay: u32,
    /// Samples a second.
    #[arg(long, default_value_t = 1999)]
    frequency: u32,
    /// Also show the callers of functions whose names contain this text, by share of samples.
    #[arg(long)]
    callers: Option<String>,
    /// Frames of callers to show for each.
    #[arg(long, default_value_t = 3)]
    depth: usize,
}

pub fn run(args: &Profile) -> Result<()> {
    let bundle = repo_root().join("bundles").join(&args.version);
    ensure!(
        bundle.join("bundle.json").is_file(),
        "no bundle at {}; extract {} first",
        bundle.display(),
        args.version
    );
    if first_line("docker", &["version", "--format", "{{.Server.Version}}"]).is_none() {
        bail!("Docker isn't running; profiles run in a Linux container");
    }
    build_image()?;
    start()?;
    set_up(&pinned_channel()?)?;

    println!("building the bench");
    must(&format!(
        "cd /src && CARGO_TARGET_DIR={TARGET_DIR} \
         CARGO_PROFILE_DIST_DEBUG=line-tables-only RUSTFLAGS='-C force-frame-pointers=yes' \
         cargo build --locked --quiet --profile dist -p spyglass --example bench"
    ))?;
    // A copy inside the container, so reading the bundle through the Windows mount doesn't
    // show up in the samples.
    let version = sh_quote(&args.version);
    must(&format!(
        "mkdir -p /root/bundles /root/prof && rm -rf /root/bundles/{version} && \
         cp -r /src/bundles/{version} /root/bundles/"
    ))?;

    let name = slug(&format!("{}-{}", args.version, args.only));
    let runs = args.runs.map(|n| format!("--runs {n}")).unwrap_or_default();
    println!("recording");
    must(&format!(
        "cd /root/prof && $(perf_path) record -q -e cpu-clock -F {freq} -g -D {delay} \
         -o {name}.data -- {TARGET_DIR}/dist/examples/bench /root/bundles/{version} \
         --only {only} {runs}",
        freq = args.frequency,
        delay = args.delay,
        only = sh_quote(&args.only),
    ))?;

    println!("\n--- self time");
    print_report(&name, "--no-children --percent-limit 0.8", 40)?;
    println!("\n--- inclusive time");
    print_report(&name, "--children --percent-limit 4", 45)?;

    let stacks = save_stacks(&name)?;
    if let Some(target) = &args.callers {
        println!("\n--- callers of {target}");
        print_callers(&fs::read_to_string(&stacks)?, target, args.depth);
    }
    println!("\nStacks saved to {}", stacks.display());
    Ok(())
}

/// Starts the container unless it's already running, replacing a stopped one.
fn start() -> Result<()> {
    let running = first_line(
        "docker",
        &["inspect", "--format", "{{.State.Running}}", CONTAINER],
    );
    if running.as_deref() == Some("true") {
        return Ok(());
    }
    let _ = command("docker").args(["rm", "-f", CONTAINER]).output();
    let mount = format!(
        "type=bind,source={},target=/src,readonly",
        repo_root().display()
    );
    let out = command("docker")
        .args([
            "run",
            "-d",
            "--name",
            CONTAINER,
            "--privileged",
            "--user",
            "root",
        ])
        .args(["--mount", &mount, "--volume", &format!("{VOLUME}:/root")])
        .args([IMAGE, "sleep", "infinity"])
        .output()
        .context("failed to run docker")?;
    ensure!(
        out.status.success(),
        "couldn't start the container: {}",
        String::from_utf8_lossy(&out.stderr).trim()
    );
    Ok(())
}

/// Installs what's missing: `perf` from Ubuntu's kernel tools, the pinned Rust and `rustfilt`
/// for Rust's symbol names. Then lets `perf` see every process, which the container's
/// privilege allows.
fn set_up(channel: &str) -> Result<()> {
    let channel = sh_quote(channel);
    must(&format!(
        "if ! ls /usr/lib/linux-tools/*/perf >/dev/null 2>&1; then \
           echo 'installing perf'; apt-get update -qq && \
           DEBIAN_FRONTEND=noninteractive apt-get install -y -qq linux-tools-generic >/dev/null; \
         fi && \
         if [ ! -x /root/.cargo/bin/rustup ]; then \
           echo 'installing Rust'; curl -sSf https://sh.rustup.rs | \
           sh -s -- -y -q --profile minimal --default-toolchain {channel}; \
         fi && \
         rustup default {channel} >/dev/null 2>&1 && \
         if ! command -v rustfilt >/dev/null; then \
           echo 'installing rustfilt'; cargo install --locked --quiet rustfilt; \
         fi && \
         echo -1 > /proc/sys/kernel/perf_event_paranoid"
    ))
}

/// Prints `perf report` for the recording `name`, by function, with `options`, at most `lines`
/// lines, leaving out the frames every sample has.
fn print_report(name: &str, options: &str, lines: usize) -> Result<()> {
    let out = capture(&format!(
        "cd /root/prof && $(perf_path) report -i {name}.data --stdio --sort sym -g none \
         {options} 2>/dev/null | rustfilt"
    ))?;
    let shown = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        // Generic symbols can run to thousands of characters; their start says enough.
        .map(|l| {
            let line = strip_hashes(l);
            line.trim_end_matches([' ', '-'])
                .chars()
                .take(180)
                .collect::<String>()
        })
        .filter(|l| !is_entry_frame(l))
        .take(lines)
        .collect::<Vec<_>>();
    for line in shown {
        println!("{line}");
    }
    Ok(())
}

/// Saves every sample's stack, innermost frame first and samples separated by blank lines, to
/// `target/profile/<name>.stacks`.
fn save_stacks(name: &str) -> Result<PathBuf> {
    let out = capture(&format!(
        "cd /root/prof && $(perf_path) script -i {name}.data -F ip,sym 2>/dev/null | rustfilt"
    ))?;
    let mut text = String::new();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        // Each frame is `<address> <symbol>`; the address isn't needed.
        let frame = line.trim().split_once(' ').map_or("", |(_, sym)| sym);
        text.push_str(&strip_hashes(frame));
        text.push('\n');
    }
    let dir = repo_root().join("target").join("profile");
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{name}.stacks"));
    fs::write(&path, text)?;
    Ok(path)
}

/// Prints [`callers`]' counts as shares of all samples.
fn print_callers(stacks: &str, target: &str, depth: usize) {
    let Callers {
        samples,
        hits,
        chains,
    } = callers(stacks, target, depth);
    let share = |n: usize| 100.0 * n as f64 / samples.max(1) as f64;
    println!("{:.1}% of {samples} samples", share(hits));
    for (chain, n) in chains.into_iter().take(15) {
        println!("{:6.1}%  {chain}", share(n));
    }
}

/// Who calls a function, from saved stacks.
struct Callers {
    /// Samples in all.
    samples: usize,
    /// Samples whose stacks hold the function.
    hits: usize,
    /// Each chain of callers above its outermost frame, with its samples, most first.
    chains: Vec<(String, usize)>,
}

/// For the samples whose stacks hold a function containing `target`: the `depth` callers
/// above its outermost frame.
fn callers(stacks: &str, target: &str, depth: usize) -> Callers {
    let samples: Vec<Vec<&str>> = stacks
        .split("\n\n")
        .map(|s| s.lines().filter(|l| !l.is_empty()).collect::<Vec<_>>())
        .filter(|s| !s.is_empty())
        .collect();
    let mut chains: HashMap<String, usize> = HashMap::new();
    let mut hits = 0;
    for sample in &samples {
        let Some(outermost) = sample.iter().rposition(|f| f.contains(target)) else {
            continue;
        };
        hits += 1;
        let chain = sample[outermost + 1..]
            .iter()
            .take(depth)
            .map(|f| f.chars().take(90).collect::<String>())
            .collect::<Vec<_>>()
            .join(" <- ");
        *chains.entry(chain).or_default() += 1;
    }
    let mut chains: Vec<_> = chains.into_iter().collect();
    chains.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    Callers {
        samples: samples.len(),
        hits,
        chains,
    }
}

/// Whether a report line is one of the frames every sample passes through: the process's
/// entry, the bench's `main` and unresolved addresses.
fn is_entry_frame(line: &str) -> bool {
    let symbol = line.rsplit("] ").next().unwrap_or(line).trim();
    symbol.starts_with("0x")
        || matches!(
            symbol,
            "_start" | "__libc_start_main" | "main" | "bench::main"
        )
        || symbol.starts_with("std::sys::backtrace::__rust_begin_short_backtrace")
}

/// `text` without the `::h<16 hex digits>` hashes legacy-mangled Rust symbols end in.
fn strip_hashes(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("::h") {
        let hash = &rest[at + 3..];
        let is_hash = hash.len() >= 16 && hash[..16].bytes().all(|b| b.is_ascii_hexdigit());
        let end = if is_hash { at + 19 } else { at + 3 };
        out.push_str(&rest[..at]);
        if !is_hash {
            out.push_str("::h");
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

/// `text` as a file name: lowercase letters and digits, other runs as one `-`.
fn slug(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if c == '.' {
            out.push('.');
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

/// A command running `script` in the container as root, with cargo on the path and
/// `perf_path` printing where Ubuntu put `perf` (its `/usr/bin/perf` wrapper refuses a kernel
/// it has no package for, such as Docker Desktop's).
fn exec(script: &str) -> std::process::Command {
    let mut cmd = command("docker");
    cmd.args(["exec", CONTAINER, "sh", "-c"]).arg(format!(
        "export PATH=/root/.cargo/bin:$PATH; \
         perf_path() {{ ls -d /usr/lib/linux-tools/*/perf | head -n 1; }}; {script}"
    ));
    cmd
}

/// Runs `script` with its output on the console, failing if it fails.
fn must(script: &str) -> Result<()> {
    let status = exec(script).status().context("failed to run docker exec")?;
    ensure!(status.success(), "failed in the container: {script}");
    Ok(())
}

/// Runs `script` and captures its output, failing if it fails.
fn capture(script: &str) -> Result<Output> {
    let out = exec(script).output().context("failed to run docker exec")?;
    ensure!(
        out.status.success(),
        "failed in the container: {}",
        String::from_utf8_lossy(&out.stderr).trim()
    );
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_are_stripped_and_other_text_kept() {
        assert_eq!(
            strip_hashes("spyglass::f::h0123456789abcdef <x::hello>"),
            "spyglass::f <x::hello>"
        );
    }

    #[test]
    fn slugs_keep_the_version() {
        assert_eq!(slug("26.3-dungeons at 1:2"), "26.3-dungeons-at-1-2");
    }

    #[test]
    fn callers_are_counted_above_the_outermost_frame() {
        // Two samples of three hold `lookup`; in one it's also called from itself, which counts
        // from the outermost frame.
        let stacks =
            "leaf\nlookup\nouter\nmain\n\nleaf\nother\nmain\n\nlookup\nlookup\nouter\nmain\n";
        let found = callers(stacks, "lookup", 2);
        assert_eq!((found.samples, found.hits), (3, 2));
        assert_eq!(found.chains, vec![("outer <- main".to_string(), 2)]);
    }
}
