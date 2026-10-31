//! Features in both searches (`docs/specs/search.md#features`) on the real bundle: a feature
//! count's and a cluster's answers are `features_in`'s, the seed search's fast path reports what
//! `evaluate` does, and a `find` query's feature targets are never counted around themselves.
//!
//! Needs a bundle. Run with
//! `cargo test --release -p spyglass --test feature_search -- --ignored --nocapture`.

use std::time::Instant;

use spyglass::{
    Feature, FeatureOptions, FindOptions, FoundFeature, Outcome, SearchOptions, SeedSource, World,
};

const SEEDS: [i64; 6] = [0, 1, 2, 3, -4_172_144_997_902_289_642, 123_456_789];

/// `features_in` over the chunks of `min..=max` (blocks), kept where `keep` says.
fn by_hand(
    world: &World,
    seed: i64,
    min: [i32; 2],
    max: [i32; 2],
    keep: impl Fn(&FoundFeature) -> bool,
) -> Vec<FoundFeature> {
    let query = world.feature_query("minecraft:monster_room").unwrap();
    world
        .session(seed)
        .unwrap()
        .features_in(
            &query,
            min.map(|v| v >> 4),
            max.map(|v| v >> 4),
            FeatureOptions::default(),
        )
        .unwrap()
        .into_iter()
        .filter(keep)
        .collect()
}

fn d2(a: [i32; 2], f: &FoundFeature) -> i64 {
    let (dx, dz) = (
        i64::from(f.position[0] - a[0]),
        i64::from(f.position[2] - a[1]),
    );
    dx * dx + dz * dz
}

fn placed(features: &[Feature]) -> Vec<&FoundFeature> {
    features
        .iter()
        .map(|f| match f {
            Feature::Placed(f) => f,
            other => panic!("{other:?}"),
        })
        .collect()
}

#[test]
#[ignore = "needs bundles/<version> (SPYGLASS_ORACLE_VERSION, else the newest release)"]
fn feature_constraints_count_features_in() {
    let world = World::open(spyglass_oracle::bundle_dir()).unwrap();
    let query = world
        .search_query(
            "require feature minecraft:monster_room within 60 of 8,8\n\
             prefer 3 feature minecraft:monster_room within 60 of 8,8\n\
             prefer cluster 2 feature minecraft:monster_room within 24 of each other in -64,-64 to 63,63\n\
             prefer no feature minecraft:monster_room_deep within 60 of 8,8 weight 2",
        )
        .unwrap();
    assert_eq!(
        query.to_string().lines().next(),
        Some("require feature minecraft:monster_room within 60 of 8,8")
    );
    let start = Instant::now();
    for seed in SEEDS {
        let evaluation = world
            .session(seed)
            .unwrap()
            .evaluate(&query, false)
            .unwrap();
        let near = by_hand(&world, seed, [-52, -52], [68, 68], |f| {
            d2([8, 8], f) <= 60 * 60
        });
        // Every feature in the disc, nearest first.
        let c = &evaluation.constraints;
        let mut expected: Vec<&FoundFeature> = near.iter().collect();
        expected.sort_by_key(|f| d2([8, 8], f));
        assert_eq!(placed(&c[1].features), expected, "seed {seed}");
        let certain = near.iter().filter(|f| f.certain).count();
        let outcome = match (certain, near.len()) {
            (1.., _) => Outcome::Met,
            (0, 1..) => Outcome::PossiblyMet,
            _ => Outcome::Failed,
        };
        assert_eq!(c[0].outcome, outcome, "seed {seed}");
        // The cluster's group is two features 24 blocks apart at most, in three dimensions.
        let group = placed(&c[2].features);
        if let [a, b] = group[..] {
            let d = (0..3)
                .map(|i| i64::from(a.position[i] - b.position[i]).pow(2))
                .sum::<i64>();
            assert!(d <= 24 * 24, "seed {seed}");
        }
        assert_eq!(c[2].best, 500 * group.len() as u32, "seed {seed}");
        let deep = near
            .iter()
            .filter(|f| f.feature == "minecraft:monster_room_deep")
            .collect::<Vec<_>>();
        assert_eq!(c[3].best == 1000, deep.iter().all(|f| !f.certain));
        assert_eq!(c[3].worst == 1000, deep.is_empty());
        println!(
            "seed {seed}: {} dungeons near 8,8 ({certain} certain), {:?}",
            near.len(),
            evaluation.verdict
        );
    }
    println!("{:.1} s", start.elapsed().as_secs_f64());
}

#[test]
#[ignore = "needs bundles/<version> (SPYGLASS_ORACLE_VERSION, else the newest release)"]
fn the_fast_path_agrees_with_evaluate_for_features() {
    let world = World::open(spyglass_oracle::bundle_dir()).unwrap();
    let queries = [
        "require 2 feature minecraft:monster_room within 50 of 0,0",
        "require cluster 2 feature minecraft:monster_room within 20 of each other within 70 of 0,0",
        "require no feature minecraft:monster_room in -32,-32 to 31,31\nprefer slime within 64 of 0,0",
        "require feature minecraft:monster_room within 90 of spawn\nprefer 2 feature minecraft:monster_room within 40 of spawn",
    ];
    for text in queries {
        let query = world.search_query(text).unwrap();
        for certain in [false, true] {
            let start = Instant::now();
            let options = SearchOptions {
                source: SeedSource::List(SEEDS.to_vec()),
                top: SEEDS.len(),
                certain,
                ..SearchOptions::default()
            };
            let outcome = world
                .search(&query, &options, |_| std::ops::ControlFlow::Continue(()))
                .unwrap();
            let mut expected: Vec<(u64, i64)> = SEEDS
                .iter()
                .filter_map(|&seed| {
                    let e = world
                        .session(seed)
                        .unwrap()
                        .evaluate(&query, false)
                        .unwrap();
                    e.reported(certain).then(|| (e.score(certain), seed))
                })
                .collect();
            expected.sort_by_key(|&(score, seed)| {
                (
                    std::cmp::Reverse(score),
                    SEEDS.iter().position(|&s| s == seed),
                )
            });
            let found: Vec<(u64, i64)> = outcome.hits.iter().map(|h| (h.score, h.seed)).collect();
            assert_eq!(found, expected, "certain {certain}: {text}");
            println!(
                "{} of {} seeds, certain {certain}, {:.1} s: {}",
                found.len(),
                SEEDS.len(),
                start.elapsed().as_secs_f64(),
                text.lines().next().unwrap()
            );
        }
    }
}

#[test]
#[ignore = "needs bundles/<version> (SPYGLASS_ORACLE_VERSION, else the newest release)"]
fn find_feature_targets_skip_themselves() {
    let world = World::open(spyglass_oracle::bundle_dir()).unwrap();
    let query = world
        .search_query(
            "find feature minecraft:monster_room in -64,-64 to 63,63\n\
             require feature minecraft:monster_room within 24 of here",
        )
        .unwrap();
    for seed in SEEDS {
        let session = world.session(seed).unwrap();
        let outcome = session
            .find(&query, &FindOptions::default(), |_| {
                std::ops::ControlFlow::Continue(())
            })
            .unwrap();
        // Every dungeon with another within 24 blocks; nearby ones outside the box count too.
        let all = by_hand(&world, seed, [-88, -88], [87, 87], |_| true);
        let mut expected: Vec<[i32; 2]> = all
            .iter()
            .filter(|f| (-64..=63).contains(&f.position[0]) && (-64..=63).contains(&f.position[2]))
            .filter(|f| {
                all.iter().any(|g| {
                    g.position != f.position && d2([f.position[0], f.position[2]], g) <= 24 * 24
                })
            })
            .map(|f| [f.position[0], f.position[2]])
            .collect();
        let centre = [-1, -1];
        expected.sort_by_key(|p| {
            let (dx, dz) = (i64::from(p[0] - centre[0]), i64::from(p[1] - centre[1]));
            (dx * dx + dz * dz, p[1], p[0])
        });
        let found: Vec<[i32; 2]> = outcome.matches.iter().map(|m| m.position).collect();
        assert_eq!(found, expected, "seed {seed}");
        println!("seed {seed}: {} of {} dungeons", found.len(), all.len());
    }
}
