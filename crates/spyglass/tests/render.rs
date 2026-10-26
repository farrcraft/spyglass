//! Map rendering on the hand-written fixture world (`spyglass_oracle::fixtures`): plains west of
//! x = 0 tied with forest, desert east of it, two structure sets, and ground that rises to the
//! east and south out of a sea. No game data (ADR 0008), so these always run. See
//! `docs/specs/render.md#testing`.

use serde_json::json;
use spyglass::{Error, Heightmap, Layers, MapArea, Session, StructureStart, World};
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
            sloped_terrain(),
        ],
        true,
    )
}

/// The fixture's noise settings with ground below `y = 50 + x/4 + z/8`, so it comes out of the
/// sea (63) east of about x = 52.
fn sloped_terrain() -> (String, serde_json::Value) {
    let gradient = |axis: &str, from: i32, to: i32, from_value: f64, to_value: f64| {
        json!({"type": "minecraft:gradient", "axis": axis, "from_coordinate": from,
            "from_value": from_value, "to_coordinate": to, "to_value": to_value})
    };
    let constant = |v: f64| json!({"type": "minecraft:constant", "value": v});
    resource(
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
        ..Layers::default()
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
    // Terrain layers too, and with the biome layer off.
    for (scale, biomes) in [(1, true), (2, false), (4, true)] {
        let area = MapArea::centred_on([60, 10], [100, 70], scale, 64).unwrap();
        let layers = Layers {
            biomes,
            confirm: true,
            relief: true,
            surface: true,
            ..layers(&world, true)
        };
        let whole = session.render(&area, &layers).unwrap();
        let pixels: Vec<[u8; 3]> = (0..70)
            .flat_map(|j| (0..100).map(move |i| [i, j]))
            .map(|p| whole.pixel(p))
            .collect();
        assert_eq!(pixels, tiled(&session, &area, &layers, 32), "scale {scale}");
    }
}

#[test]
fn terrain_layers_show_the_heights_at_their_corner() {
    let fixture = bundle();
    let world = World::open(fixture.path()).unwrap();
    let session = world.session(7).unwrap();
    for scale in [1, 2, 4] {
        let area = MapArea::centred_on([60, -9], [40, 20], scale, 64).unwrap();
        let heights = session.height_map(&area).unwrap();
        let surface = session.surface_biome_map(&heights).unwrap();
        let mut seen = std::collections::BTreeSet::new();
        for j in 0..20 {
            for i in 0..40 {
                let [x, z] = area.block([i, j]);
                let h = session.height_at([x, z], Heightmap::WorldSurfaceWg);
                assert_eq!(heights.height([i, j]), h, "({x}, {z})");
                seen.insert(h);
                let at = session.biome_at([x, h, z]);
                assert_eq!(world.biomes()[surface.biome([i, j])], at.biome);
                assert_eq!(session.surface_biome_at([x, z]), at);
            }
        }
        assert!(seen.len() > 3, "the area should slope: {seen:?}");
    }

    // Relief: the flat sea unshaded, slopes facing the north-west lit.
    let area = MapArea::centred_on([60, 0], [60, 4], 1, 64).unwrap();
    let flat = Layers {
        biomes: false,
        ..Layers::default()
    };
    let relief = Layers {
        relief: true,
        ..flat.clone()
    };
    let heights = session.height_map(&area).unwrap();
    let plain = heights.elevation();
    let shaded = session.render(&area, &relief).unwrap();
    assert_eq!(
        session.render(&area, &flat).unwrap().pixel([0, 0]),
        [0x20; 3]
    );
    let sea = [0, 1];
    assert_eq!(heights.height(sea), 63);
    assert_eq!(shaded.pixel(sea), plain.pixel(sea));
    // The ground rises in steps: lit where it's higher than to the west or north, else as is.
    let mut lit = 0;
    for j in 1..4 {
        for i in 1..60 {
            let h = heights.height([i, j]);
            let rises = h > heights.height([i - 1, j]) || h > heights.height([i, j - 1]);
            let (got, base) = (shaded.pixel([i, j]), plain.pixel([i, j]));
            if rises {
                assert!(got[0] > base[0], "({i}, {j})");
                lit += 1;
            } else {
                assert_eq!(got, base, "({i}, {j})");
            }
        }
    }
    assert!(lit > 3);
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
    let confirmed = session
        .map_features(
            &area,
            &Layers {
                confirm: true,
                ..layers.clone()
            },
        )
        .unwrap();
    let reach = i64::from(spyglass::MARKER_REACH) * 8;
    let [x0, z0] = area.min.map(i64::from);
    let (x1, z1) = (x0 + 64 * 8, z0 + 48 * 8);
    let mut expected = Vec::new();
    for query in &layers.structures {
        for start in session
            .structures_in(query, [-100, -100], [100, 100], false)
            .unwrap()
        {
            let [x, _, z] = start.position.map(i64::from);
            if (x0 - reach..x1 + reach).contains(&x) && (z0 - reach..z1 + reach).contains(&z) {
                expected.push(start);
            }
        }
    }
    let by_place = |a: &StructureStart, b: &StructureStart| {
        (&a.set, a.chunk[1], a.chunk[0]).cmp(&(&b.set, b.chunk[1], b.chunk[0]))
    };
    expected.sort_by(by_place);
    assert!(!expected.is_empty());
    assert_eq!(features.structures, expected);

    // Confirmed: the chunks confirmed structures_in finds among them.
    let mut expected = Vec::new();
    for query in &layers.structures {
        for start in session
            .structures_in(query, [-100, -100], [100, 100], true)
            .unwrap()
        {
            let same = |s: &StructureStart| s.chunk == start.chunk && s.set == start.set;
            if features.structures.iter().any(same) {
                expected.push(start);
            }
        }
    }
    expected.sort_by(by_place);
    assert_eq!(confirmed.structures, expected);

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

    let relief = Layers {
        relief: true,
        ..Layers::default()
    };
    let far = MapArea::centred_on([0, 0], [16, 16], 8, 64).unwrap();
    assert!(matches!(
        session.render(&far, &relief),
        Err(Error::InvalidMap(_))
    ));
    assert!(session.height_map(&far).is_err());

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

#[test]
#[ignore = "needs a bundle: spyglass-extract 26.3"]
fn the_real_world_renders_the_same_in_tiles() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../bundles/26.3");
    let world = World::open(dir).unwrap();
    let session = world.session(-4_893_098_694_207_956_070).unwrap();
    for scale in [1, 4, 16, 64] {
        let layers = Layers {
            biomes: true,
            ties: true,
            highlight: vec![world.biome_query("minecraft:cherry_grove").unwrap()],
            structures: vec![
                world.structure_query("#minecraft:village").unwrap(),
                world.structure_query("minecraft:stronghold").unwrap(),
            ],
            slime_chunks: scale <= spyglass::MAX_SLIME_SCALE,
            spawn: true,
            confirm: true,
            relief: scale <= spyglass::MAX_TERRAIN_SCALE,
            surface: scale <= spyglass::MAX_TERRAIN_SCALE,
        };
        let area = MapArea::centred_on([200, -300], [160, 120], scale, 64).unwrap();
        let whole = session.render(&area, &layers).unwrap();
        let pixels: Vec<[u8; 3]> = (0..120)
            .flat_map(|j| (0..160).map(move |i| [i, j]))
            .map(|p| whole.pixel(p))
            .collect();
        assert_eq!(pixels, tiled(&session, &area, &layers, 64), "scale {scale}");
        let map = session.biome_map(&area).unwrap();
        for [i, j] in [[0, 0], [159, 119], [80, 60], [13, 101]] {
            let [x, z] = area.block([i, j]);
            assert_eq!(
                world.biomes()[map.biome([i, j])],
                session.biome_at([x, 64, z]).biome,
                "scale {scale} pixel ({i}, {j})"
            );
        }
        if scale <= spyglass::MAX_TERRAIN_SCALE {
            let heights = session.height_map(&area).unwrap();
            let surface = session.surface_biome_map(&heights).unwrap();
            for [i, j] in [[0, 0], [159, 119], [80, 60], [13, 101]] {
                let [x, z] = area.block([i, j]);
                let h = session.height_at([x, z], Heightmap::WorldSurfaceWg);
                assert_eq!(heights.height([i, j]), h, "scale {scale} pixel ({i}, {j})");
                assert_eq!(
                    world.biomes()[surface.biome([i, j])],
                    session.biome_at([x, h.min(319), z]).biome,
                );
            }
        }
    }
}
