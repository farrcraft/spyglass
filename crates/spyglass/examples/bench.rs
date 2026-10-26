//! Times the public API's main operations, for comparing changes: `cargo xtask bench`, or
//! `cargo run --release -p spyglass --example bench -- [bundle dir]`. Needs a bundle (default
//! `bundles/26.3`). Each line is the median of several runs.

use std::hint::black_box;
use std::time::{Duration, Instant};

use spyglass::World;

const SEEDS: [i64; 5] = [
    0,
    1,
    -1_556_512_960,
    123_456_789,
    -4_893_098_694_207_956_070,
];

fn main() -> spyglass::Result<()> {
    let dir = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "bundles/26.3".to_string());
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
                .locate_structure(&villages, [0, 64, 0], spyglass::LOCATE_RADIUS)
                .unwrap(),
        );
    });
    report("locate_structure #village (from text)", 10, || {
        let query = world.structure_query("#minecraft:village").unwrap();
        black_box(
            session
                .locate_structure(&query, [0, 64, 0], spyglass::LOCATE_RADIUS)
                .unwrap(),
        );
    });
    let stronghold = world.structure_query("minecraft:stronghold")?;
    report("locate_structure stronghold, new session", 5, || {
        let session = world.session(*seeds.next().unwrap()).unwrap();
        black_box(
            session
                .locate_structure(&stronghold, [0, 64, 0], spyglass::LOCATE_RADIUS)
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
    let huts = world.search_query(
        "require 2 minecraft:swamp_hut within 600 of spawn
         require cluster 2 minecraft:swamp_hut within 200 of each other within 600 of spawn
         prefer 3 #minecraft:village within 400 of spawn
         prefer biome minecraft:cherry_grove within 500 of spawn",
    )?;
    report("evaluate swamp huts near spawn, new session", 5, || {
        let session = world.session(*seeds.next().unwrap()).unwrap();
        black_box(session.evaluate(&huts).unwrap());
    });
    Ok(())
}

/// Runs `f` `runs` times and prints the median.
fn report(name: &str, runs: usize, mut f: impl FnMut()) {
    let mut times: Vec<Duration> = (0..runs)
        .map(|_| {
            let start = Instant::now();
            f();
            start.elapsed()
        })
        .collect();
    times.sort();
    let median = times[runs / 2];
    println!("{name:<45} {:>10.3} ms", median.as_secs_f64() * 1000.0);
}
