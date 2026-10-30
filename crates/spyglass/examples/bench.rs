//! Times the public API's main operations, for comparing changes: `cargo xtask bench`, or
//! `cargo run --profile dist -p spyglass --example bench -- [bundle dir or version] [--only text]`.
//! Needs a bundle: a directory, a version in `bundles/`, or by default the newest release there.
//! Each line is the median of several runs; `--only` runs just the lines whose names contain
//! the text.

use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use spyglass::{Dimension, Game, Layers, MapArea, Session, World};

/// The text a line's name must contain to run, from `--only`.
static ONLY: OnceLock<Option<String>> = OnceLock::new();

const SEEDS: [i64; 5] = [
    0,
    1,
    -1_556_512_960,
    123_456_789,
    -4_893_098_694_207_956_070,
];

fn main() -> spyglass::Result<()> {
    let mut bundle = None;
    let mut only = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--only" {
            only = args.next();
        } else {
            bundle = Some(arg);
        }
    }
    ONLY.set(only).expect("set once");
    let dir: PathBuf = match bundle {
        Some(arg) if Path::new(&arg).join("bundle.json").is_file() => arg.into(),
        version => spyglass::find_bundle("bundles", version.as_deref())?.dir,
    };
    println!("bundle: {}", dir.display());
    report("World::open", 3, || {
        black_box(World::open(&dir).unwrap());
    });
    let world = World::open(&dir)?;
    let mut seeds = SEEDS.iter().cycle();
    report("World::session", 20, || {
        black_box(world.session(*seeds.next().unwrap()).unwrap());
    });
    let session = world.session(SEEDS[0])?;
    let villages = world.structure_query("#minecraft:village")?;
    report("locate_structure #village (resolved)", 10, || {
        black_box(
            session
                .locate_structure(
                    &villages,
                    [0, 64, 0],
                    &spyglass::LocateOptions {
                        radius: spyglass::LOCATE_RADIUS,
                        confirm: false,
                    },
                )
                .unwrap(),
        );
    });
    report("locate_structure #village (from text)", 10, || {
        let query = world.structure_query("#minecraft:village").unwrap();
        black_box(
            session
                .locate_structure(
                    &query,
                    [0, 64, 0],
                    &spyglass::LocateOptions {
                        radius: spyglass::LOCATE_RADIUS,
                        confirm: false,
                    },
                )
                .unwrap(),
        );
    });
    report(
        "locate_structure #village confirmed, new session",
        10,
        || {
            let session = world.session(*seeds.next().unwrap()).unwrap();
            black_box(
                session
                    .locate_structure(&villages, [0, 64, 0], &spyglass::LocateOptions::default())
                    .unwrap(),
            );
        },
    );
    // The nearest beside /locate's answer, confirmed, over the same seeds.
    for target in [
        "#minecraft:village",
        "minecraft:ancient_city",
        "minecraft:mansion",
    ] {
        let query = world.structure_query(target)?;
        let options = spyglass::LocateOptions::default();
        let mut seeds = SEEDS.iter().cycle();
        report(&format!("locate_structure {target} confirmed"), 5, || {
            let session = world.session(*seeds.next().unwrap()).unwrap();
            black_box(
                session
                    .locate_structure(&query, [0, 64, 0], &options)
                    .unwrap(),
            );
        });
        let mut seeds = SEEDS.iter().cycle();
        report(&format!("nearest_structure {target} confirmed"), 5, || {
            let session = world.session(*seeds.next().unwrap()).unwrap();
            black_box(
                session
                    .nearest_structure(&query, [0, 64, 0], &options)
                    .unwrap(),
            );
        });
    }
    report("structure_layout village_plains, bound", 5, || {
        black_box(
            session
                .structure_layout("minecraft:village_plains", [58, -48])
                .unwrap(),
        );
    });
    let stronghold = world.structure_query("minecraft:stronghold")?;
    report("locate_structure stronghold, new session", 5, || {
        let session = world.session(*seeds.next().unwrap()).unwrap();
        black_box(
            session
                .locate_structure(
                    &stronghold,
                    [0, 64, 0],
                    &spyglass::LocateOptions {
                        radius: spyglass::LOCATE_RADIUS,
                        confirm: false,
                    },
                )
                .unwrap(),
        );
    });
    let cherry = world.biome_query("minecraft:cherry_grove")?;
    report("locate_biome cherry_grove", 3, || {
        black_box(
            session
                .locate_biome(&cherry, [0, 64, 0], spyglass::LOCATE_BIOME_RADIUS)
                .unwrap(),
        );
    });
    report("biome_at, 256 x 256 quarts", 3, || {
        for z in 0..256 {
            for x in 0..256 {
                black_box(session.biome_at([x * 4, 64, z * 4]));
            }
        }
    });
    for scale in [1, 4, 64] {
        let tile = spyglass::MapArea::centred_on([0, 0], [256, 256], scale, 64)?;
        report(
            &format!("biome_map, 256 x 256 tile at scale {scale}"),
            3,
            || {
                black_box(session.biome_map(&tile).unwrap());
            },
        );
    }
    for scale in [1, 4] {
        let tile = spyglass::MapArea::centred_on([0, 0], [256, 256], scale, 64)?;
        report(
            &format!("height_map, 256 x 256 tile at scale {scale}"),
            1,
            || {
                black_box(session.height_map(&tile).unwrap());
            },
        );
    }
    viewer_tiles(&world, &mut seeds)?;
    let view = spyglass::MapArea::centred_on([0, 0], [1024, 1024], 16, 64)?;
    let layers = spyglass::Layers {
        structures: vec![villages.clone(), stronghold.clone()],
        slime_chunks: true,
        spawn: true,
        ..spyglass::Layers::default()
    };
    report("render 1024 x 1024 at scale 16, all layers", 3, || {
        black_box(session.render(&view, &layers).unwrap());
    });
    report("slime_chunks, 1000 x 1000 chunks", 3, || {
        black_box(session.slime_chunks([-500, -500], [499, 499]));
    });
    report("spawn_origin, new session", 5, || {
        let session = world.session(*seeds.next().unwrap()).unwrap();
        black_box(session.spawn_origin());
    });
    report("height_at one column, new session", 10, || {
        let session = world.session(*seeds.next().unwrap()).unwrap();
        black_box(session.height_at([100, -200], spyglass::Heightmap::WorldSurfaceWg));
    });
    let heights = world.session(SEEDS[0])?;
    heights.height_at([0, 0], spyglass::Heightmap::WorldSurfaceWg);
    // A column far from every other, so nothing the session remembers helps.
    let mut far = 0;
    report("height_at one column, bound", 20, || {
        far += 10_000;
        black_box(heights.height_at([far, -200], spyglass::Heightmap::WorldSurfaceWg));
    });
    // Columns next to the last, as structure checks and jigsaw assembly ask for them.
    let mut near = 0;
    report("height_at one nearby column, bound", 20, || {
        near += 3;
        black_box(heights.height_at([near, 5000], spyglass::Heightmap::WorldSurfaceWg));
    });
    report("heights_in 40 x 25 columns", 3, || {
        black_box(heights.heights_in([0, 0], [39, 24], spyglass::Heightmap::OceanFloorWg)).unwrap();
    });
    // Seed search: one constraint of each kind, judged in a new session (what a search's
    // full check costs per seed), then whole searches.
    let kinds = [
        (
            "swamp hut count",
            "require 2 minecraft:swamp_hut within 1000 of 0,0",
        ),
        (
            "village near an outpost",
            "require #minecraft:village within 300 of minecraft:pillager_outpost within 1500 of 0,0",
        ),
        (
            "swamp hut cluster",
            "require cluster 2 minecraft:swamp_hut within 300 of each other within 2000 of 0,0",
        ),
        ("slime chunks", "require 4 slime within 128 of 0,0"),
        ("spawn", "require spawn within 300 of 0,0"),
        (
            "near spawn",
            "require 2 minecraft:swamp_hut within 1000 of spawn",
        ),
        (
            "biome presence, 500",
            "require biome minecraft:cherry_grove within 500 of 0,0",
        ),
        (
            "biome coverage, 1000 step 32",
            "require biome minecraft:plains covers 30% within 1000 of 0,0 step 32",
        ),
        (
            "stronghold",
            "require minecraft:stronghold within 1500 of 0,0",
        ),
    ];
    for (name, text) in kinds {
        let query = world.search_query(text)?;
        report(&format!("evaluate {name}, new session"), 5, || {
            let session = world.session(*seeds.next().unwrap()).unwrap();
            black_box(session.evaluate(&query, false).unwrap());
        });
    }
    let huts = world.search_query(kinds[0].1)?;
    report("evaluate swamp hut count confirmed, new session", 5, || {
        let session = world.session(*seeds.next().unwrap()).unwrap();
        black_box(session.evaluate(&huts, true).unwrap());
    });
    // Six huts can't fit within 300 blocks, so placement rejects every lower 48 bits.
    let impossible = world.search_query("require 6 minecraft:swamp_hut within 300 of 0,0")?;
    rate_world(
        &world,
        "search, placement rejects all, 1 thread",
        &impossible,
        spyglass::SeedSource::All,
        1 << 32,
        Some(1),
        false,
    );
    // The witch-hut farm query, unconfirmed and confirmed.
    let witch = world.search_query("require 2 minecraft:swamp_hut within 400 of 0,0")?;
    for confirm in [false, true] {
        let mode = if confirm { "confirmed" } else { "unconfirmed" };
        for (threads, label, count) in [
            (Some(1), "1 thread", 1 << 12),
            (None, "all threads", 1 << 16),
        ] {
            rate_world(
                &world,
                &format!("search, witch huts {mode}, {label}"),
                &witch,
                spyglass::SeedSource::Range {
                    first: 0,
                    last: i64::MAX,
                },
                count,
                threads,
                confirm,
            );
        }
    }
    let mixed = world.search_query(
        "require 2 minecraft:swamp_hut within 400 of 0,0\n\
         prefer biome minecraft:cherry_grove within 300 of 0,0\n\
         prefer 3 #minecraft:village within 600 of 0,0",
    )?;
    // A range, so each seed is prefiltered on its own and most are checked.
    let range = spyglass::SeedSource::Range {
        first: 0,
        last: i64::MAX,
    };
    rate_world(
        &world,
        "search, huts and preferences, 1 thread",
        &mixed,
        range.clone(),
        1 << 11,
        Some(1),
        false,
    );
    rate_world(
        &world,
        "search, huts and preferences, all threads",
        &mixed,
        range,
        1 << 15,
        None,
        false,
    );
    // The Nether and the End: opening and binding, a fortress locate, and a search whose
    // constraints are in two dimensions.
    for dimension in [Dimension::Nether, Dimension::End] {
        report(&format!("World::open_dimension {dimension}"), 3, || {
            black_box(World::open_dimension(&dir, dimension).unwrap());
        });
    }
    let game = Game::open(&dir)?;
    let nether = game.world(Dimension::Nether)?;
    let end = game.world(Dimension::End)?;
    report("Nether World::session", 20, || {
        black_box(nether.session(*seeds.next().unwrap()).unwrap());
    });
    report("End World::session", 20, || {
        black_box(end.session(*seeds.next().unwrap()).unwrap());
    });
    let fortress = nether.structure_query("minecraft:fortress")?;
    for confirm in [false, true] {
        let mode = if confirm { "confirmed" } else { "unconfirmed" };
        report(
            &format!("locate_structure fortress {mode}, new session"),
            5,
            || {
                let session = nether.session(*seeds.next().unwrap()).unwrap();
                let options = spyglass::LocateOptions {
                    radius: spyglass::LOCATE_RADIUS,
                    confirm,
                };
                black_box(
                    session
                        .locate_structure(&fortress, [0, 64, 0], &options)
                        .unwrap(),
                );
            },
        );
    }
    let portal = game.search_query(
        "require minecraft:fortress within 200 of spawn\n\
         prefer #minecraft:village within 400 of spawn",
    )?;
    report(
        "evaluate fortress near spawn's portal, new session",
        5,
        || {
            let session = game.session(*seeds.next().unwrap());
            black_box(session.evaluate(&portal, false).unwrap());
        },
    );
    for (threads, label, count) in [
        (Some(1), "1 thread", 1 << 11),
        (None, "all threads", 1 << 15),
    ] {
        rate(
            &format!("search, fortress near spawn's portal, {label}"),
            spyglass::SeedSource::Range {
                first: 0,
                last: i64::MAX,
            },
            count,
            threads,
            false,
            |options| {
                game.search(&portal, options, |_| std::ops::ControlFlow::Continue(()))
                    .unwrap()
            },
        );
    }
    Ok(())
}

/// The work the viewer asks for: one 256 × 256 tile, in a new session and then for the tile
/// east of it in the same session, since the viewer's workers share one session per seed.
fn viewer_tiles<'a>(
    world: &World,
    seeds: &mut impl Iterator<Item = &'a i64>,
) -> spyglass::Result<()> {
    let tile = |scale: u32, x: i32| MapArea {
        min: [x * 256 * scale as i32, 0],
        size: [256, 256],
        scale,
        y: 64,
    };
    let mut both = |name: &str, scales: &[u32], runs: usize, f: &dyn Fn(&Session<'_>, &MapArea)| {
        for &scale in scales {
            for (place, warm) in [("new session", false), ("next tile", true)] {
                report_after(
                    &format!("tile {name} at 1:{scale}, {place}"),
                    runs,
                    || {
                        let session = world.session(*seeds.next().unwrap()).unwrap();
                        if warm {
                            f(&session, &tile(scale, 0));
                        }
                        session
                    },
                    |session| f(session, &tile(scale, 1)),
                );
            }
        }
    };
    both("biomes at y 64", &[4, 16], 5, &|session, area| {
        black_box(session.biome_map(area).unwrap());
    });
    both("relief", &[4], 3, &|session, area| {
        black_box(session.height_map(area).unwrap());
    });
    both("biomes at the surface", &[4], 3, &|session, area| {
        let heights = session.height_map(area).unwrap();
        black_box(session.surface_biome_map(&heights).unwrap());
    });
    for id in ["minecraft:swamp_hut", "#minecraft:village"] {
        let query = world.structure_query(id)?;
        for confirm in [false, true] {
            let mode = if confirm { "confirmed" } else { "unconfirmed" };
            let layers = Layers {
                biomes: false,
                structures: vec![query.clone()],
                confirm,
                ..Layers::default()
            };
            both(&format!("{id} {mode}"), &[4, 16], 3, &|session, area| {
                black_box(session.map_features(area, &layers).unwrap());
            });
        }
    }
    Ok(())
}

/// [`rate`] for a search in one world.
fn rate_world(
    world: &World,
    name: &str,
    query: &spyglass::SearchQuery,
    source: spyglass::SeedSource,
    count: u64,
    threads: Option<usize>,
    confirm: bool,
) {
    rate(name, source, count, threads, confirm, |options| {
        world
            .search(query, options, |_| std::ops::ControlFlow::Continue(()))
            .unwrap()
    });
}

/// Searches the first `count` seeds of `source` with `search` and prints seeds per second.
fn rate(
    name: &str,
    source: spyglass::SeedSource,
    count: u64,
    threads: Option<usize>,
    confirm: bool,
    search: impl Fn(&spyglass::SearchOptions) -> spyglass::SearchOutcome,
) {
    if !wanted(name) {
        return;
    }
    let options = spyglass::SearchOptions {
        source,
        seeds: Some(count),
        threads,
        confirm,
        ..spyglass::SearchOptions::default()
    };
    let start = Instant::now();
    let outcome = search(&options);
    let seconds = start.elapsed().as_secs_f64();
    let p = &outcome.progress;
    println!(
        "{name:<50} {:>10.0} seeds/s ({} checked, {} reported)",
        count as f64 / seconds,
        p.checked,
        p.reported
    );
}

/// Whether `--only` lets the line `name` run.
fn wanted(name: &str) -> bool {
    ONLY.get()
        .and_then(Option::as_deref)
        .is_none_or(|only| name.contains(only))
}

/// Runs `f` `runs` times and prints the median.
fn report(name: &str, runs: usize, mut f: impl FnMut()) {
    report_after(name, runs, || (), |()| f());
}

/// Runs `f` on what `setup` gives, `runs` times, and prints the median time of `f` alone.
fn report_after<T>(name: &str, runs: usize, mut setup: impl FnMut() -> T, mut f: impl FnMut(&T)) {
    if !wanted(name) {
        return;
    }
    let mut times: Vec<Duration> = (0..runs)
        .map(|_| {
            let input = setup();
            let start = Instant::now();
            f(&input);
            start.elapsed()
        })
        .collect();
    times.sort();
    let median = times[runs / 2];
    println!("{name:<50} {:>10.3} ms", median.as_secs_f64() * 1000.0);
}
