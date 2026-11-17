//! Map calls made through `Session::stopping` (ADR 0078): the session's answers while the stop
//! isn't raised, `Error::Stopped` once it is, and soon after it's raised mid-call. The fixture
//! tests always run; those on the game's own world need a bundle.

use std::thread;
use std::time::{Duration, Instant};

use spyglass::{Error, Layers, MapArea, Session, Stop, World};
use spyglass_oracle::fixtures;

/// Every stoppable call on `area`, through `stopping`, against the session's own answers when
/// `expect_same`, or expecting `Error::Stopped` otherwise. Calls the world can't answer (no
/// carvers or flowers in the fixture) are skipped.
fn each_call(session: &Session, area: &MapArea, layers: &Layers, stop: &Stop, expect_same: bool) {
    let stopping = session.stopping(stop);
    let check = |name: &str, same: bool, stopped: bool| {
        if expect_same {
            assert!(same, "{name}: not the session's answer");
        } else {
            assert!(stopped, "{name}: not stopped");
        }
    };
    let stopped = |e: &Error| matches!(e, Error::Stopped);

    let biomes = stopping.biome_map(area);
    check(
        "biome_map",
        biomes.as_ref().ok() == session.biome_map(area).as_ref().ok(),
        biomes.as_ref().is_err_and(stopped),
    );
    let heights = session.height_map(area).unwrap();
    let ours = stopping.height_map(area);
    check(
        "height_map",
        ours.as_ref().ok() == Some(&heights),
        ours.as_ref().is_err_and(stopped),
    );
    let surface = stopping.surface_biome_map(&heights);
    check(
        "surface_biome_map",
        surface.as_ref().ok() == session.surface_biome_map(&heights).as_ref().ok(),
        surface.as_ref().is_err_and(stopped),
    );
    let features = stopping.map_features(area, layers);
    check(
        "map_features",
        features.as_ref().ok() == Some(&session.map_features(area, layers).unwrap()),
        features.as_ref().is_err_and(stopped),
    );
    if area.scale <= spyglass::MAX_UNDERGROUND_SCALE
        && let Ok(carvings) = session.carving_map(area)
    {
        let ours = stopping.carving_map(area);
        check(
            "carving_map",
            ours.as_ref().ok() == Some(&carvings),
            ours.as_ref().is_err_and(stopped),
        );
        let underground = Layers {
            carvers: true,
            ..Layers::default()
        };
        let ours = stopping.underground_map(area, &underground, None);
        check(
            "underground_map",
            ours.as_ref().ok()
                == session
                    .underground_map(area, &underground, None)
                    .as_ref()
                    .ok(),
            ours.as_ref().is_err_and(stopped),
        );
    }
    if let Ok(flowers) = session.flower_map(area) {
        let ours = stopping.flower_map(area);
        check(
            "flower_map",
            ours.as_ref().ok() == Some(&flowers),
            ours.as_ref().is_err_and(stopped),
        );
    }
}

#[test]
fn unstopped_calls_give_the_session_s_answers_and_stopped_ones_give_none() {
    let fixture = fixtures::structured_overworld(true);
    let world = World::open(fixture.path()).unwrap();
    let session = world.session(42).unwrap();
    let layers = Layers {
        biomes: false,
        structures: world
            .structures()
            .map(|id| world.structure_query(id).unwrap())
            .collect(),
        confirm: true,
        ..Layers::default()
    };
    for scale in [1, 4, 16] {
        let area = MapArea::centred_on([30, -20], [48, 40], scale, 64).unwrap();
        each_call(&session, &area, &layers, &Stop::new(), true);
        let stop = Stop::new();
        stop.stop();
        each_call(&session, &area, &layers, &stop, false);
    }
}

/// How long `call` takes to give `Error::Stopped` once the stop is raised `after` it starts.
fn stops_within(after: Duration, call: impl FnOnce(&Stop) -> spyglass::Result<()>) -> Duration {
    let stop = Stop::new();
    thread::scope(|scope| {
        let raised = scope.spawn(|| {
            thread::sleep(after);
            stop.stop();
            Instant::now()
        });
        let result = call(&stop);
        let finished = Instant::now();
        let raised = raised.join().unwrap();
        assert!(
            matches!(result, Err(Error::Stopped)),
            "finished before the stop: make the call slower"
        );
        finished.saturating_duration_since(raised)
    })
}

#[test]
#[ignore = "needs bundles/<version> (SPYGLASS_ORACLE_VERSION, else the newest release)"]
fn slow_tiles_stop_within_half_a_second() {
    let dir = spyglass_oracle::bundle_dir();
    let world = World::open(dir).unwrap();
    let session = world.session(-4_893_098_694_207_956_070).unwrap();
    let limit = Duration::from_millis(500);
    // What a world loads on first use, which a stop doesn't interrupt (ADR 0078): the jigsaw
    // template pools that features and containers read structure pieces from.
    let room = world.feature_query("minecraft:monster_room").unwrap();
    session
        .features_in(&room, [40, 40], [43, 43], Default::default())
        .unwrap();
    session.containers_in([40, 40], [40, 40]).unwrap();
    let after = Duration::from_millis(50);

    // Heights at 1:64 take about 20 s a tile, a row of chunks at a time.
    let far = MapArea::centred_on([0, 0], [128, 128], 64, 64).unwrap();
    let took = stops_within(after, |stop| {
        session.stopping(stop).height_map(&far).map(drop)
    });
    eprintln!("heights at 1:64 stopped {took:?} after the stop");
    assert!(took < limit, "heights at 1:64 took {took:?} to stop");

    // Every underground layer at 1:4: carving is most of it.
    let near = MapArea::centred_on([0, 0], [128, 128], 4, 0).unwrap();
    let underground = Layers {
        veins: true,
        noise_caves: true,
        carvers: true,
        ..Layers::default()
    };
    let took = stops_within(after, |stop| {
        session
            .stopping(stop)
            .underground_map(&near, &underground, None)
            .map(drop)
    });
    eprintln!("underground at 1:4 stopped {took:?} after the stop");
    assert!(took < limit, "underground at 1:4 took {took:?} to stop");

    // Features at 1:2 place every chunk, about 30 ms each.
    let close = MapArea::centred_on([0, 0], [128, 128], 2, 64).unwrap();
    let features = Layers {
        biomes: false,
        features: vec![
            world.feature_query("minecraft:monster_room").unwrap(),
            world.feature_query("minecraft:amethyst_geode").unwrap(),
        ],
        ..Layers::default()
    };
    let took = stops_within(after, |stop| {
        session
            .stopping(stop)
            .map_features(&close, &features)
            .map(drop)
    });
    eprintln!("features at 1:2 stopped {took:?} after the stop");
    assert!(took < limit, "features at 1:2 took {took:?} to stop");

    // Every container at 1:2.
    let took = stops_within(after, |stop| {
        session
            .stopping(stop)
            .containers_in([-8, -8], [8, 8])
            .map(drop)
    });
    eprintln!("containers stopped {took:?} after the stop");
    assert!(took < limit, "containers took {took:?} to stop");
}
