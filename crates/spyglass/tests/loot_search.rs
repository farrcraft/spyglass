//! Loot in both searches (`docs/specs/search.md`, ADR 0065) on the real bundle: a loot count's
//! answers are `containers_holding`'s, the seed search's fast path reports what `evaluate`
//! does, a `find` query's loot targets are never counted around themselves, and a search's
//! containers are the game's.
//!
//! Needs a bundle, and for the last test the oracle's container golden files. Run with
//! `cargo test --release -p spyglass --test loot_search -- --ignored --nocapture`.

use std::time::Instant;

use spyglass::{
    Feature, FindOptions, FoundContainer, Outcome, SearchOptions, SeedSource, Session, World,
};
use spyglass_oracle::containers::{ContainerFile, GoldenContainer};

/// The seeds tested, each around a container near the origin.
const SEEDS: [i64; 2] = [0, 12_345];

/// `containers_holding` for `item` over the chunks of `min..=max` (blocks) and their neighbours,
/// kept where `keep` says.
fn by_hand(
    session: &Session<'_>,
    item: &str,
    min: [i32; 2],
    max: [i32; 2],
    keep: impl Fn(&FoundContainer) -> bool,
) -> Vec<FoundContainer> {
    let query = session.world().item_query(item, None).unwrap();
    session
        .containers_holding(&query, min.map(|v| (v >> 4) - 1), max.map(|v| (v >> 4) + 1))
        .unwrap()
        .into_iter()
        .filter(keep)
        .collect()
}

/// Where to centre a seed's queries, and the item to ask for: the first filled container in the
/// 8 x 8 chunks around the origin, and its first item that isn't a map (whose target may not be
/// certain), so the answers have something in them.
fn probe(session: &Session<'_>) -> ([i32; 2], String) {
    session
        .containers_in([-4, -4], [3, 3])
        .unwrap()
        .into_iter()
        .find_map(|c| {
            let item = c
                .contents
                .iter()
                .flatten()
                .map(|s| s.item.id.clone())
                .find(|id| !id.ends_with("map"))?;
            Some(([c.position[0], c.position[2]], item))
        })
        .expect("a filled container near the origin")
}

fn d2(a: [i32; 2], c: &FoundContainer) -> i64 {
    let (dx, dz) = (
        i64::from(c.position[0] - a[0]),
        i64::from(c.position[2] - a[1]),
    );
    dx * dx + dz * dz
}

fn containers(features: &[Feature]) -> Vec<&FoundContainer> {
    features
        .iter()
        .map(|f| match f {
            Feature::Container(c) => c,
            other => panic!("{other:?}"),
        })
        .collect()
}

#[test]
#[ignore = "needs bundles/<version> (SPYGLASS_ORACLE_VERSION, else the newest release)"]
fn loot_constraints_count_containers_holding() {
    let world = World::open(spyglass_oracle::bundle_dir()).unwrap();
    let start = Instant::now();
    for seed in SEEDS {
        let session = world.session(seed).unwrap();
        let (at, item) = probe(&session);
        // Off the container, so distances differ.
        let [x, z] = at.map(|v| v + 5);
        let query = world
            .search_query(&format!(
                "require loot {item} within 64 of {x},{z}\n\
                 prefer 2 loot {item} within 64 of {x},{z}\n\
                 prefer no loot {item} within 64 of {x},{z} weight 2",
            ))
            .unwrap();
        assert_eq!(
            query.to_string().lines().next(),
            Some(format!("require loot {item} within 64 of {x},{z}").as_str())
        );
        let evaluation = session.evaluate(&query, false).unwrap();
        let near = by_hand(&session, &item, [x - 64, z - 64], [x + 64, z + 64], |c| {
            d2([x, z], c) <= 64 * 64
        });
        assert!(!near.is_empty(), "seed {seed}");
        // Every container in the disc, nearest first: by distance rounded down, then in the
        // order they were placed.
        let c = &evaluation.constraints;
        let mut expected: Vec<&FoundContainer> = near.iter().collect();
        expected.sort_by_key(|c| d2([x, z], c).isqrt());
        assert_eq!(containers(&c[1].features), expected, "seed {seed}");
        let certain = near.iter().filter(|c| c.contents_certain()).count();
        let outcome = match (certain, near.len()) {
            (1.., _) => Outcome::Met,
            (0, 1..) => Outcome::PossiblyMet,
            _ => Outcome::Failed,
        };
        assert_eq!(c[0].outcome, outcome, "seed {seed}");
        assert_eq!(c[2].best == 1000, certain == 0, "seed {seed}");
        assert_eq!(c[2].worst == 1000, near.is_empty(), "seed {seed}");
        println!(
            "seed {seed}: {} containers with {item} near {x},{z} ({certain} certain), {:?}",
            near.len(),
            evaluation.verdict
        );
    }
    println!("{:.1} s", start.elapsed().as_secs_f64());
}

#[test]
#[ignore = "needs bundles/<version> (SPYGLASS_ORACLE_VERSION, else the newest release)"]
fn the_fast_path_agrees_with_evaluate_for_loot() {
    let world = World::open(spyglass_oracle::bundle_dir()).unwrap();
    // Each seed searched on its own, around its own container.
    for seed in SEEDS {
        let ([x, z], item) = probe(&world.session(seed).unwrap());
        let queries = [
            format!("require loot {item} within 48 of {x},{z}"),
            format!("require 3 loot {item} within 48 of {x},{z}"),
            format!(
                "require no loot {item} in {},{} to {},{}\nprefer slime within 64 of 0,0",
                x - 32,
                z - 32,
                x + 31,
                z + 31
            ),
            "require loot #minecraft:pickaxes within 80 of spawn\n\
             prefer 2 loot minecraft:saddle within 48 of spawn"
                .to_string(),
            format!(
                "prefer loot minecraft:enchanted_book with minecraft:sharpness 2 within 64 of \
                 {x},{z} weight 3"
            ),
        ];
        for text in &queries {
            let query = world.search_query(text).unwrap();
            let evaluation = world
                .session(seed)
                .unwrap()
                .evaluate(&query, false)
                .unwrap();
            for certain in [false, true] {
                let start = Instant::now();
                let options = SearchOptions {
                    source: SeedSource::List(vec![seed]),
                    top: 1,
                    certain,
                    ..SearchOptions::default()
                };
                let outcome = world
                    .search(&query, &options, |_| std::ops::ControlFlow::Continue(()))
                    .unwrap();
                let expected: Vec<u64> = evaluation
                    .reported(certain)
                    .then(|| evaluation.score(certain))
                    .into_iter()
                    .collect();
                let found: Vec<u64> = outcome.hits.iter().map(|h| h.score).collect();
                assert_eq!(found, expected, "seed {seed}, certain {certain}: {text}");
                println!(
                    "seed {seed}, certain {certain}, {:?}, {:.1} s: {}",
                    evaluation.verdict,
                    start.elapsed().as_secs_f64(),
                    text.lines().next().unwrap()
                );
            }
        }
    }
}

#[test]
#[ignore = "needs bundles/<version> (SPYGLASS_ORACLE_VERSION, else the newest release)"]
fn find_loot_targets_skip_themselves() {
    let world = World::open(spyglass_oracle::bundle_dir()).unwrap();
    for seed in SEEDS {
        let session = world.session(seed).unwrap();
        let ([x, z], item) = probe(&session);
        let query = world
            .search_query(&format!(
                "find loot {item} in {},{} to {},{}\n\
                 require loot {item} within 32 of here",
                x - 64,
                z - 64,
                x + 63,
                z + 63
            ))
            .unwrap();
        let outcome = session
            .find(&query, &FindOptions::default(), |_| {
                std::ops::ControlFlow::Continue(())
            })
            .unwrap();
        // Every container with another elsewhere within 32 blocks; nearby ones outside the box
        // count too.
        let all = by_hand(&session, &item, [x - 96, z - 96], [x + 95, z + 95], |_| {
            true
        });
        assert!(!all.is_empty(), "seed {seed}");
        let mut expected: Vec<[i32; 2]> = all
            .iter()
            .filter(|c| {
                (x - 64..=x + 63).contains(&c.position[0])
                    && (z - 64..=z + 63).contains(&c.position[2])
            })
            .filter(|c| {
                all.iter().any(|other| {
                    other.position != c.position
                        && d2([c.position[0], c.position[2]], other) <= 32 * 32
                })
            })
            .map(|c| [c.position[0], c.position[2]])
            .collect();
        // The box's centre, rounded down.
        let middle = [x - 1, z - 1];
        expected.sort_by_key(|p| {
            let (dx, dz) = (i64::from(p[0] - middle[0]), i64::from(p[1] - middle[1]));
            (dx * dx + dz * dz, p[1], p[0])
        });
        let found: Vec<[i32; 2]> = outcome.matches.iter().map(|m| m.position).collect();
        assert_eq!(found, expected, "seed {seed}");
        println!("seed {seed}: {} of {} containers", found.len(), all.len());
    }
}

/// The items the game put in a container.
fn items(c: &GoldenContainer) -> Vec<String> {
    c.items
        .iter()
        .flatten()
        .filter_map(|s| s["id"].as_str().map(str::to_string))
        .collect()
}

/// For each of a few golden seeds, a search for an item one of the game's containers holds, in a
/// small box around it, reports the seed, and every container it counts as certainly holding the
/// item is one of the game's that does.
#[test]
#[ignore = "needs the oracle's container golden files and a bundle: cargo xtask oracle containers"]
fn loot_search_containers_are_the_game_s() {
    let version = spyglass_oracle::version();
    let world = World::open(spyglass_oracle::bundle_dir())
        .unwrap_or_else(|e| panic!("{e}; run spyglass-extract {version}"));
    let files = spyglass_oracle::containers::files();
    let start = Instant::now();
    let (mut searched, mut confirmed) = (0, 0);
    let mut failures = Vec::new();
    for path in &files {
        let file = ContainerFile::read(path).unwrap();
        if file.dimension != "minecraft:overworld" || searched >= 3 {
            continue;
        }
        let golden: Vec<&GoldenContainer> =
            file.chunks.iter().flat_map(|c| &c.containers).collect();
        // An item the game put in one of its containers; maps point somewhere else in
        // spyglass's fills.
        let Some((container, item)) = golden.iter().find_map(|c| {
            items(c)
                .into_iter()
                .find(|i| !i.ends_with("map"))
                .map(|i| (*c, i))
        }) else {
            continue;
        };
        let [x, _, z] = container.pos;
        let text = format!(
            "require loot {item} in {},{} to {},{}",
            x - 8,
            z - 8,
            x + 8,
            z + 8
        );
        let query = world.search_query(&text).unwrap();
        let options = SearchOptions {
            source: SeedSource::List(vec![file.seed]),
            top: 1,
            ..SearchOptions::default()
        };
        let outcome = world
            .search(&query, &options, |_| std::ops::ControlFlow::Continue(()))
            .unwrap();
        searched += 1;
        let Some(hit) = outcome.hits.first() else {
            failures.push(format!(
                "seed {}: not reported for {text}, though the game's {} at {:?} holds it",
                file.seed, container.by, container.pos
            ));
            continue;
        };
        for c in containers(&hit.evaluation.constraints[0].features) {
            if !c.contents_certain() {
                continue;
            }
            let theirs = golden
                .iter()
                .find(|g| g.pos == c.position && g.table == c.table);
            match theirs {
                Some(g) if items(g).contains(&item) => confirmed += 1,
                Some(_) => failures.push(format!(
                    "seed {}: the game's container at {:?} has no {item}",
                    file.seed, c.position
                )),
                // Outside the golden chunks: the game's answer isn't known.
                None => {}
            }
        }
        println!("seed {}: {text}", file.seed);
    }
    println!(
        "{searched} searches, {confirmed} certain containers confirmed, {:.1} s",
        start.elapsed().as_secs_f64()
    );
    assert!(
        searched > 0,
        "no container files for {version}; run cargo xtask oracle containers"
    );
    assert!(confirmed > 0, "no certain container to compare");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
