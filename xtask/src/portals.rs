//! `cargo xtask oracle portals`: where the game's own portal code sends a trip, and which portal
//! its search finds, for random layouts and constructed edge cases.
//!
//! The cases are made here, from a fixed generator, and played by `spyglass.oracle.Portals` on
//! the stock server in the harness's JVM, which places the portals, runs the searches and trips
//! through the game's code and removes what it built
//! (docs/adr/0076-the-oracle-drives-the-game-s-portal-code.md). See docs/specs/oracle.md.

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

use crate::oracle::Harness;
use crate::repo_root;

const OVERWORLD: &str = "minecraft:overworld";
const NETHER: &str = "minecraft:the_nether";
const END: &str = "minecraft:the_end";

/// A standing player's half-width, `0.6f / 2` widened (docs/specs/portals.md, "Entering a
/// portal").
const HALF_WIDTH: f64 = 0.300_000_011_920_928_96;
/// A standing player's height, `1.8f` widened.
const HEIGHT: f64 = 1.799_999_952_316_284_2;
/// What the game deflates an entity's box by before finding the blocks it's in.
const DEFLATE: f64 = 9.999_999_747_378_752E-6;

/// Writes the cases for `random` random layouts and every constructed kind, plays them on the
/// server, and writes the answers to `oracle-out/<version>/portals/portals.json`.
pub fn run(harness: &Harness, random: usize) -> Result<()> {
    if !harness.packs.is_empty() {
        bail!("oracle portals doesn't take --pack yet: it plays vanilla dimensions");
    }
    let dir = server_dir(harness)?;
    let cases = dir.join("cases.json");
    let mut layouts = constructed();
    layouts.extend(random_layouts(random));
    fs::write(
        &cases,
        serde_json::to_string(&json!({ "layouts": layouts }))? + "\n",
    )?;
    let out = repo_root()
        .join("oracle-out")
        .join(&harness.version)
        .join("portals")
        .join("portals.json");
    let started = Instant::now();
    play(harness, &dir, &cases, &out)?;
    println!(
        "{}: {} layouts, {:.0} s",
        out.display(),
        layouts.len(),
        started.elapsed().as_secs_f64()
    );
    fs::remove_dir_all(&dir).with_context(|| format!("failed to remove {}", dir.display()))?;
    Ok(())
}

/// Runs `Portals` in `dir`; a failed run's folder is kept, with its log.
fn play(harness: &Harness, dir: &Path, cases: &Path, out: &Path) -> Result<()> {
    let log = dir.join("portals.log");
    let mut java = Command::new(&harness.java);
    java.args([
        "-Xmx4G",
        "--enable-native-access=ALL-UNNAMED",
        // Portals finds the server through the server thread's task (ADR 0073).
        "--add-opens",
        "java.base/java.lang=ALL-UNNAMED",
    ]);
    // The game's libraries use sun.misc.Unsafe; the option exists from Java 23.
    if harness.major >= 23 {
        java.arg("--sun-misc-unsafe-memory-access=allow");
    }
    let status = java
        .arg(format!("-Dspyglass.oracle.version={}", harness.version))
        .arg("-cp")
        .arg(&harness.classpath)
        .arg("spyglass.oracle.Portals")
        .arg(cases)
        .arg(out)
        .args(["--", "--nogui"])
        .current_dir(dir)
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
            "the portals run failed ({status}); {} ends:\n{tail}",
            log.display()
        );
    }
    Ok(())
}

/// A fresh server folder: the EULA and properties. The Overworld is a void superflat, so the
/// chunks a search loads and a built portal needs generate quickly; its dimension type, and so
/// its scale and height, are the Overworld's.
fn server_dir(harness: &Harness) -> Result<PathBuf> {
    let dir = harness.work.join("portals-server");
    if dir.exists() {
        fs::remove_dir_all(&dir)?;
    }
    fs::create_dir_all(&dir)?;
    // Accepted by whoever ran this: `require_eula` checked before any server started.
    fs::write(dir.join("eula.txt"), "eula=true\n")?;
    // As for locate: any free port, no pause when empty, no watchdog.
    fs::write(
        dir.join("server.properties"),
        "level-seed=1\n\
         level-type=minecraft\\:flat\n\
         generator-settings={\"layers\"\\:[{\"block\"\\:\"minecraft\\:air\",\"height\"\\:1}],\"biome\"\\:\"minecraft\\:the_void\"}\n\
         generate-structures=false\n\
         server-ip=127.0.0.1\n\
         server-port=0\n\
         online-mode=false\n\
         pause-when-empty-seconds=-1\n\
         max-tick-time=-1\n\
         sync-chunk-writes=false\n\
         spawn-protection=0\n",
    )?;
    Ok(dir)
}

/// A rectangle of portal blocks, as the cases file has it.
#[derive(Clone, Copy, Debug)]
struct Rect {
    dimension: &'static str,
    along_x: bool,
    corner: [i32; 3],
    width: i32,
    height: i32,
}

impl Rect {
    fn new(
        dimension: &'static str,
        along_x: bool,
        corner: [i32; 3],
        width: i32,
        height: i32,
    ) -> Self {
        Rect {
            dimension,
            along_x,
            corner,
            width,
            height,
        }
    }

    /// A single portal block.
    fn block(dimension: &'static str, at: [i32; 3]) -> Self {
        Rect::new(dimension, true, at, 1, 1)
    }

    fn json(&self) -> Value {
        json!({
            "dimension": self.dimension,
            "axis": if self.along_x { "x" } else { "z" },
            "corner": self.corner,
            "width": self.width,
            "height": self.height,
        })
    }

    /// The blocks' box, inclusive: x, y and z ranges.
    fn extent(&self) -> [[i32; 2]; 3] {
        let [x, y, z] = self.corner;
        let along = self.width - 1;
        if self.along_x {
            [[x, x + along], [y, y + self.height - 1], [z, z]]
        } else {
            [[x, x], [y, y + self.height - 1], [z, z + along]]
        }
    }

    /// Whether the two are in one dimension and touch or overlap, which would make the game's
    /// rectangle around one of them take in the other.
    fn touches(&self, other: &Rect) -> bool {
        self.dimension == other.dimension
            && self
                .extent()
                .iter()
                .zip(other.extent())
                .all(|(a, b)| a[0] <= b[1] + 1 && b[0] <= a[1] + 1)
    }

    /// The positions a standing player can be at inside it (docs/specs/portals.md, "Entering a
    /// portal"): along the portal and up inclusive, across it half-open. `None` when it's too
    /// low to stand in.
    fn standing_range(&self) -> Option<[[f64; 2]; 3]> {
        let [x, y, z] = self.corner.map(f64::from);
        let up = [y, y + f64::from(self.height) - HEIGHT];
        if up[0] > up[1] {
            return None;
        }
        let along = |start: f64| {
            [
                start + HALF_WIDTH,
                start + f64::from(self.width) - HALF_WIDTH,
            ]
        };
        let across = |start: f64| {
            [
                start - HALF_WIDTH + DEFLATE,
                start + 1.0 + HALF_WIDTH - DEFLATE,
            ]
        };
        Some(if self.along_x {
            [along(x), up, across(z)]
        } else {
            [across(x), up, along(z)]
        })
    }
}

/// A layout in the cases file.
struct Layout {
    name: String,
    kind: &'static str,
    portals: Vec<Rect>,
    stale: Vec<(&'static str, [i32; 3])>,
    borders: Vec<Value>,
    searches: Vec<(&'static str, [i32; 3])>,
    trips: Vec<(&'static str, [f64; 3], usize)>,
}

impl Layout {
    fn new(name: impl Into<String>, kind: &'static str, portals: Vec<Rect>) -> Self {
        Layout {
            name: name.into(),
            kind,
            portals,
            stale: Vec::new(),
            borders: Vec::new(),
            searches: Vec::new(),
            trips: Vec::new(),
        }
    }

    fn search(mut self, dimension: &'static str, exit: [i32; 3]) -> Self {
        self.searches.push((dimension, exit));
        self
    }

    /// A trip from inside portal `portal`, which must be in `dimension`.
    fn trip(mut self, portal: usize, pos: [f64; 3]) -> Self {
        self.trips
            .push((self.portals[portal].dimension, pos, portal));
        self
    }

    fn border(mut self, dimension: &str, center: [f64; 2], size: f64) -> Self {
        self.borders
            .push(json!({ "dimension": dimension, "center": center, "size": size }));
        self
    }

    fn json(&self) -> Value {
        json!({
            "name": self.name,
            "kind": self.kind,
            "borders": self.borders,
            "portals": self.portals.iter().map(Rect::json).collect::<Vec<_>>(),
            "stale": self.stale.iter().map(|(dimension, block)| json!({ "dimension": dimension, "block": block })).collect::<Vec<_>>(),
            "searches": self.searches.iter().map(|(dimension, exit)| json!({ "dimension": dimension, "exit": exit })).collect::<Vec<_>>(),
            "trips": self.trips.iter().map(|(dimension, pos, portal)| json!({ "dimension": dimension, "pos": pos, "portal": portal })).collect::<Vec<_>>(),
        })
    }
}

/// One layout of each constructed kind, or a few.
fn constructed() -> Vec<Value> {
    let mut layouts = Vec::new();
    // The square's edges: a block at the radius is a candidate, one past it isn't, at any
    // height.
    for (dimension, radius, exit) in [(OVERWORLD, 128, [37, 70, -21]), (NETHER, 16, [5, 70, -3])] {
        let [x, y, z] = exit;
        let short = dimension.trim_start_matches("minecraft:");
        for (side, [dx, dz]) in [
            ("+x", [1, 0]),
            ("-x", [-1, 0]),
            ("+z", [0, 1]),
            ("-z", [0, -1]),
        ] {
            for (kind, reach) in [("radius-in", radius), ("radius-out", radius + 1)] {
                // Off the axis by the radius too, turning a quarter each side, so each of the
                // square's corners counts.
                let off = if kind == "radius-in" { radius } else { 0 };
                let at = [x + dx * reach - dz * off, y + 3, z + dz * reach + dx * off];
                layouts.push(
                    Layout::new(
                        format!("{kind}-{short}{side}"),
                        kind,
                        vec![Rect::block(dimension, at)],
                    )
                    .search(dimension, exit),
                );
            }
        }
        layouts.push(
            Layout::new(
                format!("height-{short}"),
                "height",
                vec![Rect::block(dimension, [x, y + 100, z])],
            )
            .search(dimension, exit),
        );
    }
    // Equal distance, the lower block wins.
    layouts.push(
        Layout::new(
            "y-tie",
            "y-tie",
            vec![
                Rect::block(OVERWORLD, [41, 73, -21]),
                Rect::block(OVERWORLD, [41, 67, -21]),
            ],
        )
        .search(OVERWORLD, [37, 70, -21]),
    );
    // Equal distance and height in different chunks: the chunk walk goes by z first, then x.
    layouts.push(
        Layout::new(
            "chunk-tie-z",
            "chunk-tie-z",
            vec![
                Rect::block(OVERWORLD, [29, 70, -16]),
                Rect::block(OVERWORLD, [32, 70, -19]),
            ],
        )
        .search(OVERWORLD, [32, 70, -16]),
    );
    layouts.push(
        Layout::new(
            "chunk-tie-x",
            "chunk-tie-x",
            vec![
                Rect::block(OVERWORLD, [35, 70, -16]),
                Rect::block(OVERWORLD, [29, 70, -16]),
            ],
        )
        .search(OVERWORLD, [32, 70, -16]),
    );
    // Equal in one section: the game's pick is its set's order.
    for (name, dimension, exit) in [
        ("section-tie-overworld", OVERWORLD, [40, 70, -24]),
        ("section-tie-nether", NETHER, [8, 70, 8]),
    ] {
        let [x, y, z] = exit;
        layouts.push(
            Layout::new(
                name,
                "section-tie",
                vec![
                    Rect::block(dimension, [x - 3, y, z]),
                    Rect::block(dimension, [x + 3, y, z]),
                    Rect::block(dimension, [x, y, z + 3]),
                ],
            )
            .search(dimension, exit),
        );
    }
    // A small Nether border: x from -31 to 32, z from -31.5 to 31.5.
    let small = |layout: Layout| layout.border(NETHER, [0.5, 0.0], 63.0);
    layouts.push(
        small(Layout::new(
            "border-outside",
            "border-outside",
            vec![
                Rect::block(NETHER, [32, 70, 0]),
                Rect::block(NETHER, [23, 70, 0]),
            ],
        ))
        .search(NETHER, [28, 70, 0]),
    );
    layouts.push(
        small(Layout::new(
            "border-inside",
            "border-inside",
            vec![
                Rect::block(NETHER, [0, 70, 31]),
                Rect::block(NETHER, [-31, 70, 0]),
                Rect::block(NETHER, [0, 70, -32]),
            ],
        ))
        .search(NETHER, [0, 70, 28])
        .search(NETHER, [-28, 70, 0])
        .search(NETHER, [0, 70, -28]),
    );
    // Trips aimed past the border are clamped to it.
    layouts.push(
        small(Layout::new(
            "border-clamp",
            "border-clamp",
            vec![
                Rect::new(OVERWORLD, true, [1000, 70, 0], 2, 3),
                Rect::new(OVERWORLD, false, [8, 70, 1000], 2, 3),
                Rect::new(OVERWORLD, true, [-1000, 70, 0], 2, 3),
                Rect::block(NETHER, [31, 72, 3]),
                Rect::block(NETHER, [3, 72, 31]),
                Rect::block(NETHER, [-30, 72, 2]),
            ],
        ))
        .trip(0, [1001.0, 70.0, 0.5])
        .trip(1, [8.5, 70.0, 1001.0])
        .trip(2, [-999.0, 70.0, 0.5]),
    );
    // y isn't clamped: a trip from above the Nether's build height.
    layouts.push(
        Layout::new(
            "y-unclamped",
            "y-unclamped",
            vec![
                Rect::new(OVERWORLD, true, [80, 300, 80], 2, 3),
                Rect::block(NETHER, [10, 120, 10]),
            ],
        )
        .trip(0, [81.0, 300.0, 80.5]),
    );
    // A record whose block isn't a portal any more.
    let mut stale = Layout::new(
        "stale",
        "stale",
        vec![Rect::block(OVERWORLD, [45, 70, -21])],
    )
    .search(OVERWORLD, [37, 70, -21]);
    stale.stale.push((OVERWORLD, [40, 70, -21]));
    layouts.push(stale);
    // A portal in the End leads to the Nether, at the End's scale.
    layouts.push(
        Layout::new(
            "end",
            "end",
            vec![
                Rect::new(END, true, [100, 70, 0], 2, 3),
                Rect::block(NETHER, [14, 70, 2]),
            ],
        )
        .trip(0, [101.0, 70.0, 0.5]),
    );
    // No portal in range: the game builds one.
    layouts.push(
        Layout::new(
            "built",
            "built",
            vec![Rect::new(NETHER, false, [60, 70, 60], 2, 3)],
        )
        .trip(0, [60.5, 70.0, 61.0]),
    );
    layouts.into_iter().map(|layout| layout.json()).collect()
}

/// `count` random layouts from a fixed generator, the same on every machine: portals in the
/// Overworld and the Nether near the origin, searches near them, and trips from inside each
/// portal, its standing range's corners included.
fn random_layouts(count: usize) -> Vec<Value> {
    let mut random = SplitMix::new(0x706f_7274_616c_7321); // "portals!"
    (0..count)
        .map(|index| {
            let mut portals: Vec<Rect> = Vec::new();
            for (dimension, spread, low, high) in [(OVERWORLD, 200, 60, 90), (NETHER, 25, 40, 100)]
            {
                let wanted = random.between(2, 6);
                let mut tries = 0;
                while portals.iter().filter(|p| p.dimension == dimension).count() < wanted as usize
                    && tries < 100
                {
                    tries += 1;
                    let rect = Rect::new(
                        dimension,
                        random.between(0, 1) == 0,
                        [
                            random.between(-spread, spread),
                            random.between(low, high),
                            random.between(-spread, spread),
                        ],
                        random.between(1, 4),
                        random.between(1, 5),
                    );
                    if !portals.iter().any(|p| p.touches(&rect)) {
                        portals.push(rect);
                    }
                }
            }
            let mut layout = Layout::new(format!("random-{index}"), "random", portals.clone());
            for (dimension, spread, low, high) in [(OVERWORLD, 260, 50, 100), (NETHER, 35, 30, 110)]
            {
                for _ in 0..6 {
                    let exit = [
                        random.between(-spread, spread),
                        random.between(low, high),
                        random.between(-spread, spread),
                    ];
                    layout = layout.search(dimension, exit);
                }
            }
            for (portal, rect) in portals.iter().enumerate() {
                let Some(range) = rect.standing_range() else {
                    continue;
                };
                // The low corner and the high one (just inside the open end across), then
                // random positions.
                let low = range.map(|r| r[0]);
                let mut high = range.map(|r| r[1]);
                let across = if rect.along_x { 2 } else { 0 };
                high[across] = high[across].next_down();
                layout = layout.trip(portal, low).trip(portal, high);
                for _ in 0..3 {
                    let pos = range.map(|[a, b]| a + (b - a) * random.unit());
                    layout = layout.trip(portal, pos);
                }
            }
            layout.json()
        })
        .collect()
}

/// SplitMix64, from a fixed start, so the random layouts are the same everywhere.
struct SplitMix(u64);

impl SplitMix {
    fn new(seed: u64) -> Self {
        SplitMix(seed)
    }

    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// A whole number from `low` to `high`, both included.
    fn between(&mut self, low: i32, high: i32) -> i32 {
        let span = u64::from((high - low).unsigned_abs()) + 1;
        low + i32::try_from(self.next() % span).expect("the span fits")
    }

    /// A number in `[0, 1)`.
    fn unit(&mut self) -> f64 {
        #[expect(clippy::cast_precision_loss, reason = "53 bits, exactly representable")]
        let bits = (self.next() >> 11) as f64;
        bits / (1u64 << 53) as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every kind of case, as the comparison in `crates/spyglass/tests/portals.rs` requires.
    const KINDS: &[&str] = &[
        "random",
        "radius-in",
        "radius-out",
        "height",
        "y-tie",
        "chunk-tie-z",
        "chunk-tie-x",
        "section-tie",
        "border-outside",
        "border-inside",
        "border-clamp",
        "y-unclamped",
        "stale",
        "end",
        "built",
    ];

    #[test]
    fn every_kind_is_constructed_and_only_those() {
        let mut kinds: Vec<String> = constructed()
            .iter()
            .chain(&random_layouts(2))
            .map(|layout| layout["kind"].as_str().unwrap().to_owned())
            .collect();
        kinds.sort();
        kinds.dedup();
        let mut expected = KINDS.to_vec();
        expected.sort_unstable();
        assert_eq!(kinds, expected);
    }

    #[test]
    fn random_layouts_are_fixed_and_their_portals_apart() {
        let layouts = random_layouts(5);
        assert_eq!(layouts, random_layouts(5));
        for layout in &layouts {
            let portals = layout["portals"].as_array().unwrap();
            assert!(portals.len() >= 2);
            assert!(!layout["trips"].as_array().unwrap().is_empty());
        }
    }

    #[test]
    fn trips_start_in_their_own_dimension() {
        for layout in constructed().iter().chain(&random_layouts(3)) {
            let portals = layout["portals"].as_array().unwrap();
            for trip in layout["trips"].as_array().unwrap() {
                let portal = &portals[usize::try_from(trip["portal"].as_u64().unwrap()).unwrap()];
                assert_eq!(trip["dimension"], portal["dimension"]);
            }
        }
    }

    #[test]
    fn a_two_by_three_portal_has_the_spec_s_standing_range() {
        let range = Rect::new(OVERWORLD, true, [0, 64, 0], 2, 3)
            .standing_range()
            .unwrap();
        assert_eq!(range[0], [HALF_WIDTH, 2.0 - HALF_WIDTH]);
        assert_eq!(range[1], [64.0, 67.0 - HEIGHT]);
        assert!(
            Rect::new(OVERWORLD, true, [0, 64, 0], 2, 1)
                .standing_range()
                .is_none()
        );
    }
}
