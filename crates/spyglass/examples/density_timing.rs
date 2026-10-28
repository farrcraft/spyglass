//! Where terrain's time goes: `cargo run --profile dist -p spyglass --features timing --example
//! density_timing -- [bundle dir or version]`. For each scenario, the time per operation in each
//! column stage and density node type, exclusive of what runs inside it, then inclusive. Each
//! runs twice: coarse (volume calls, point samples, column stages), whose totals are close to the
//! real cost, then with a span per density node, which shows the split but slows everything.

use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::time::Instant;

use spyglass::{Heightmap, World, timing};

fn main() -> spyglass::Result<()> {
    let dir: PathBuf = match std::env::args().nth(1) {
        Some(arg) if Path::new(&arg).join("bundle.json").is_file() => arg.into(),
        version => spyglass::find_bundle("bundles", version.as_deref())?.dir,
    };
    println!("bundle: {}", dir.display());
    let world = World::open(&dir)?;
    let session = world.session(0)?;
    session.height_at([0, 0], Heightmap::WorldSurfaceWg);
    for (pass, detail) in [(1, false), (2, true)] {
        timing::set_detail(detail);
        println!("\n##### {}", if detail { "per node" } else { "coarse" });
        scenarios(&world, &session, pass);
    }
    Ok(())
}

/// Every scenario once, away from the columns earlier passes asked about, so per-seed caches
/// don't help.
fn scenarios(world: &World, session: &spyglass::Session<'_>, pass: i32) {
    let away = pass * 3_000_000;
    let mut far = 0;
    scenario("isolated column, bound", 300, || {
        far += 10_000;
        black_box(session.height_at([far, away], Heightmap::WorldSurfaceWg));
    });
    let mut near = 0;
    scenario("nearby column, bound", 300, || {
        near += 3;
        black_box(session.height_at([near, away + 5000], Heightmap::WorldSurfaceWg));
    });
    let mut origin = 0;
    scenario("40 x 25 region, bound", 10, || {
        origin += 100_000;
        black_box(session.heights_in(
            [origin, away],
            [origin + 39, away + 24],
            Heightmap::OceanFloorWg,
        ))
        .unwrap();
    });
    // A village's layout cold (a new session each time) and warm (the same session again).
    let seeds = 1000 * i64::from(pass);
    let mut seed = seeds;
    scenario("village_plains layout, new session", 10, || {
        seed += 1;
        let s = world.session(seed).unwrap();
        black_box(
            s.structure_layout("minecraft:village_plains", [58, -48])
                .unwrap(),
        );
    });
    let warm = world.session(seeds + 500).unwrap();
    warm.structure_layout("minecraft:village_plains", [58, -48])
        .unwrap();
    scenario("village_plains layout, same session", 10, || {
        black_box(
            warm.structure_layout("minecraft:village_plains", [58, -48])
                .unwrap(),
        );
    });
}

/// The cost of one empty span inside another, in microseconds.
fn span_cost() -> f64 {
    const N: u32 = 200_000;
    let outer = timing::span("calibration");
    let start = Instant::now();
    for _ in 0..N {
        drop(black_box(timing::span("calibration: inner")));
    }
    let cost = start.elapsed().as_secs_f64() * 1e6 / f64::from(N);
    drop(outer);
    timing::take();
    cost
}

/// Runs `op` `n` times and prints the timing totals per operation.
fn scenario(name: &str, n: u32, mut op: impl FnMut()) {
    timing::take();
    let start = Instant::now();
    for _ in 0..n {
        op();
    }
    let wall = start.elapsed().as_secs_f64() * 1e6 / f64::from(n);
    let mut totals: Vec<_> = timing::take().into_iter().collect();
    totals.sort_by_key(|(_, t)| std::cmp::Reverse(t.exclusive));
    let per = |d: std::time::Duration| d.as_secs_f64() * 1e6 / f64::from(n);
    let accounted: f64 = totals.iter().map(|(_, t)| per(t.exclusive)).sum();
    let spans: u64 = totals.iter().map(|(_, t)| t.calls).sum();
    let overhead = span_cost() * spans as f64 / f64::from(n);
    println!(
        "\n== {name}: {wall:.1} us per operation, timed spans {accounted:.1} us, about {overhead:.1} us of it timing's own cost ({:.0} spans)",
        spans as f64 / f64::from(n)
    );
    println!(
        "  {:44} {:>10} {:>10} {:>9}",
        "", "exclusive", "inclusive", "calls"
    );
    for (span, t) in totals {
        let share = per(t.exclusive) / wall * 100.0;
        if share < 0.5 && per(t.inclusive) / wall < 0.05 {
            continue;
        }
        println!(
            "  {span:44} {:>8.1}us {:>8.1}us {:>9.1}  {share:4.1}%",
            per(t.exclusive),
            per(t.inclusive),
            t.calls as f64 / f64::from(n),
        );
    }
}
