//! `find` queries, biome tags and terrain conditions on hand-written worlds
//! (`spyglass_oracle::fixtures`, ADR 0047). No game data (ADR 0008), so these always run.

use std::ops::ControlFlow;

use serde_json::json;
use spyglass::{
    Error, Feature, FindOptions, FindOutcome, Heightmap, Outcome, SearchStop, Session, World,
};
use spyglass_oracle::fixtures::{self, TestBundle, resource};

fn run(session: &Session<'_>, world: &World, text: &str, options: &FindOptions) -> FindOutcome {
    let query = world.search_query(text).unwrap();
    session
        .find(&query, options, |_| ControlFlow::Continue(()))
        .unwrap()
}

fn options() -> FindOptions {
    FindOptions {
        top: 10_000,
        ..FindOptions::default()
    }
}

fn distance_sqr(a: [i32; 2], b: [i32; 2]) -> i64 {
    let d = |i: usize| i64::from(a[i]) - i64::from(b[i]);
    d(0) * d(0) + d(1) * d(1)
}

#[test]
fn matches_are_the_targets_that_meet_the_query_nearest_first() {
    let fixture = fixtures::structured_overworld(false);
    let world = World::open(fixture.path()).unwrap();
    let towers = world.structure_query("minecraft:tower").unwrap();
    let text = "find minecraft:tower within 1500\nrequire minecraft:tower within 200 of here";
    for seed in [3, 12345] {
        let session = world.session(seed).unwrap();
        let origin = [-700, 40];
        let outcome = run(
            &session,
            &world,
            text,
            &FindOptions {
                origin,
                ..options()
            },
        );
        assert!(outcome.complete);
        // By hand: every tower start in the disc, nearest first, with another start within
        // 200 blocks of it (itself not counted).
        let chunks = |c: i32| (c - 1800).div_euclid(16)..=(c + 1800).div_euclid(16);
        let x = chunks(origin[0]);
        let z = chunks(origin[1]);
        let starts = session
            .structures_in(
                &towers,
                [*x.start(), *z.start()],
                [*x.end(), *z.end()],
                false,
            )
            .unwrap();
        let xz = |s: &spyglass::StructureStart| [s.position[0], s.position[2]];
        let mut expected: Vec<_> = starts
            .iter()
            .filter(|t| distance_sqr(xz(t), origin) <= 1500 * 1500)
            .filter(|t| {
                starts
                    .iter()
                    .any(|o| o.chunk != t.chunk && distance_sqr(xz(o), xz(t)) <= 200 * 200)
            })
            .map(xz)
            .collect();
        expected.sort_by_key(|&p| (distance_sqr(p, origin), p[1], p[0]));
        let found: Vec<[i32; 2]> = outcome.matches.iter().map(|m| m.position).collect();
        assert_eq!(found, expected, "seed {seed}");
        assert!(!found.is_empty(), "seed {seed}: no pairs to test with");
        for m in &outcome.matches {
            assert!(matches!(&m.target, Some(Feature::Structure(s)) if xz(s) == m.position));
            assert_eq!(m.evaluation.verdict, Outcome::Met);
            assert_eq!(
                i64::from(m.distance),
                distance_sqr(m.position, origin).isqrt()
            );
        }
    }
}

#[test]
fn threads_and_stops_keep_the_order() {
    let fixture = fixtures::structured_overworld(false);
    let world = World::open(fixture.path()).unwrap();
    let session = world.session(7).unwrap();
    let text = "find minecraft:tower within 3000 of -500,0\n\
                prefer minecraft:hut within 300 of here";
    let one = run(
        &session,
        &world,
        text,
        &FindOptions {
            threads: Some(1),
            ..options()
        },
    );
    let eight = run(
        &session,
        &world,
        text,
        &FindOptions {
            threads: Some(8),
            ..options()
        },
    );
    assert_eq!(one.matches, eight.matches);
    assert!(one.matches.len() > 20);
    // Only the first batch, then a break: a prefix of the full answer.
    let query = world.search_query(text).unwrap();
    let cut = session
        .find(&query, &options(), |_| ControlFlow::Break(()))
        .unwrap();
    assert!(!cut.complete);
    assert!(!cut.matches.is_empty());
    assert_eq!(cut.matches[..], one.matches[..cut.matches.len()]);
    // Stopped before it starts: nothing.
    let stop = SearchStop::new();
    stop.stop();
    let stopped = run(&session, &world, text, &FindOptions { stop, ..options() });
    assert!(!stopped.complete && stopped.matches.is_empty());
    // `top` keeps the nearest.
    let few = run(
        &session,
        &world,
        text,
        &FindOptions {
            top: 3,
            ..options()
        },
    );
    assert!(few.complete);
    assert_eq!(few.matches[..], one.matches[..3]);
}

#[test]
fn grid_targets_are_kept_apart() {
    let fixture = fixtures::structured_overworld(false);
    let world = World::open(fixture.path()).unwrap();
    let session = world.session(1).unwrap();
    // Desert is east of x = 0.
    let outcome = run(
        &session,
        &world,
        "find biome minecraft:desert within 1000 of -300,0 step 32",
        &options(),
    );
    let positions: Vec<[i32; 2]> = outcome.matches.iter().map(|m| m.position).collect();
    assert!(positions.len() > 3);
    for (i, a) in positions.iter().enumerate() {
        assert!(a[0] >= 0, "{a:?}");
        for b in &positions[..i] {
            assert!(distance_sqr(*a, *b) > 256 * 256, "{a:?} and {b:?}");
        }
    }
    // The first is the nearest desert sample on the grid: x = 0 or just east, at z = 0.
    assert_eq!(positions[0][1], 0);
    let biome = session.biome_at([positions[0][0], 64, 0]);
    assert_eq!(biome.biome, "minecraft:desert");
    // `apart 0` keeps every sample.
    let all = run(
        &session,
        &world,
        "find biome minecraft:desert within 200 of 100,0 step 32 apart 0",
        &options(),
    );
    assert_eq!(
        all.matches.len(),
        run(
            &session,
            &world,
            "find point within 200 of 100,0 step 32 apart 0\nrequire biome desert within 0 of here",
            &FindOptions {
                top: 10_000,
                ..FindOptions::default()
            }
        )
        .matches
        .len()
    );
}

/// The structured fixture with ground below `y = 50 + x/4 + z/8`: out of the sea (63) east of
/// about x = 52.
fn sloped() -> TestBundle {
    let gradient = |axis: &str, from: i32, to: i32, from_value: f64, to_value: f64| {
        json!({"type": "minecraft:gradient", "axis": axis, "from_coordinate": from,
            "from_value": from_value, "to_coordinate": to, "to_value": to_value})
    };
    let constant = |v: f64| json!({"type": "minecraft:constant", "value": v});
    fixtures::overworld(
        &[resource(
            "worldgen/noise_settings",
            "overworld",
            json!({
                "sea_level": 63,
                "noise": {"min_y": -64, "height": 384},
                "default_block": "minecraft:stone",
                "default_fluid": "minecraft:water",
                "noise_router": {
                    "final_density": {"type": "minecraft:add",
                        "left": gradient("y", 0, 100, 1.0, -1.0),
                        "right": {"type": "minecraft:add",
                            "left": gradient("x", 0, 200, 0.0, 1.0),
                            "right": gradient("z", 0, 400, 0.0, 1.0)}},
                    "temperature": gradient("x", -1000, 1001, -1.0, 1.0),
                    "vegetation": constant(0.0),
                    "continents": constant(0.0),
                    "erosion": "minecraft:zero",
                    "depth": 0.0,
                    "ridges": 0.0
                },
                "spawn_target": [{"minecraft:zero": [-1.0, 1.0]}]
            }),
        )],
        false,
    )
}

#[test]
fn terrain_conditions_follow_the_heights() {
    let fixture = sloped();
    let world = World::open(fixture.path()).unwrap();
    let session = world.session(0).unwrap();
    let height = |p: [i32; 2]| session.height_at(p, Heightmap::WorldSurfaceWg);
    let grid: Vec<[i32; 2]> = (-4..=4)
        .flat_map(|j| (-4..=4).map(move |i| [100 + 32 * i, 32 * j]))
        .filter(|p| distance_sqr(*p, [100, 0]) <= 128 * 128)
        .collect();
    // Heights from 70 to 80 at the target itself.
    let outcome = run(
        &session,
        &world,
        "find point within 128 of 100,0 step 32 apart 0\nrequire height 70 to 80 within 0 of here",
        &options(),
    );
    let mut expected: Vec<[i32; 2]> = grid
        .iter()
        .copied()
        .filter(|&p| (70..=80).contains(&height(p)))
        .collect();
    expected.sort_by_key(|&p| (distance_sqr(p, [100, 0]), p[1], p[0]));
    let found: Vec<[i32; 2]> = outcome.matches.iter().map(|m| m.position).collect();
    assert_eq!(found, expected);
    assert!(!found.is_empty() && found.len() < grid.len(), "{found:?}");
    // Flat: the 3 × 3 grid at spacing 8 around each point varies by at most 2 blocks.
    let outcome = run(
        &session,
        &world,
        "find point within 128 of 100,0 step 32 apart 0\nrequire flat 2 in -8,-8 to 8,8 step 8",
        &options(),
    );
    let around: Vec<i32> = (-1..=1)
        .flat_map(|j| (-1..=1).map(move |i| [8 * i, 8 * j]))
        .map(height)
        .collect();
    let flat_at_origin = around.iter().max().unwrap() - around.iter().min().unwrap() <= 2;
    // The box isn't around `here`: every target or none.
    assert_eq!(
        outcome.matches.len(),
        if flat_at_origin { grid.len() } else { 0 }
    );
    let outcome = run(
        &session,
        &world,
        "find point within 128 of 100,0 step 32 apart 0\nprefer flat 2 within 16 of here step 8",
        &options(),
    );
    // Preferred, so every target matches, scored by how flat its surroundings are.
    assert_eq!(outcome.matches.len(), grid.len());
    for m in &outcome.matches {
        let p = m.position;
        let hs: Vec<i32> = (-2..=2)
            .flat_map(|j| (-2..=2).map(move |i| [p[0] + 8 * i, p[1] + 8 * j]))
            .filter(|q| distance_sqr(*q, p) <= 16 * 16)
            .map(height)
            .collect();
        let spread = (hs.iter().max().unwrap() - hs.iter().min().unwrap()) as u64;
        let expected = if spread <= 2 { 1000 } else { 2000 / spread };
        assert_eq!(m.score, expected, "{p:?}: {hs:?}");
    }
}

#[test]
fn biome_tags_name_their_members() {
    let fixture = fixtures::overworld(
        &[
            resource(
                "tags/worldgen/biome",
                "warm",
                json!({"values": ["minecraft:desert", "#minecraft:more"]}),
            ),
            resource(
                "tags/worldgen/biome",
                "more",
                json!({"values": [
                    {"id": "minecraft:nowhere", "required": false},
                    {"id": "#minecraft:none", "required": false},
                    "minecraft:forest"
                ]}),
            ),
            resource(
                "tags/worldgen/biome",
                "broken",
                json!({"values": ["minecraft:nowhere"]}),
            ),
            resource(
                "tags/worldgen/biome",
                "loop",
                json!({"values": ["#minecraft:loop"]}),
            ),
        ],
        false,
    );
    let world = World::open(fixture.path()).unwrap();
    let session = world.session(5).unwrap();
    let warm = world.biome_query("#minecraft:warm").unwrap();
    assert_eq!(warm.id(), "#minecraft:warm");
    // /locate biome with a tag, as the game takes one: desert is east of x = 0.
    let found = session.locate_biome(&warm, [-600, 64, 0], 6400).unwrap();
    let last = found.hits.last().unwrap();
    assert!(last.certain && last.position[0] >= 0);
    // In both kinds of search.
    let query = world
        .search_query(
            "require biome #warm within 0 of 500,0\nrequire no biome #warm within 0 of -500,0",
        )
        .unwrap();
    assert_eq!(
        session.evaluate(&query, false).unwrap().verdict,
        Outcome::Met
    );
    // Missing members, missing tags and cycles are errors.
    for (tag, what) in [
        ("#minecraft:broken", "unknown biome"),
        ("#minecraft:absent", "unknown biome tag"),
        ("#minecraft:loop", "includes itself"),
    ] {
        let e = world.biome_query(tag).unwrap_err().to_string();
        assert!(e.contains(what), "{tag}: {e}");
    }
}

#[test]
fn find_queries_and_the_seed_search_keep_apart() {
    let fixture = fixtures::structured_overworld(false);
    let world = World::open(fixture.path()).unwrap();
    let session = world.session(0).unwrap();
    let find = world
        .search_query("find minecraft:tower within 100\nrequire minecraft:hut within 50 of here")
        .unwrap();
    let e = session.evaluate(&find, false).unwrap_err();
    assert!(matches!(e, Error::InvalidQuery { line: 1, .. }), "{e}");
    let e = world
        .search(&find, &spyglass::SearchOptions::default(), |_| {
            ControlFlow::Continue(())
        })
        .unwrap_err();
    assert!(matches!(e, Error::InvalidQuery { line: 1, .. }), "{e}");
    let seeds = world
        .search_query("require minecraft:tower within 100 of spawn")
        .unwrap();
    let e = session
        .find(&seeds, &FindOptions::default(), |_| {
            ControlFlow::Continue(())
        })
        .unwrap_err();
    assert!(e.to_string().contains("no `find` line"), "{e}");
    // `here` needs a find line, and a terrain condition a sky.
    let e = world
        .search_query("require minecraft:tower within 100 of here")
        .unwrap_err();
    assert!(
        matches!(
            e,
            Error::InvalidQuery {
                line: 1,
                column: Some(_),
                ..
            }
        ),
        "{e}"
    );
}

#[test]
fn terrain_is_not_checked_under_a_ceiling() {
    let fixture = fixtures::overworld_and_nether();
    let nether = World::open_dimension(fixture.path(), spyglass::Dimension::Nether).unwrap();
    assert!(nether.has_ceiling());
    let e = nether
        .search_query("require flat 3 within 16 of 0,0")
        .unwrap_err()
        .to_string();
    assert!(e.contains("ceiling"), "{e}");
}

/// The viewer's presets, as the milestone's checks use them.
const PRESETS: [(&str, i64); 5] = [
    (
        "find minecraft:swamp_hut within 4096\nrequire minecraft:swamp_hut within 200 of here",
        59,
    ),
    (
        "find minecraft:monument within 4096\n\
         require biome #minecraft:required_ocean_monument_surrounding covers 100% within 128 of here",
        12345,
    ),
    (
        "find minecraft:pillager_outpost within 4096\nrequire flat 4 within 48 of here step 4",
        12345,
    ),
    (
        "find biome minecraft:mushroom_fields within 6000 step 32",
        1,
    ),
    (
        "find point within 4096 step 64\n\
         require biome minecraft:plains within 0 of here\n\
         require biome minecraft:cherry_grove within 200 of here\n\
         require biome #minecraft:is_ocean within 300 of here",
        12345,
    ),
];

#[test]
#[ignore = "needs bundles/<version> (SPYGLASS_ORACLE_VERSION, else the newest release)"]
fn presets_agree_with_the_library_on_real_data() {
    let world = World::open(spyglass_oracle::bundle_dir()).unwrap();
    for (text, seed) in PRESETS {
        let session = world.session(seed).unwrap();
        let options = FindOptions {
            top: 3,
            confirm: true,
            ..FindOptions::default()
        };
        let outcome = run(&session, &world, text, &options);
        let first = outcome
            .matches
            .first()
            .unwrap_or_else(|| panic!("seed {seed}: nothing found for\n{text}"));
        let [x, z] = first.position;
        // The same constraints around the match's block, as a seed search's query.
        let fixed: String = text
            .lines()
            .skip(1)
            .map(|l| l.replace(" of here", &format!(" of {x},{z}")) + "\n")
            .collect();
        // Around a point, a structure target's own start would count too, so the hut pair is
        // checked by hand below instead.
        if !fixed.is_empty() && !text.contains("swamp_hut within") {
            let query = world.search_query(&fixed).unwrap();
            let evaluation = session.evaluate(&query, true).unwrap();
            assert_eq!(
                evaluation.constraints, first.evaluation.constraints,
                "seed {seed}:\n{text}"
            );
        }
        // And by hand, where a constraint asks something the library answers directly.
        if text.contains("flat 4") {
            let heights: Vec<i32> = (-12..=12)
                .flat_map(|j| (-12..=12).map(move |i| [x + 4 * i, z + 4 * j]))
                .filter(|p| distance_sqr(*p, [x, z]) <= 48 * 48)
                .map(|p| session.height_at(p, Heightmap::WorldSurfaceWg))
                .collect();
            let spread = heights.iter().max().unwrap() - heights.iter().min().unwrap();
            assert!(spread <= 4, "seed {seed}: heights vary by {spread}");
        }
        if text.contains("plains within 0") {
            assert_eq!(session.biome_at([x, 64, z]).biome, "minecraft:plains");
        }
        if text.contains("swamp_hut within 200") {
            let huts = world.structure_query("minecraft:swamp_hut").unwrap();
            let around = session
                .structures_in(
                    &huts,
                    [(x - 200) >> 4, (z - 200) >> 4],
                    [(x + 200) >> 4, (z + 200) >> 4],
                    true,
                )
                .unwrap();
            assert!(around.iter().any(|s| {
                [s.position[0], s.position[2]] != [x, z]
                    && distance_sqr([s.position[0], s.position[2]], [x, z]) <= 200 * 200
            }));
        }
        if let Some(Feature::Biome { biome, .. }) = &first.target {
            assert_eq!(biome.biome, "minecraft:mushroom_fields");
        }
        println!(
            "seed {seed}: {} matches, first at {x} {z}, in {:.2} s",
            outcome.matches.len(),
            outcome.progress.elapsed.as_secs_f64()
        );
    }
}
