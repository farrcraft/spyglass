//! Seed search on the hand-written world of `spyglass_oracle::fixtures::structured_overworld`
//! (`docs/specs/search.md`). No game data (ADR 0008), so these always run.
//!
//! Each test works out the expected answer from the session's own queries and the spec's
//! formulas, independently of `Session::evaluate`.

use std::ops::ControlFlow;

use spyglass::{
    Error, Feature, Outcome, SearchOptions, SearchOutcome, SeedSource, Session, StructureStart,
    World,
};
use spyglass_oracle::fixtures;

#[test]
fn queries_resolve_and_errors_name_lines() {
    let fixture = fixtures::structured_overworld(false);
    let world = World::open(fixture.path()).unwrap();
    let query = world
        .search_query(
            "# huts\nrequire 2 hut within 600 of spawn\nprefer #everything in 0,0 to 9,9\n",
        )
        .unwrap();
    assert_eq!(
        query.to_string(),
        "require 2 minecraft:hut within 600 of spawn\nprefer #minecraft:everything in 0,0 to 9,9\n"
    );
    let e = world
        .search_query("\nrequire minecraft:nothing within 1 of spawn")
        .unwrap_err();
    assert!(
        matches!(
            e,
            Error::InvalidQuery {
                line: 2,
                column: None,
                ..
            }
        ),
        "{e}"
    );
    assert_eq!(
        e.to_string(),
        "line 2: unknown structure \"minecraft:nothing\""
    );
    let e = world
        .search_query("require hut within 1 of #minecraft:nothing in 0,0 to 1,1")
        .unwrap_err();
    assert_eq!(
        e.to_string(),
        "line 1: unknown structure tag \"#minecraft:nothing\""
    );
    let e = world
        .search_query("require biome minecraft:nothing within 1 of spawn")
        .unwrap_err();
    assert_eq!(e.to_string(), "line 1: unknown biome \"minecraft:nothing\"");
    let e = world
        .search_query("require biome plains within 1 of spawn at y 400")
        .unwrap_err();
    assert_eq!(
        e.to_string(),
        "line 1: y 400 is outside the build height, -64 to 319"
    );
    let e = world.search_query("require nothing").unwrap_err();
    assert!(
        matches!(
            e,
            Error::InvalidQuery {
                line: 1,
                column: Some(16),
                ..
            }
        ),
        "{e}"
    );
    assert_eq!(
        e.to_string(),
        "line 1, column 16: expected `within` or `in` at the end of the line"
    );
}

/// `isqrt(dx² + dz²)` if at most `r`.
fn distance(a: [i32; 2], b: [i32; 2], r: u32) -> Option<u32> {
    let d = |i: usize| i64::from(a[i]) - i64::from(b[i]);
    let d2 = (d(0) * d(0) + d(1) * d(1)).unsigned_abs();
    (d2 <= u64::from(r).pow(2)).then(|| d2.isqrt() as u32)
}

/// The spec's points for a feature `d` from the centre of a disc of radius `r`.
fn points(d: u32, r: u32) -> u64 {
    500 + 500 * u64::from(r - d) / u64::from(r)
}

/// "At least `n`" satisfaction from the matches' distances.
fn graded(mut ds: Vec<u32>, n: u32, r: u32) -> u32 {
    ds.sort();
    let sum: u64 = ds.iter().take(n as usize).map(|&d| points(d, r)).sum();
    (sum / u64::from(n)) as u32
}

fn xz(s: &StructureStart) -> [i32; 2] {
    [s.position[0], s.position[2]]
}

/// Every start of `structure` whose position is in the box `min..=max` (blocks), from
/// `structures_in` over a chunk range one chunk wider.
fn starts_in(
    world: &World,
    session: &Session<'_>,
    structure: &str,
    min: [i32; 2],
    max: [i32; 2],
) -> Vec<StructureStart> {
    let q = world.structure_query(structure).unwrap();
    session
        .structures_in(&q, min.map(|b| (b >> 4) - 1), max.map(|b| (b >> 4) + 1))
        .unwrap()
        .into_iter()
        .filter(|s| (0..2).all(|i| (min[i]..=max[i]).contains(&xz(s)[i])))
        .collect()
}

/// Every start of `structure` within `r` blocks of `centre`, with its distance.
fn starts_near(
    world: &World,
    session: &Session<'_>,
    structure: &str,
    centre: [i32; 2],
    r: u32,
) -> Vec<(StructureStart, u32)> {
    let r_i = r as i32;
    starts_in(
        world,
        session,
        structure,
        centre.map(|c| c - r_i),
        centre.map(|c| c + r_i),
    )
    .into_iter()
    .filter_map(|s| distance(centre, xz(&s), r).map(|d| (s, d)))
    .collect()
}

fn outcome(best: bool, worst: bool) -> Outcome {
    match (best, worst) {
        (_, true) => Outcome::Met,
        (true, false) => Outcome::PossiblyMet,
        (false, false) => Outcome::Failed,
    }
}

#[test]
fn structure_counts_agree_with_structures_in() {
    let fixture = fixtures::structured_overworld(false);
    let world = World::open(fixture.path()).unwrap();
    let query = world
        .search_query(
            "require 2 tower within 500 of -200,30\n\
             prefer 3 hut within 400 of -100,0 weight 3\n\
             prefer no tower within 300 of 50,50",
        )
        .unwrap();
    let mut met = 0;
    for seed in 0..30 {
        let session = world.session(seed).unwrap();
        let e = session.evaluate(&query).unwrap();

        let towers = starts_near(&world, &session, "tower", [-200, 30], 500);
        let certain: Vec<u32> = towers.iter().filter(|t| t.0.certain).map(|t| t.1).collect();
        let c = &e.constraints[0];
        assert_eq!(
            c.outcome,
            outcome(towers.len() >= 2, certain.len() >= 2),
            "seed {seed}"
        );
        assert_eq!(c.best, graded(towers.iter().map(|t| t.1).collect(), 2, 500));
        assert_eq!(c.worst, graded(certain, 2, 500));
        assert_eq!(c.features.len(), towers.len());

        let huts = starts_near(&world, &session, "hut", [-100, 0], 400);
        let c = &e.constraints[1];
        // Huts need terrain, so they're never certain.
        assert!(huts.iter().all(|(s, _)| !s.certain));
        assert_eq!(c.outcome, outcome(huts.len() >= 3, false), "seed {seed}");
        assert_eq!(c.best, graded(huts.iter().map(|h| h.1).collect(), 3, 400));
        assert_eq!(c.worst, 0);

        let near = starts_near(&world, &session, "tower", [50, 50], 300);
        let c = &e.constraints[2];
        let certain_near = near.iter().any(|(s, _)| s.certain);
        assert_eq!(c.outcome, outcome(!certain_near, near.is_empty()));
        assert_eq!(c.best, if certain_near { 0 } else { 1000 });
        assert_eq!(c.worst, if near.is_empty() { 1000 } else { 0 });

        assert_eq!(e.verdict, e.constraints[0].outcome);
        assert_eq!(
            e.best_score,
            3 * u64::from(e.constraints[1].best) + u64::from(e.constraints[2].best)
        );
        assert_eq!(
            e.worst_score,
            3 * u64::from(e.constraints[1].worst) + u64::from(e.constraints[2].worst)
        );
        assert_eq!(e.reported(true), e.verdict == Outcome::Met);
        met += usize::from(e.verdict == Outcome::Met);
    }
    assert!(met > 0, "no seed had two certain towers");
}

#[test]
fn structure_anchors_pair_starts() {
    let fixture = fixtures::structured_overworld(false);
    let world = World::open(fixture.path()).unwrap();
    let query = world
        .search_query(
            "require hut within 200 of tower within 600 of -300,0\n\
             require 2 tower within 200 of tower in -900,-300 to -100,300",
        )
        .unwrap();
    let mut paired_seeds = 0;
    for seed in 0..30 {
        let session = world.session(seed).unwrap();
        let e = session.evaluate(&query).unwrap();

        let anchors = starts_near(&world, &session, "tower", [-300, 0], 600);
        let huts = starts_near(&world, &session, "hut", [-300, 0], 800);
        let paired: Vec<u32> = huts
            .iter()
            .filter_map(|(h, _)| {
                anchors
                    .iter()
                    .filter_map(|(a, _)| distance(xz(a), xz(h), 200))
                    .min()
            })
            .collect();
        let c = &e.constraints[0];
        assert_eq!(c.outcome, outcome(!paired.is_empty(), false), "seed {seed}");
        assert_eq!(c.best, graded(paired.clone(), 1, 200));
        paired_seeds += usize::from(!paired.is_empty());

        // A tower is never its own anchor. Anchors are in the box; the towers counted can be
        // up to 200 blocks outside it.
        let anchors = starts_in(&world, &session, "tower", [-900, -300], [-100, 300]);
        let towers = starts_in(&world, &session, "tower", [-1100, -500], [100, 500]);
        let counted = towers
            .iter()
            .filter(|t| {
                anchors
                    .iter()
                    .any(|a| a.chunk != t.chunk && distance(xz(a), xz(t), 200).is_some())
            })
            .count();
        assert_eq!(e.constraints[1].features.len(), counted, "seed {seed}");
    }
    assert!(paired_seeds > 0, "no hut was near a tower");
}

#[test]
fn clusters_slime_and_spawn() {
    let fixture = fixtures::structured_overworld(false);
    let world = World::open(fixture.path()).unwrap();
    for seed in 0..10 {
        let session = world.session(seed).unwrap();
        let spawn = session.spawn_origin();
        let [sx, sz] = spawn.map_block();
        let text = format!(
            "require 3 slime within 100 of spawn\n\
             prefer cluster 2 hut within 300 of each other in -2000,-2000 to -1,2000\n\
             require spawn within 50 of {},{}\n\
             prefer spawn in {sx},{sz} to {sx},{sz}",
            sx + 30,
            sz - 40
        );
        let query = world.search_query(&text).unwrap();
        let e = session.evaluate(&query).unwrap();

        let slime: Vec<u32> = session
            .slime_chunks(
                [(sx - 120) >> 4, (sz - 120) >> 4],
                [(sx + 120) >> 4, (sz + 120) >> 4],
            )
            .into_iter()
            .filter_map(|c| distance([sx, sz], c.map(|v| v * 16 + 8), 100))
            .collect();
        let c = &e.constraints[0];
        assert_eq!(c.outcome, outcome(slime.len() >= 3, slime.len() >= 3));
        let sat = graded(slime, 3, 100);
        assert_eq!((c.best, c.worst), (sat, sat));

        let c = &e.constraints[1];
        let members: Vec<[i32; 2]> = c
            .features
            .iter()
            .map(|f| match f {
                Feature::Structure(s) => xz(s),
                other => panic!("{other:?}"),
            })
            .collect();
        for a in &members {
            for b in &members {
                assert!(distance(*a, *b, 300).is_some());
            }
        }
        assert_eq!(c.best, 1000 * members.len().min(2) as u32 / 2);
        assert_eq!(c.worst, 0, "huts are candidates");

        // (30, -40) away is 50 blocks: the edge of the disc.
        let c = &e.constraints[2];
        assert_eq!((c.outcome, c.best), (Outcome::Met, 500));
        assert_eq!(c.features, [Feature::Spawn(spawn)]);
        assert_eq!(e.constraints[3].best, 1000);
        assert_eq!(e.best_score, u64::from(e.constraints[1].best) + 1000);
    }
}

#[test]
fn biomes_on_the_gradient() {
    let fixture = fixtures::structured_overworld(false);
    let world = World::open(fixture.path()).unwrap();
    let session = world.session(5).unwrap();
    // Plains is the quarts with x < 1: blocks west of x = 4.
    let plains = (-1000..1000)
        .filter(|&x| session.biome_at([x, 64, 0]).biome == "minecraft:plains")
        .count();
    assert_eq!(plains, 1004);
    let query = world
        .search_query(
            "require biome plains within 0 of -100,0\n\
             require biome plains within 0 of 100,0\n\
             require biome plains within 160 of 100,0\n\
             require no biome desert in -1000,-1000 to -10,1000 step 32\n\
             prefer biome plains covers 50% in -1000,0 to 999,0 step 1\n\
             prefer biome plains covers 100% in -1000,0 to 999,0 step 1",
        )
        .unwrap();
    let e = session.evaluate(&query).unwrap();
    let outcomes: Vec<Outcome> = e.constraints.iter().map(|c| c.outcome).collect();
    assert_eq!(
        outcomes,
        [
            Outcome::Met,
            Outcome::Failed,
            Outcome::Met,
            Outcome::Met,
            Outcome::Met,
            Outcome::Failed
        ]
    );
    // The nearest plains sample to (100, 0) on the 16-block grid is (-12, 0), 112 away.
    let c = &e.constraints[2];
    assert_eq!(c.best, points(112, 160) as u32);
    assert!(matches!(
        &c.features[..],
        [Feature::Biome {
            position: [-12, 64, 0],
            certain: true,
            ..
        }]
    ));
    // 1004 of 2000 samples: all of 50%, about half of 100%.
    assert_eq!(e.constraints[4].best, 1000);
    assert_eq!(e.constraints[5].best, 502);
    assert_eq!(e.verdict, Outcome::Failed);
    assert_eq!(e.best_score, 1502);
}

#[test]
fn tied_cells_are_possible_matches() {
    let fixture = fixtures::structured_overworld(true);
    let world = World::open(fixture.path()).unwrap();
    let session = world.session(5).unwrap();
    let query = world
        .search_query(
            "require biome forest within 0 of -100,0\n\
             require no biome plains within 0 of -100,0\n\
             require biome desert within 0 of 100,0\n\
             prefer biome forest covers 10% in -100,0 to 99,0 step 1",
        )
        .unwrap();
    let e = session.evaluate(&query).unwrap();
    let outcomes: Vec<Outcome> = e.constraints.iter().map(|c| c.outcome).collect();
    assert_eq!(
        outcomes,
        [
            Outcome::PossiblyMet,
            Outcome::PossiblyMet,
            Outcome::Met,
            Outcome::PossiblyMet
        ]
    );
    assert_eq!((e.constraints[0].best, e.constraints[0].worst), (1000, 0));
    assert!(matches!(
        &e.constraints[0].features[..],
        [Feature::Biome { certain: false, .. }]
    ));
    assert_eq!(e.verdict, Outcome::PossiblyMet);
    assert!(e.reported(false) && !e.reported(true));
    assert_eq!((e.score(false), e.score(true)), (1000, 0));
}

#[test]
fn queries_belong_to_their_world() {
    let (a, b) = (
        fixtures::structured_overworld(false),
        fixtures::structured_overworld(false),
    );
    let (wa, wb) = (
        World::open(a.path()).unwrap(),
        World::open(b.path()).unwrap(),
    );
    let query = wa.search_query("require hut within 10 of spawn").unwrap();
    let e = wb.session(0).unwrap().evaluate(&query).unwrap_err();
    assert!(matches!(e, Error::OtherWorld(_)), "{e}");
}

#[test]
#[ignore = "needs a bundle: spyglass-extract 26.3"]
fn the_real_world_resolves_and_counts_like_structures_in() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../bundles/26.3");
    let world = World::open(dir).unwrap();
    let spec = include_str!("../../../docs/specs/search.md");
    let example = spec
        .split("### Example\n\n```text\n")
        .nth(1)
        .and_then(|rest| rest.split("```").next())
        .unwrap();
    let query = world.search_query(example).unwrap();
    let huts = world
        .search_query("require 2 minecraft:swamp_hut within 2000 of 300,-700")
        .unwrap();
    let mut seen = [0; 3];
    for seed in 0..20 {
        let session = world.session(seed).unwrap();
        let e = session.evaluate(&query).unwrap();
        assert_eq!(e.constraints.len(), 10);
        let e = session.evaluate(&huts).unwrap();
        let near = starts_near(&world, &session, "minecraft:swamp_hut", [300, -700], 2000);
        let certain = near.iter().filter(|(s, _)| s.certain).count();
        let c = &e.constraints[0];
        assert_eq!(
            c.outcome,
            outcome(near.len() >= 2, certain >= 2),
            "seed {seed}"
        );
        assert_eq!(c.best, graded(near.iter().map(|t| t.1).collect(), 2, 2000));
        seen[c.outcome as usize] += 1;
    }
    assert!(seen[1] > 0, "no seed possibly had two huts: {seen:?}");
}

/// The top `n` of `seeds` by `evaluate`, ranked as the spec says: score, then index.
fn brute_force(
    world: &World,
    query: &spyglass::SearchQuery,
    source: &SeedSource,
    indices: std::ops::Range<u64>,
    n: usize,
    certain: bool,
) -> Vec<(u64, i64, u64)> {
    let mut hits: Vec<(u64, i64, u64)> = indices
        .filter_map(|index| {
            let seed = match source {
                SeedSource::All => index.rotate_right(16) as i64,
                SeedSource::Range { first, .. } => first + index as i64,
                SeedSource::List(seeds) => seeds[index as usize],
            };
            let e = world.session(seed).unwrap().evaluate(query).unwrap();
            e.reported(certain).then(|| (index, seed, e.score(certain)))
        })
        .collect();
    hits.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(&b.0)));
    hits.truncate(n);
    hits
}

fn ranked(outcome: &SearchOutcome) -> Vec<(u64, i64, u64)> {
    outcome
        .hits
        .iter()
        .map(|h| (h.index, h.seed, h.score))
        .collect()
}

fn search(world: &World, query: &spyglass::SearchQuery, options: &SearchOptions) -> SearchOutcome {
    world
        .search(query, options, |_| ControlFlow::Continue(()))
        .unwrap()
}

const RANKED: &str = "require 2 tower within 400 of -200,0\n\
                      prefer 3 hut within 300 of -100,0 weight 2\n\
                      prefer cluster 2 hut within 150 of each other in -600,-300 to 0,300\n\
                      prefer no monument within 200 of 0,0";

#[test]
fn searches_rank_like_brute_force_on_any_thread_count() {
    let fixture = fixtures::structured_overworld(false);
    let world = World::open(fixture.path()).unwrap();
    let query = world.search_query(RANKED).unwrap();
    let source = SeedSource::Range {
        first: -300,
        last: 300,
    };
    for certain in [false, true] {
        let expected = brute_force(&world, &query, &source, 10..560, 12, certain);
        assert!(expected.len() == 12, "too few hits: {expected:?}");
        for threads in [1, 2, 8] {
            let options = SearchOptions {
                source: source.clone(),
                from: 10,
                seeds: Some(550),
                top: 12,
                threads: Some(threads),
                certain,
                ..SearchOptions::default()
            };
            let outcome = search(&world, &query, &options);
            assert_eq!(
                ranked(&outcome),
                expected,
                "{threads} threads, certain {certain}"
            );
            assert_eq!(outcome.progress.evaluated, 550);
            assert_eq!(outcome.progress.next, Some(560));
            for hit in &outcome.hits {
                assert!(hit.evaluation.reported(certain));
                assert_eq!(hit.evaluation.score(certain), hit.score);
            }
        }
    }
}

#[test]
fn a_range_searched_in_two_parts_ranks_the_same() {
    let fixture = fixtures::structured_overworld(false);
    let world = World::open(fixture.path()).unwrap();
    let query = world.search_query(RANKED).unwrap();
    let source = SeedSource::List((0..400).map(|i| i * 1_000_003 - 7).collect());
    let options = |from, seeds| SearchOptions {
        source: source.clone(),
        from,
        seeds,
        top: 8,
        ..SearchOptions::default()
    };
    let whole = search(&world, &query, &options(0, None));
    assert_eq!(whole.progress.next, None);
    let first = search(&world, &query, &options(0, Some(150)));
    let second = search(&world, &query, &options(first.progress.next.unwrap(), None));
    let mut merged: Vec<(u64, i64, u64)> =
        ranked(&first).into_iter().chain(ranked(&second)).collect();
    merged.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(&b.0)));
    merged.truncate(8);
    assert_eq!(merged, ranked(&whole));
}

#[test]
fn searching_every_seed_uses_the_prefilter() {
    let fixture = fixtures::structured_overworld(false);
    let world = World::open(fixture.path()).unwrap();
    let query = world
        .search_query("require 3 hut within 150 of 40,-30\nprefer 2 tower within 300 of 0,0")
        .unwrap();
    // Two lower-48 values, 1,500 seeds each, from the middle of their groups.
    for lower in [0_u64, 7] {
        let from = (lower << 16) + 20_000;
        let options = SearchOptions {
            from,
            seeds: Some(1500),
            top: 5,
            ..SearchOptions::default()
        };
        let outcome = search(&world, &query, &options);
        let expected = brute_force(
            &world,
            &query,
            &SeedSource::All,
            from..from + 1500,
            5,
            false,
        );
        assert_eq!(ranked(&outcome), expected, "lower 48 bits {lower}");
        let p = &outcome.progress;
        assert_eq!(p.prefiltered + p.checked, 1500);
        // One prefilter answer covers all of them.
        assert!(p.prefiltered == 0 || p.prefiltered == 1500, "{p:?}");
    }
}

#[test]
fn progress_reports_and_can_stop() {
    let fixture = fixtures::structured_overworld(false);
    let world = World::open(fixture.path()).unwrap();
    let query = world.search_query(RANKED).unwrap();
    let options = SearchOptions {
        source: SeedSource::Range {
            first: 0,
            last: 1_000_000,
        },
        top: 3,
        threads: Some(1),
        ..SearchOptions::default()
    };
    let mut rounds = 0;
    let outcome = world
        .search(&query, &options, |p| {
            rounds += 1;
            assert!(p.top.len() <= 3);
            assert_eq!(p.prefiltered + p.checked, p.evaluated);
            if rounds == 2 {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        })
        .unwrap();
    assert_eq!(rounds, 2);
    let next = outcome.progress.next.unwrap();
    // The same count again gives the same hits.
    let again = search(
        &world,
        &query,
        &SearchOptions {
            seeds: Some(next),
            ..options
        },
    );
    assert_eq!(ranked(&again), ranked(&outcome));
}
