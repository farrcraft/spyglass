//! `cargo xtask viewer-timing`: the viewer's timing scenarios. Each is a cold start of the `dist`
//! viewer from settings written for it (`SPYGLASS_VIEWER_SETTINGS`), run until the map has
//! filled the window, with `SPYGLASS_VIEWER_TIMING` on. It prints the viewer's own lines: when
//! the window was covered and full, and each kind of job's time. Scenarios 6 to 9 then zoom
//! (`SPYGLASS_VIEWER_TIMING_ZOOM`) and run until the window is full again. Last, the viewer's
//! memory: its working set at the end, and the most it reached.

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
    /// 5: 2 while a search runs;
    /// 6: 1:2 with every feature kind, then zoomed to 1:1;
    /// 7: 1:1 with biomes at the surface and relief, then zoomed out to 1:4;
    /// 8: 1:4 with every underground layer at y 0, then zoomed in to 1:1;
    /// 9: 1:2 with every feature kind and every container, zoomed out to 1:8 once covered.
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

/// A scenario's viewer settings.
#[derive(Clone, Copy, Default)]
struct Layers {
    /// Blocks per pixel at the start.
    scale: u32,
    /// The height shown.
    y: i32,
    /// Biomes at the surface.
    surface: bool,
    /// Relief shading.
    relief: bool,
    /// The structures marked, and whether terrain confirms them.
    structures: &'static [&'static str],
    confirm: bool,
    /// The placed features marked.
    features: &'static [&'static str],
    /// Every loot container marked.
    containers: bool,
    /// Every underground layer.
    underground: bool,
    /// The zoom made once the window's full, as `SPYGLASS_VIEWER_TIMING_ZOOM` takes it.
    zoom: Option<&'static str>,
}

/// Every kind of feature the viewer marks, as its Features section lists them.
const EVERY_FEATURE: &[&str] = &[
    "minecraft:monster_room",
    "minecraft:monster_room_deep",
    "minecraft:amethyst_geode",
    "minecraft:desert_well",
    "minecraft:fossil_upper",
    "minecraft:fossil_lower",
    "minecraft:lake_lava_underground",
    "minecraft:lake_lava_surface",
];

/// Scenario `scenario`'s settings, if there's one by that number.
fn layers(scenario: u32) -> Option<Layers> {
    let at = |scale| Layers {
        scale,
        y: 64,
        confirm: true,
        ..Layers::default()
    };
    Some(match scenario {
        1 => at(4),
        2 | 5 => Layers {
            surface: true,
            ..at(4)
        },
        3 => Layers {
            surface: true,
            relief: true,
            structures: &["minecraft:swamp_hut"],
            ..at(4)
        },
        4 => Layers {
            structures: &["#minecraft:village"],
            confirm: false,
            ..at(16)
        },
        6 => Layers {
            features: EVERY_FEATURE,
            zoom: Some("1"),
            ..at(2)
        },
        7 => Layers {
            surface: true,
            relief: true,
            zoom: Some("4"),
            ..at(1)
        },
        8 => Layers {
            y: 0,
            underground: true,
            zoom: Some("1"),
            ..at(4)
        },
        9 => Layers {
            features: EVERY_FEATURE,
            containers: true,
            zoom: Some("8,covered"),
            ..at(2)
        },
        _ => return None,
    })
}

pub fn run(args: &ViewerTiming) -> Result<()> {
    let scenarios = if args.scenarios.is_empty() {
        (1..=9).collect()
    } else {
        args.scenarios.clone()
    };
    if let Some(s) = scenarios.iter().find(|&&s| layers(s).is_none()) {
        bail!("there's no scenario {s}: they're 1 to 9");
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
    let layers = layers(scenario).expect("checked");
    let Layers {
        scale,
        y,
        surface,
        relief,
        structures,
        confirm,
        features,
        containers,
        underground,
        zoom,
    } = layers;
    let bundles = repo_root().join("bundles");
    let version = args
        .game_version
        .as_ref()
        .map_or("None".to_string(), |v| format!("Some({v:?})"));
    let list = |ids: &[&str]| -> String {
        ids.iter()
            .map(|s| format!("{s:?}"))
            .collect::<Vec<_>>()
            .join(",")
    };
    // The viewer's settings in RON, as eframe stores them: fields left out take their defaults.
    let viewer = format!(
        "(bundles:{bundles:?},version:{version},seed:\"{seed}\",dimension:\"minecraft:overworld\",\
         view:(centre:(0.0,0.0),scale:{scale},y:{y}),views:{{}},ties:false,slime_chunks:false,\
         spawn:true,confirm:{confirm},relief:{relief},surface:{surface},\
         veins:{underground},noise_caves:{underground},carvers:{underground},\
         structures:[{structures}],features:[{features}],containers:{containers},highlight:[])",
        seed = args.seed,
        structures = list(structures),
        features = list(features),
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
    let mut viewer = Command::new(exe("spyglass-viewer"));
    viewer
        .env("SPYGLASS_VIEWER_SETTINGS", &settings)
        .env("SPYGLASS_VIEWER_TIMING", "1")
        .stderr(std::fs::File::create(&log)?);
    if let Some(zoom) = zoom {
        viewer.env("SPYGLASS_VIEWER_TIMING_ZOOM", zoom);
    }
    let viewer = viewer.spawn().context("failed to start the viewer")?;
    let pid = viewer.id();
    // Full once, and again after a zoom made once it's full; a zoom made once it's covered
    // comes before the first.
    let fills = match zoom {
        Some(zoom) if !zoom.ends_with(",covered") => 2,
        _ => 1,
    };
    let viewer = Killed(viewer);
    let start = Instant::now();
    let mut text = String::new();
    // The summary follows "window full" at once; a moment more lets it all arrive.
    while start.elapsed() < Duration::from_secs(args.timeout)
        && text.matches("window full").count() < fills
    {
        std::thread::sleep(Duration::from_millis(500));
        text = std::fs::read_to_string(&log)?;
    }
    std::thread::sleep(Duration::from_millis(500));
    let memory = memory(pid);
    drop(viewer);
    search.take();
    let text = std::fs::read_to_string(&log)?;
    let mut lines: Vec<String> = text
        .lines()
        .filter(|l| !l.starts_with("timing: job"))
        .map(str::to_string)
        .collect();
    if text.matches("window full").count() < fills {
        lines.push(format!("(not full after {} s)", args.timeout));
    }
    if let Some([now, peak]) = memory {
        let mb = |bytes: u64| bytes as f64 / (1024.0 * 1024.0);
        lines.push(format!(
            "memory: {:.0} MB at the end, {:.0} MB at most",
            mb(now),
            mb(peak)
        ));
    }
    Ok(lines)
}

/// Process `pid`'s resident memory in bytes, now and the most it's reached, where the platform
/// says: the working set on Windows, `VmRSS` and `VmHWM` on Linux, and elsewhere `ps`'s resident
/// size for both.
fn memory(pid: u32) -> Option<[u64; 2]> {
    if cfg!(windows) {
        let script =
            format!("$p = Get-Process -Id {pid}; \"$($p.WorkingSet64) $($p.PeakWorkingSet64)\"");
        let out = Command::new("powershell")
            .args(["-NoProfile", "-Command", &script])
            .output()
            .ok()?;
        let text = String::from_utf8(out.stdout).ok()?;
        let mut numbers = text.split_whitespace().map(|n| n.parse::<u64>().ok());
        Some([numbers.next()??, numbers.next()??])
    } else if cfg!(target_os = "linux") {
        let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
        let field = |name: &str| -> Option<u64> {
            let line = status.lines().find(|l| l.starts_with(name))?;
            let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
            Some(kb * 1024)
        };
        Some([field("VmRSS:")?, field("VmHWM:")?])
    } else {
        let out = Command::new("ps")
            .args(["-o", "rss=", "-p", &pid.to_string()])
            .output()
            .ok()?;
        let kb: u64 = String::from_utf8(out.stdout).ok()?.trim().parse().ok()?;
        Some([kb * 1024, kb * 1024])
    }
}

/// A child process, killed when dropped.
struct Killed(Child);

impl Drop for Killed {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
