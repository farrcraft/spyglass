//! Loot queries against the oracle's container golden files
//! (`docs/specs/oracle.md#container-golden-files`): `containers_in` over each golden chunk must
//! answer every container the game placed there, and every certain answer must be the game's,
//! table and seed included, with the game's contents. Exploration maps point somewhere in
//! spyglass's fills and nowhere in the oracle's, so containers with one aren't compared by
//! contents (their targets are the map cases', `docs/specs/loot.md#maps`).
//!
//! Needs `cargo xtask oracle containers` and a bundle for the same version. Run with
//! `cargo test --release -p spyglass --test loot -- --ignored --nocapture`.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use spyglass::{ContainerSource, Dimension, FeatureOptions, FoundContainer, FoundFeature, World};
use spyglass_oracle::containers::{ContainerFile, GoldenContainer, Holder};

/// Whether an answer is the game's container: what placed it, where, and from which table.
fn same(ours: &FoundContainer, theirs: &GoldenContainer) -> bool {
    let by = match &ours.source {
        ContainerSource::Structure(id) | ContainerSource::Feature(id) => id,
        _ => return false,
    };
    let (Holder::Block(holder) | Holder::Entity(holder)) = &theirs.what;
    *by == theirs.by
        && ours.position == theirs.pos
        && ours.table == theirs.table
        && ours.holder == *holder
}

/// Each slot's item and count, of the game's contents.
fn game_contents(c: &GoldenContainer) -> Option<BTreeMap<usize, (String, i64)>> {
    let items = c.items.as_ref()?;
    Some(
        items
            .iter()
            .map(|s| {
                (
                    s["slot"].as_u64().unwrap() as usize,
                    (
                        s["id"].as_str().unwrap().to_string(),
                        s["count"].as_i64().unwrap_or(1),
                    ),
                )
            })
            .collect(),
    )
}

/// The dungeons that might place in `chunk`, unlikely ones included.
fn unlikely_dungeons(session: &spyglass::Session<'_>, chunk: [i32; 2]) -> Vec<FoundFeature> {
    let query = session
        .world()
        .feature_query("minecraft:monster_room")
        .unwrap();
    let options = FeatureOptions {
        unlikely: true,
        ..Default::default()
    };
    session.features_in(&query, chunk, chunk, options).unwrap()
}

#[test]
#[ignore = "needs the oracle's container golden files and a bundle: cargo xtask oracle containers"]
fn containers_in_are_the_game_s() {
    let version = spyglass_oracle::version();
    let files = spyglass_oracle::containers::files();
    assert!(
        !files.is_empty(),
        "no container files for {version}; run cargo xtask oracle containers"
    );
    let mut worlds = BTreeMap::new();
    let (mut game, mut certain, mut candidates, mut filled) = (0, 0, 0, 0);
    let mut unlikely_dungeon_chests = 0;
    // A certain container with the game's contents, and an item in it, for the nearest search.
    let mut probe: Option<(i64, String, FoundContainer, String)> = None;
    let mut failures = Vec::new();
    let started = std::time::Instant::now();
    // Two seeds of each dimension keep the test short.
    let mut per_dimension: BTreeMap<String, usize> = BTreeMap::new();
    for path in &files {
        let file = ContainerFile::read(path).unwrap();
        let taken = per_dimension.entry(file.dimension.clone()).or_default();
        if file.chunks.is_empty() || *taken >= 2 {
            continue;
        }
        let dimension = match file.dimension.as_str() {
            "minecraft:the_nether" => Dimension::Nether,
            "minecraft:the_end" => Dimension::End,
            _ => Dimension::Overworld,
        };
        let world = worlds.entry(file.dimension.clone()).or_insert_with(|| {
            World::open_dimension(spyglass_oracle::bundle_dir(), dimension)
                .unwrap_or_else(|e| panic!("{e}; run spyglass-extract {version}"))
        });
        *taken += 1;
        println!("seed {} {}", file.seed, file.dimension);
        let session = world.session(file.seed).unwrap();
        // Each start's chunks in one query, which keeps carved chunks from one chunk to the
        // next, then the golden chunks no start's box holds.
        let golden: BTreeSet<[i32; 2]> = file.chunks.iter().map(|c| c.chunk).collect();
        let mut answers: HashMap<[i32; 2], Vec<FoundContainer>> = HashMap::new();
        let mut areas: Vec<([i32; 2], [i32; 2])> = file
            .starts
            .iter()
            .map(|s| {
                let b = s.bounds;
                ([b[0] >> 4, b[2] >> 4], [b[3] >> 4, b[5] >> 4])
            })
            .collect();
        areas.extend(golden.iter().map(|&c| (c, c)));
        for (min, max) in areas {
            let wanted = golden
                .iter()
                .any(|c| (min[0]..=max[0]).contains(&c[0]) && (min[1]..=max[1]).contains(&c[1]));
            let done =
                (min[0]..=max[0]).all(|x| (min[1]..=max[1]).all(|z| answers.contains_key(&[x, z])));
            if !wanted || done {
                continue;
            }
            for z in min[1]..=max[1] {
                for x in min[0]..=max[0] {
                    answers.entry([x, z]).or_default();
                }
            }
            for c in session.containers_in(min, max).unwrap() {
                let list = answers.entry(c.chunk).or_default();
                if !list.contains(&c) {
                    list.push(c);
                }
            }
        }
        for chunk in &file.chunks {
            let ours = &answers[&chunk.chunk];
            let at = format!(
                "seed {} {} chunk {:?}",
                file.seed, file.dimension, chunk.chunk
            );
            // Only the structures and features spyglass places can be answered.
            let placed = |by: &str| {
                world.structures().any(|s| s == by) || by.starts_with("minecraft:monster_room")
            };
            for t in chunk.containers.iter().filter(|t| placed(&t.by)) {
                game += 1;
                // A candidate whose height is in doubt answers at the game's x and z.
                let answered = ours.iter().any(|c| {
                    same(c, t)
                        || (!c.certain
                            && c.seed == Some(t.seed)
                            && [c.position[0], c.position[2]] == [t.pos[0], t.pos[2]])
                });
                // A dungeon that doesn't place on spyglass's blocks (a mineshaft's carving can
                // make room for it) has no chests to answer: an unlikely feature candidate.
                let unlikely = !answered
                    && t.by.starts_with("minecraft:monster_room")
                    && unlikely_dungeons(&session, chunk.chunk).iter().any(|f| {
                        f.feature == t.by
                            && !f.likely
                            && (f.position[0] - t.pos[0]).abs() <= 8
                            && (f.position[2] - t.pos[2]).abs() <= 8
                    });
                if unlikely {
                    unlikely_dungeon_chests += 1;
                } else if !answered {
                    failures.push(format!(
                        "{at}: the game's {} container at {:?} ({}) isn't an answer",
                        t.by, t.pos, t.table
                    ));
                }
            }
            for c in ours {
                if !c.certain {
                    candidates += 1;
                    continue;
                }
                let Some(t) = chunk
                    .containers
                    .iter()
                    .find(|t| same(c, t) && Some(t.seed) == c.seed)
                else {
                    failures.push(format!(
                        "{at}: a certain {:?} container at {:?} ({}, seed {:?}) isn't the game's",
                        c.source, c.position, c.table, c.seed
                    ));
                    continue;
                };
                certain += 1;
                let (Some(theirs), Some(contents)) = (game_contents(t), &c.contents) else {
                    continue;
                };
                let ours: BTreeMap<usize, (String, i64)> = contents
                    .iter()
                    .map(|s| (s.slot, (s.item.id.clone(), i64::from(s.item.count))))
                    .collect();
                let maps = contents.iter().any(|s| s.item.map.is_some());
                if maps || c.contents_doubt.is_some() {
                    continue;
                }
                if ours == theirs {
                    filled += 1;
                    if probe.is_none()
                        && let Some(first) = contents.first()
                    {
                        probe = Some((
                            file.seed,
                            file.dimension.clone(),
                            c.clone(),
                            first.item.id.clone(),
                        ));
                    }
                } else {
                    failures.push(format!(
                        "{at}: {} at {:?}: spyglass {ours:?}, the game {theirs:?}",
                        c.table, c.position
                    ));
                }
            }
        }
    }
    println!(
        "{game} of the game's containers, {unlikely_dungeon_chests} in unlikely dungeons and the \
         rest answered; {certain} certain answers, all the game's, {filled} with the game's \
         contents; {candidates} candidates; {} differences; {:.1} s",
        failures.len(),
        started.elapsed().as_secs_f64()
    );
    assert!(game > 0, "no containers; run cargo xtask oracle containers");
    // From the container itself, the nearest holding its item is at its distance, 0.
    if let Some((seed, dimension, container, item)) = probe {
        let session = worlds[&dimension].session(seed).unwrap();
        let query = session.world().item_query(&item, None).unwrap();
        let found = session
            .nearest_container(&query, container.position, 1)
            .unwrap();
        let last = found.hits.last();
        println!(
            "nearest {item} from {:?}: {} hits, ending at {:?}",
            container.position,
            found.hits.len(),
            last.map(|c| c.position)
        );
        if !found.complete
            || last.is_none_or(|c| {
                [c.position[0], c.position[2]] != [container.position[0], container.position[2]]
                    && c.position != container.position
            })
        {
            failures.push(format!(
                "seed {seed}: the nearest {item} from the container at {:?} holding one isn't at                  its column",
                container.position
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} differences:\n{}",
        failures.len(),
        failures
            .iter()
            .take(30)
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
#[ignore = "needs a bundle: spyglass-extract"]
fn item_queries_resolve_ids_tags_and_enchantments() {
    let world = World::open(spyglass_oracle::bundle_dir()).unwrap();
    assert!(world.item_query("minecraft:golden_apple", None).is_ok());
    assert!(world.item_query("golden_apple", None).is_ok());
    assert!(world.item_query("#minecraft:logs", None).is_ok());
    assert!(
        world
            .item_query("minecraft:enchanted_book", Some(("mending", Some(1))))
            .is_ok()
    );
    for (item, enchantment) in [
        ("minecraft:no_such_item", None),
        ("#minecraft:no_such_tag", None),
        (
            "minecraft:enchanted_book",
            Some(("minecraft:no_such_enchantment", None)),
        ),
    ] {
        assert!(
            matches!(
                world.item_query(item, enchantment),
                Err(spyglass::Error::Unknown { .. })
            ),
            "{item} {enchantment:?}"
        );
    }
}
