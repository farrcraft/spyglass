//! The public API on a hand-written world (`spyglass_oracle::fixtures`): plains west of x = 0,
//! desert east of it, and structure sets chosen to exercise each kind of locate answer. No game
//! data (ADR 0008), so these always run.

use spyglass::{Error, Heightmap, World};
use spyglass_oracle::fixtures;

#[test]
fn biomes_follow_the_temperature_gradient() {
    let fixture = fixtures::structured_overworld(false);
    let world = World::open(fixture.path()).unwrap();
    let session = world.session(42).unwrap();
    assert_eq!(session.biome_at([-500, 64, 0]).biome, "minecraft:plains");
    assert_eq!(session.biome_at([500, 64, 123]).biome, "minecraft:desert");
    assert!(session.biome_at([500, 64, 123]).tied.is_empty());
}

#[test]
fn heights_follow_the_flat_seabed() {
    let fixture = fixtures::structured_overworld(false);
    let world = World::open(fixture.path()).unwrap();
    let session = world.session(7).unwrap();
    // Stone below y 50, water to the sea level at 63.
    assert_eq!(session.height_at([-30, 900], Heightmap::WorldSurfaceWg), 63);
    assert_eq!(session.height_at([-30, 900], Heightmap::OceanFloorWg), 50);
    let region = session
        .heights_in([-3, 5], [20, 6], Heightmap::OceanFloorWg)
        .unwrap();
    assert_eq!(region, vec![50; 24 * 2]);
    assert!(
        session
            .heights_in([1, 1], [0, 5], Heightmap::OceanFloorWg)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn a_certain_structure_ends_the_search() {
    let fixture = fixtures::structured_overworld(false);
    let world = World::open(fixture.path()).unwrap();
    let towers = world.structure_query("minecraft:tower").unwrap();
    for seed in [0, 1, 42] {
        let session = world.session(seed).unwrap();
        let found = session
            .locate_structure(
                &towers,
                [100, 64, 0],
                &spyglass::LocateOptions {
                    radius: spyglass::LOCATE_RADIUS,
                    confirm: false,
                },
            )
            .unwrap();
        assert!(found.complete);
        let last = found.hits.len() - 1;
        assert_eq!(found.certain_answer, Some(last), "seed {seed}");
        let hit = &found.hits[last];
        assert!(hit.start.certain);
        assert_eq!(hit.start.structures, ["minecraft:tower"]);
        // Towers only pass in plains, west of x = 0.
        assert!(hit.start.position[0] < 0, "seed {seed}: {hit:?}");
        // A certain answer is in structures_in too.
        let [cx, cz] = hit.start.chunk;
        let around = session
            .structures_in(&towers, [cx - 1, cz - 1], [cx + 1, cz + 1], false)
            .unwrap();
        assert!(around.iter().any(|s| s.chunk == [cx, cz] && s.certain));
    }
}

#[test]
fn candidates_stop_at_the_limit() {
    let fixture = fixtures::structured_overworld(false);
    let world = World::open(fixture.path()).unwrap();
    let huts = world.structure_query("minecraft:hut").unwrap();
    let session = world.session(7).unwrap();
    let found = session
        .locate_structure(
            &huts,
            [-5000, 64, 0],
            &spyglass::LocateOptions {
                radius: spyglass::LOCATE_RADIUS,
                confirm: false,
            },
        )
        .unwrap();
    // Huts depend on terrain, so none is certain, and the search gives up.
    assert!(!found.complete);
    assert_eq!(found.certain_answer, None);
    assert_eq!(found.hits.len(), 32);
    assert!(found.hits.iter().all(|h| !h.start.certain));
    // Hits come in the game's order, not nearest first, but each is within the radius.
    assert!(found.hits.iter().all(|h| h.blocks_away < 8 * 16 * 101));
}

#[test]
fn tags_expand_in_order() {
    let fixture = fixtures::structured_overworld(false);
    let world = World::open(fixture.path()).unwrap();
    let all = world.structure_query("#minecraft:everything").unwrap();
    assert_eq!(all.argument(), "#minecraft:everything");
    let session = world.session(3).unwrap();
    let found = session
        .locate_structure(
            &all,
            [-3000, 64, 0],
            &spyglass::LocateOptions {
                radius: spyglass::LOCATE_RADIUS,
                confirm: false,
            },
        )
        .unwrap();
    assert!(found.complete);
    assert!(
        found
            .hits
            .iter()
            .flat_map(|h| &h.start.structures)
            .all(|s| s == "minecraft:tower" || s == "minecraft:hut")
    );
    assert_eq!(
        world.query_structures(&all).unwrap(),
        ["minecraft:tower", "minecraft:hut"]
    );
    let hut = world.structure_query("minecraft:hut").unwrap();
    assert_eq!(world.query_structures(&hut).unwrap(), ["minecraft:hut"]);
}

#[test]
fn rings_find_a_certain_keep() {
    let fixture = fixtures::structured_overworld(false);
    let world = World::open(fixture.path()).unwrap();
    let keeps = world.structure_query("minecraft:keep").unwrap();
    let session = world.session(99).unwrap();
    let found = session
        .locate_structure(
            &keeps,
            [0, 64, 0],
            &spyglass::LocateOptions {
                radius: spyglass::LOCATE_RADIUS,
                confirm: false,
            },
        )
        .unwrap();
    assert!(found.complete);
    assert!(found.certain_answer.is_some());
    // Six positions in the first ring, 4·distance·... chunks out; all in the box structures_in
    // sees around the origin.
    let all = session
        .structures_in(&keeps, [-200, -200], [200, 200], false)
        .unwrap();
    assert!(!all.is_empty() && all.len() <= 6, "{all:?}");
}

#[test]
fn biome_locates_cross_the_gradient() {
    let fixture = fixtures::structured_overworld(false);
    let world = World::open(fixture.path()).unwrap();
    let session = world.session(5).unwrap();
    let desert = world.biome_query("desert").unwrap();
    let found = session
        .locate_biome(&desert, [-200, 64, 0], spyglass::LOCATE_BIOME_RADIUS)
        .unwrap();
    let hit = found.hits.last().unwrap();
    assert!(hit.certain);
    assert!(hit.position[0] >= 0, "{hit:?}");
    assert_eq!(hit.biome.biome, "minecraft:desert");
    // A biome that exists but the dimension can't have is never found.
    let missing = world.biome_query("forest").unwrap();
    let found = session.locate_biome(&missing, [0, 64, 0], 64).unwrap();
    assert!(found.hits.is_empty());
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
    let towers = wa.structure_query("minecraft:tower").unwrap();
    let err = wb
        .session(0)
        .unwrap()
        .locate_structure(
            &towers,
            [0, 64, 0],
            &spyglass::LocateOptions {
                radius: 1,
                confirm: false,
            },
        )
        .unwrap_err();
    assert!(matches!(err, Error::OtherWorld(_)), "{err}");
    assert!(matches!(
        wb.query_structures(&towers),
        Err(Error::OtherWorld(_))
    ));
    assert!(matches!(
        wa.structure_query("minecraft:nothing"),
        Err(Error::Unknown {
            what: "structure",
            ..
        })
    ));
    assert!(matches!(
        wa.biome_query("minecraft:nowhere"),
        Err(Error::Unknown { what: "biome", .. })
    ));
}

#[test]
fn coordinates_at_the_edge_of_the_world_wrap() {
    // Near i32's limits every coordinate sum wraps, as in the game; nothing panics.
    let fixture = fixtures::structured_overworld(false);
    let world = World::open(fixture.path()).unwrap();
    let session = world.session(11).unwrap();
    let far = 134_217_727;
    for name in ["minecraft:monument", "minecraft:tower", "minecraft:hut"] {
        let query = world.structure_query(name).unwrap();
        session
            .structures_in(&query, [far - 16, far - 16], [far, far], false)
            .unwrap();
        session
            .structures_in(&query, [-far - 1, -far - 1], [-far + 15, -far + 15], false)
            .unwrap();
        session
            .locate_structure(
                &query,
                [i32::MAX - 100, 64, i32::MIN + 100],
                &spyglass::LocateOptions {
                    radius: 2,
                    confirm: false,
                },
            )
            .unwrap();
    }
}

#[test]
fn slime_chunks_near_come_nearest_first() {
    let fixture = fixtures::structured_overworld(false);
    let world = World::open(fixture.path()).unwrap();
    let session = world.session(12345).unwrap();
    let near = session.slime_chunks_near([3, -4], 20).unwrap();
    assert!(!near.is_empty());
    let d = |[x, z]: [i32; 2]| (x - 3) * (x - 3) + (z + 4) * (z + 4);
    assert!(near.windows(2).all(|w| d(w[0]) <= d(w[1])));
    let mut square = session.slime_chunks([-17, -24], [23, 16]);
    let mut sorted = near.clone();
    square.sort();
    sorted.sort();
    assert_eq!(sorted, square);
}

#[test]
fn spawn_and_bundles() {
    let fixture = fixtures::structured_overworld(false);
    let world = World::open(fixture.path()).unwrap();
    let origin = world.session(1).unwrap().spawn_origin();
    assert!(origin.position.is_some());
    // find_bundle by version, from the folder holding the bundle.
    let found = spyglass::find_bundle(fixture.bundles(), Some("test")).unwrap();
    assert_eq!(found.dir, fixture.path());
    assert_eq!(
        spyglass::find_bundle(fixture.bundles(), None).unwrap().dir,
        fixture.path()
    );
    assert!(matches!(
        spyglass::find_bundle(fixture.bundles(), Some("0.0")),
        Err(Error::NoBundle { .. })
    ));
}

/// With no version asked for, the newest release, never a newer snapshot (ADR 0028); bundles
/// without a recorded kind go by their id.
#[test]
fn default_bundle_is_the_newest_release() {
    let dir = tempfile::tempdir().unwrap();
    let write = |name: &str, data_version: u32, kind: Option<&str>| {
        let bundle = dir.path().join(name);
        std::fs::create_dir_all(&bundle).unwrap();
        let mut manifest = serde_json::json!({
            "format": 1, "version": name, "data_version": data_version, "jar_sha1": "",
            "java_version": "", "extractor": "", "files": {},
        });
        if let Some(kind) = kind {
            manifest["kind"] = kind.into();
        }
        std::fs::write(bundle.join("bundle.json"), manifest.to_string()).unwrap();
    };
    write("26.4-snapshot-5", 5125, None);
    write("26.4-snapshot-6", 5130, Some("snapshot"));
    let only_snapshots = spyglass::find_bundle(dir.path(), None);
    assert!(
        matches!(&only_snapshots, Err(Error::NoRelease { snapshots, .. })
            if snapshots == &["26.4-snapshot-6", "26.4-snapshot-5"]),
        "{only_snapshots:?}"
    );
    write("26.2", 4903, Some("release"));
    write("26.3", 5023, None);
    let found = spyglass::find_bundle(dir.path(), None).unwrap();
    assert_eq!(
        (found.version.as_str(), found.kind),
        ("26.3", spyglass::Kind::Release)
    );
    let named = spyglass::find_bundle(dir.path(), Some("26.4-snapshot-5")).unwrap();
    assert_eq!(named.kind, spyglass::Kind::Snapshot);
}

#[test]
fn requests_past_the_limits_are_errors() {
    use spyglass::{
        LocateOptions, MAX_HEIGHT_COLUMNS, MAX_LOCATE_BIOME_RADIUS, MAX_LOCATE_RADIUS,
        MAX_SLIME_RADIUS,
    };
    let fixture = fixtures::structured_overworld(false);
    let world = World::open(fixture.path()).unwrap();
    let session = world.session(7).unwrap();
    fn too_large<T>(r: Result<T, Error>) -> bool {
        matches!(r, Err(Error::TooLarge { .. }))
    }
    assert!(too_large(
        session.slime_chunks_near([0, 0], MAX_SLIME_RADIUS + 1)
    ));
    // The whole i32 plane: once overflow made it look empty.
    let err = session
        .heights_in([i32::MIN; 2], [i32::MAX; 2], Heightmap::OceanFloorWg)
        .unwrap_err();
    assert!(
        matches!(err, Error::TooLarge { max, .. } if max == MAX_HEIGHT_COLUMNS),
        "{err}"
    );
    let towers = world.structure_query("minecraft:tower").unwrap();
    let options = LocateOptions {
        radius: MAX_LOCATE_RADIUS + 1,
        confirm: false,
    };
    assert!(too_large(session.locate_structure(
        &towers,
        [0, 64, 0],
        &options
    )));
    let biome = world.biome_query("minecraft:plains").unwrap();
    assert!(too_large(session.locate_biome(
        &biome,
        [0, 64, 0],
        MAX_LOCATE_BIOME_RADIUS + 1
    )));
}

/// The horizontal distance squared from `from` to a start's locate position.
fn horizontal(from: [i32; 3], position: [i32; 3]) -> i128 {
    let d = |i: usize| i128::from(position[i]) - i128::from(from[i]);
    d(0) * d(0) + d(2) * d(2)
}

/// Checks `nearest_structure` against every start `structures_in` finds around `from`: the
/// answer is a nearest certain start (the lowest chunk z, then x, among equals of one set),
/// and the hits before it are exactly the candidates nearer than it, nearest first.
fn check_nearest(
    session: &spyglass::Session<'_>,
    query: &spyglass::StructureQuery,
    from: [i32; 3],
    confirm: bool,
) -> spyglass::StructureLocate {
    let options = spyglass::LocateOptions {
        radius: spyglass::LOCATE_RADIUS,
        confirm,
    };
    let found = session.nearest_structure(query, from, &options).unwrap();
    let context = format!("{} from {from:?}, confirm {confirm}", query.argument());
    let distances: Vec<i128> = found
        .hits
        .iter()
        .map(|h| horizontal(from, h.start.position))
        .collect();
    assert!(distances.is_sorted(), "{context}: {distances:?}");
    let Some(answer) = found.certain_answer else {
        return found;
    };
    assert!(found.complete, "{context}");
    assert_eq!(answer, found.hits.len() - 1, "{context}");
    let best = &found.hits[answer].start;
    assert!(best.certain, "{context}");
    assert!(found.hits[..answer].iter().all(|h| !h.start.certain));
    // Every start within the answer's distance, from the chunks around it.
    let reach = i32::try_from(distances[answer].isqrt() / 16 + 2).unwrap();
    let (cx, cz) = (from[0] >> 4, from[2] >> 4);
    let around = session
        .structures_in(
            query,
            [cx.saturating_sub(reach), cz.saturating_sub(reach)],
            [cx.saturating_add(reach), cz.saturating_add(reach)],
            confirm,
        )
        .unwrap();
    for start in &around {
        let d = horizontal(from, start.position);
        let listed = found.hits.iter().any(|h| h.start.chunk == start.chunk);
        // A ring position that's tie- or platform-dependent is listed as a candidate, though
        // structures_in counts its chunk as certain.
        if start.certain && !listed {
            let key = |s: &spyglass::StructureStart| (s.chunk[1], s.chunk[0]);
            assert!(
                d > distances[answer]
                    || (d == distances[answer]
                        && (start.set != best.set || key(start) >= key(best))),
                "{context}: {start:?} is nearer than {best:?}"
            );
        } else if !start.certain && d < distances[answer] {
            assert!(listed, "{context}: candidate {start:?} is missing");
        }
    }
    found
}

#[test]
fn nearest_is_the_nearest_certain_start() {
    let fixture = fixtures::structured_overworld(false);
    let world = World::open(fixture.path()).unwrap();
    let towers = world.structure_query("minecraft:tower").unwrap();
    let everything = world.structure_query("#minecraft:everything").unwrap();
    let origins = (-3..3).flat_map(|i| (-2..2).map(move |j| [i * 1033 + 31, 64, j * 1399 - 13]));
    let edge = [
        [-29_999_000, 64, 29_999_000],
        [-29_999_999, 64, -29_999_999],
    ];
    for seed in [7, 12345] {
        let session = world.session(seed).unwrap();
        for from in origins.clone().chain(edge) {
            for confirm in [false, true] {
                let found = check_nearest(&session, &towers, from, confirm);
                assert!(found.certain_answer.is_some(), "seed {seed} from {from:?}");
                check_nearest(&session, &everything, from, confirm);
            }
        }
    }
}

#[test]
fn nearest_can_differ_from_locate() {
    let fixture = fixtures::structured_overworld(false);
    let world = World::open(fixture.path()).unwrap();
    let towers = world.structure_query("minecraft:tower").unwrap();
    let options = spyglass::LocateOptions {
        radius: spyglass::LOCATE_RADIUS,
        confirm: false,
    };
    let mut differ = 0;
    for seed in 0..20 {
        let session = world.session(seed).unwrap();
        for x in (-4000..0).step_by(250) {
            let from = [x, 64, 0];
            let game = session.locate_structure(&towers, from, &options).unwrap();
            let near = session.nearest_structure(&towers, from, &options).unwrap();
            let at =
                |l: &spyglass::StructureLocate| l.certain_answer.map(|i| l.hits[i].start.position);
            let (game, near) = (at(&game).unwrap(), at(&near).unwrap());
            // Never farther than /locate's answer. Where they differ at the same distance, the
            // tie order chose another start than /locate's scan order.
            assert!(horizontal(from, near) <= horizontal(from, game));
            if horizontal(from, near) < horizontal(from, game) {
                differ += 1;
            }
        }
    }
    // The ring rule makes /locate's answer farther than the nearest now and then.
    assert!(differ > 0);
}

#[test]
fn nearest_candidates_stop_at_the_limit() {
    let fixture = fixtures::structured_overworld(false);
    let world = World::open(fixture.path()).unwrap();
    let huts = world.structure_query("minecraft:hut").unwrap();
    let session = world.session(7).unwrap();
    let found = check_nearest(&session, &huts, [-5000, 64, 0], false);
    // Huts depend on terrain, so none is certain, and the search gives up.
    assert!(!found.complete);
    assert_eq!(found.certain_answer, None);
    assert_eq!(found.hits.len(), 32);
}

#[test]
fn nearest_keep_is_on_the_rings() {
    let fixture = fixtures::structured_overworld(false);
    let world = World::open(fixture.path()).unwrap();
    let keeps = world.structure_query("minecraft:keep").unwrap();
    for seed in [0, 99] {
        let session = world.session(seed).unwrap();
        for from in [[0, 64, 0], [-900, 64, 400], [2000, 64, -2000]] {
            let found = check_nearest(&session, &keeps, from, false);
            assert!(found.certain_answer.is_some(), "seed {seed} from {from:?}");
        }
    }
}

#[test]
fn nearest_checks_its_query_and_radius() {
    let (a, b) = (
        fixtures::structured_overworld(false),
        fixtures::structured_overworld(false),
    );
    let (wa, wb) = (
        World::open(a.path()).unwrap(),
        World::open(b.path()).unwrap(),
    );
    let towers = wa.structure_query("minecraft:tower").unwrap();
    let session = wb.session(0).unwrap();
    let err = session
        .nearest_structure(&towers, [0, 64, 0], &spyglass::LocateOptions::default())
        .unwrap_err();
    assert!(matches!(err, Error::OtherWorld(_)), "{err}");
    let towers = wb.structure_query("minecraft:tower").unwrap();
    let err = session
        .nearest_structure(
            &towers,
            [0, 64, 0],
            &spyglass::LocateOptions {
                radius: spyglass::MAX_LOCATE_RADIUS + 1,
                confirm: false,
            },
        )
        .unwrap_err();
    assert!(matches!(err, Error::TooLarge { .. }), "{err}");
}
