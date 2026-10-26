//! Map rendering on the hand-written fixture world (`spyglass_oracle::fixtures`): plains west of
//! x = 0 tied with forest, desert east of it, and two structure sets. No game data (ADR 0008),
//! so these always run. See `docs/specs/render.md#testing`.

use serde_json::json;
use spyglass::{Error, Layers, MapArea, Session, World};
use spyglass_oracle::fixtures::{self, TestBundle, resource};

fn bundle() -> TestBundle {
    let set = |name: &str, spacing: i32, structure: &str| {
        resource(
            "worldgen/structure_set",
            name,
            json!({"placement": {"type": "minecraft:random_spread", "salt": 12345,
                    "spacing": spacing, "separation": 1},
                "structures": [{"structure": structure, "weight": 1}]}),
        )
    };
    let structure = |name: &str, kind: &str| {
        resource(
            "worldgen/structure",
            name,
            json!({"type": kind, "biomes": "minecraft:plains"}),
        )
    };
    fixtures::overworld(
        &[
            set("towers", 3, "minecraft:tower"),
            structure("tower", "minecraft:stronghold"),
            set("huts", 4, "minecraft:hut"),
            structure("hut", "minecraft:swamp_hut"),
        ],
        true,
    )
}

fn layers(world: &World, slime: bool) -> Layers {
    Layers {
        biomes: true,
        ties: true,
        highlight: vec![world.biome_query("minecraft:forest").unwrap()],
        structures: vec![
            world.structure_query("minecraft:tower").unwrap(),
            world.structure_query("minecraft:hut").unwrap(),
        ],
        slime_chunks: slime,
        spawn: true,
    }
}

/// `area` rendered in tiles of `tile` pixels, assembled.
fn tiled(session: &Session, area: &MapArea, layers: &Layers, tile: u32) -> Vec<[u8; 3]> {
    let [w, h] = area.size;
    let mut out = vec![[0; 3]; w as usize * h as usize];
    for tj in (0..h).step_by(tile as usize) {
        for ti in (0..w).step_by(tile as usize) {
            let part = MapArea {
                min: area.block([ti, tj]),
                size: [tile.min(w - ti), tile.min(h - tj)],
                ..*area
            };
            let image = session.render(&part, layers).unwrap();
            for j in 0..part.size[1] {
                for i in 0..part.size[0] {
                    out[((tj + j) * w + ti + i) as usize] = image.pixel([i, j]);
                }
            }
        }
    }
    out
}

#[test]
fn a_view_equals_its_tiles() {
    let fixture = bundle();
    let world = World::open(fixture.path()).unwrap();
    let session = world.session(42).unwrap();
    for scale in [1, 2, 4, 16, 64] {
        let area = MapArea::centred_on([0, 0], [100, 70], scale, 64).unwrap();
        let layers = layers(&world, scale <= spyglass::MAX_SLIME_SCALE);
        let whole = session.render(&area, &layers).unwrap();
        let pixels: Vec<[u8; 3]> = (0..70)
            .flat_map(|j| (0..100).map(move |i| [i, j]))
            .map(|p| whole.pixel(p))
            .collect();
        assert_eq!(pixels, tiled(&session, &area, &layers, 32), "scale {scale}");
    }
}

#[test]
fn pixels_show_the_biome_at_their_corner() {
    let fixture = bundle();
    let world = World::open(fixture.path()).unwrap();
    let session = world.session(7).unwrap();
    for scale in [1, 4, 32] {
        let area = MapArea::centred_on([3, -9], [40, 20], scale, 70).unwrap();
        let map = session.biome_map(&area).unwrap();
        for j in 0..20 {
            for i in 0..40 {
                let [x, z] = area.block([i, j]);
                let at = session.biome_at([x, 70, z]);
                assert_eq!(world.biomes()[map.biome([i, j])], at.biome, "({x}, {z})");
                let candidates: Vec<&str> = map
                    .candidates([i, j])
                    .map(|b| world.biomes()[b].as_str())
                    .collect();
                let expected = if at.tied.is_empty() {
                    vec![at.biome.as_str()]
                } else {
                    at.tied.iter().map(String::as_str).collect()
                };
                assert_eq!(candidates, expected);
            }
        }
    }
}

#[test]
fn features_are_the_queries_answers() {
    let fixture = bundle();
    let world = World::open(fixture.path()).unwrap();
    let session = world.session(3).unwrap();
    let area = MapArea::centred_on([-700, 40], [64, 48], 8, 64).unwrap();
    let layers = layers(&world, true);
    let features = session.map_features(&area, &layers).unwrap();

    // Structures: what structures_in finds over a wide range, filtered to marker reach.
    let reach = i64::from(spyglass::MARKER_REACH) * 8;
    let [x0, z0] = area.min.map(i64::from);
    let (x1, z1) = (x0 + 64 * 8, z0 + 48 * 8);
    let mut expected = Vec::new();
    for query in &layers.structures {
        for start in session
            .structures_in(query, [-100, -100], [100, 100])
            .unwrap()
        {
            let [x, _, z] = start.position.map(i64::from);
            if (x0 - reach..x1 + reach).contains(&x) && (z0 - reach..z1 + reach).contains(&z) {
                expected.push(start);
            }
        }
    }
    expected
        .sort_by(|a, b| (&a.set, a.chunk[1], a.chunk[0]).cmp(&(&b.set, b.chunk[1], b.chunk[0])));
    assert!(!expected.is_empty());
    assert_eq!(features.structures, expected);

    // Slime chunks: every one overlapping the area.
    let chunks = |lo: i64, hi: i64| ((lo >> 4) as i32, ((hi - 1) >> 4) as i32);
    let ((cx0, cx1), (cz0, cz1)) = (chunks(x0, x1), chunks(z0, z1));
    assert_eq!(
        features.slime_chunks,
        session.slime_chunks([cx0, cz0], [cx1, cz1])
    );
    assert!(!features.slime_chunks.is_empty());

    // The spawn's origin is near 0, 0 here, out of reach of this area.
    assert_eq!(features.spawn, None);
    let home = MapArea::centred_on([0, 0], [64, 64], 16, 64).unwrap();
    let spawn = session.map_features(&home, &layers).unwrap().spawn;
    assert_eq!(spawn, Some(session.spawn_origin()));
}

#[test]
fn bad_requests_are_errors() {
    let fixture = bundle();
    let world = World::open(fixture.path()).unwrap();
    let session = world.session(0).unwrap();
    let layers = layers(&world, true);
    let area = MapArea::centred_on([0, 0], [16, 16], 32, 64).unwrap();
    assert!(matches!(
        session.render(&area, &layers),
        Err(Error::InvalidMap(_))
    ));
    let off_grid = MapArea {
        min: [1, 0],
        ..area
    };
    assert!(matches!(
        session.biome_map(&off_grid),
        Err(Error::InvalidMap(_))
    ));

    let other = World::open(fixture.path()).unwrap();
    let foreign = Layers {
        highlight: vec![other.biome_query("minecraft:plains").unwrap()],
        ..Layers::default()
    };
    assert!(matches!(
        session.render(&area, &foreign),
        Err(Error::OtherWorld(_))
    ));
    let foreign = Layers {
        structures: vec![other.structure_query("minecraft:hut").unwrap()],
        ..Layers::default()
    };
    assert!(matches!(
        session.render(&area, &foreign),
        Err(Error::OtherWorld(_))
    ));
}

#[test]
fn rendering_is_deterministic_and_writes_png() {
    let fixture = bundle();
    let world = World::open(fixture.path()).unwrap();
    let area = MapArea::centred_on([0, 0], [50, 30], 4, 64).unwrap();
    let layers = layers(&world, true);
    let png = |seed| {
        let mut bytes = Vec::new();
        world
            .session(seed)
            .unwrap()
            .render(&area, &layers)
            .unwrap()
            .write_png(&mut bytes)
            .unwrap();
        bytes
    };
    assert_eq!(png(5), png(5));
    assert_ne!(png(5), png(6));
}
