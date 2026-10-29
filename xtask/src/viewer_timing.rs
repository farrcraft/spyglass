//! `cargo xtask viewer-timing`: the viewer's timing scenarios. Each is a cold start of the `dist`
//! viewer from settings written for it (`SPYGLASS_VIEWER_SETTINGS`), run until the map has
//! filled the window, with `SPYGLASS_VIEWER_TIMING` on. It prints the viewer's own lines: when
//! the window was covered and full, and each kind of job's time.

use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

use crate::{command, repo_root};

#[derive(clap::Args)]
pub struct ViewerTiming {
    /// The scenarios to run, by number [default: all]:
    /// 1: 1:4, biomes at y 64;
    /// 2: 1:4, biomes at the surface;
    /// 3: 2 with relief and swamp huts confirmed;
    /// 4: 1:16, villages unconfirmed;
    /// 5: 2 while a search runs.
    scenarios: Vec<u32>,
    /// The seed shown.
    #[arg(long, default_value_t = 0)]
    seed: i64,
    /// The game version, from `bundles/` [default: the newest release there].
    #[arg(long)]
    game_version: Option<String>,
    /// Threads for scenario 5's search [default: every core but one, as the viewer's search].
    #[arg(long)]
    search_threads: Option<usize>,
    /// Give up on a scenario after this many seconds.
    #[arg(long, default_value_t = 180)]
    timeout: u64,
}

/// A scenario's viewer settings: the scale, then biomes at the surface, relief, the structures
/// marked and whether terrain confirms them.
type Layers = (u32, bool, bool, &'static [&'static str], bool);

fn layers(scenario: u32) -> Option<Layers> {
    Some(match scenario {
        1 => (4, false, false, &[], true),
        2 | 5 => (4, true, false, &[], true),
        3 => (4, true, true, &["minecraft:swamp_hut"], true),
        4 => (16, false, false, &["#minecraft:village"], false),
        _ => return None,
    })
}

pub fn run(args: &ViewerTiming) -> Result<()> {
    let scenarios = if args.scenarios.is_empty() {
        vec![1, 2, 3, 4, 5]
    } else {
        args.scenarios.clone()
    };
    if let Some(s) = scenarios.iter().find(|&&s| layers(s).is_none()) {
        bail!("there's no scenario {s}: they're 1 to 5");
    }
    let status = command("cargo")
        .args(["build", "--profile", "dist", "-p", "spyglass-viewer"])
        .args(["-p", "spyglass-cli"])
        .status()
        .context("failed to run cargo")?;
    if !status.success() {
        bail!("the dist build failed");
    }
    let temp = tempfile::tempdir()?;
    for scenario in scenarios {
        println!("scenario {scenario}:");
        for line in run_one(args, scenario, temp.path())? {
            println!("  {line}");
        }
    }
    Ok(())
}

/// Runs one scenario and returns the viewer's timing lines but each job's own.
fn run_one(args: &ViewerTiming, scenario: u32, temp: &Path) -> Result<Vec<String>> {
    let (scale, surface, relief, structures, confirm) = layers(scenario).expect("checked");
    let bundles = repo_root().join("bundles");
    let version = args
        .game_version
        .as_ref()
        .map_or("None".to_string(), |v| format!("Some({v:?})"));
    let structures: Vec<String> = structures.iter().map(|s| format!("{s:?}")).collect();
    // The viewer's settings in RON, as eframe stores them: fields left out take their defaults.
    let viewer = format!(
        "(bundles:{bundles:?},version:{version},seed:\"{seed}\",dimension:\"minecraft:overworld\",\
         view:(centre:(0.0,0.0),scale:{scale},y:64),views:{{}},ties:false,slime_chunks:false,\
         spawn:true,confirm:{confirm},relief:{relief},surface:{surface},\
         structures:[{structures}],highlight:[])",
        seed = args.seed,
        structures = structures.join(","),
    );
    let window = "(normal_geometry:(outer_position_pixels:Some((x:200.0,y:100.0)),scale_factor:1.0,\
                  inner_size_points:(x:1280.0,y:800.0)),fullscreen:false,maximized:false)";
    let settings = temp.join(format!("{scenario}.ron"));
    std::fs::write(
        &settings,
        serde_json::to_string(&serde_json::json!({"window": window, "spyglass": viewer}))?,
    )?;
    let exe = |name: &str| {
        repo_root()
            .join("target/dist")
            .join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
    };
    let mut search = if scenario == 5 {
        let threads = args.search_threads.unwrap_or_else(|| {
            std::thread::available_parallelism().map_or(1, |n| n.get().saturating_sub(1).max(1))
        });
        let mut cmd = Command::new(exe("spyglass"));
        cmd.current_dir(repo_root())
            .args(["search", "--confirm", "--threads", &threads.to_string()])
            .args(["--time", &format!("{}s", args.timeout + 10)])
            .args(["-q", "require 2 minecraft:swamp_hut within 400 of 0,0"]);
        if let Some(v) = &args.game_version {
            cmd.args(["--game-version", v]);
        }
        let child = cmd.stdout(Stdio::null()).stderr(Stdio::null()).spawn()?;
        // Let it get going before the viewer starts.
        std::thread::sleep(Duration::from_secs(2));
        Some(Killed(child))
    } else {
        None
    };
    let log = temp.join(format!("{scenario}.log"));
    let viewer = Command::new(exe("spyglass-viewer"))
        .env("SPYGLASS_VIEWER_SETTINGS", &settings)
        .env("SPYGLASS_VIEWER_TIMING", "1")
        .stderr(std::fs::File::create(&log)?)
        .spawn()
        .context("failed to start the viewer")?;
    let viewer = Killed(viewer);
    let start = Instant::now();
    let mut text = String::new();
    // The summary follows "window full" at once; a moment more lets it all arrive.
    while start.elapsed() < Duration::from_secs(args.timeout) && !text.contains("timing:   ") {
        std::thread::sleep(Duration::from_millis(500));
        text = std::fs::read_to_string(&log)?;
    }
    std::thread::sleep(Duration::from_millis(500));
    drop(viewer);
    search.take();
    let text = std::fs::read_to_string(&log)?;
    let mut lines: Vec<String> = text
        .lines()
        .filter(|l| !l.starts_with("timing: job"))
        .map(str::to_string)
        .collect();
    if !text.contains("window full") {
        lines.push(format!("(not full after {} s)", args.timeout));
    }
    Ok(lines)
}

/// A child process, killed when dropped.
struct Killed(Child);

impl Drop for Killed {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
