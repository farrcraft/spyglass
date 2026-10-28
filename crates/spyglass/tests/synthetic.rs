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
