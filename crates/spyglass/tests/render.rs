//! Map rendering on the hand-written fixture world (`spyglass_oracle::fixtures`): plains west of
//! x = 0 tied with forest, desert east of it, two structure sets, and ground that rises to the
//! east and south out of a sea. No game data (ADR 0008), so these always run. See
//! `docs/specs/render.md#testing`.

use serde_json::json;
use spyglass::{
    CarverKind, Carvers, Error, Heightmap, Layers, MapArea, Pin, Session, StructureStart, VeinPart,
    World,
};
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
            json!({"type": kind, "biomes": "minecraft:plains", "step": "surface_structures"}),
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

/// Noise settings with no blocks at all west of about x = −40, solid ground to the top of the
/// noise east of about x = 40, and air for fluid: the End's islands and void, in an Overworld. A
/// sea level below the level keeps the fluid picker's lava (below `min(−54, sea level)`) out of
/// it too, so the empty columns are empty, as the End's are.
fn void_terrain() -> (String, serde_json::Value) {
    let gradient = |axis: &str, from: i32, to: i32, from_value: f64, to_value: f64| {
        json!({"type": "minecraft:gradient", "axis": axis, "from_coordinate": from,
            "from_value": from_value, "to_coordinate": to, "to_value": to_value})
    };
    let constant = |v: f64| json!({"type": "minecraft:constant", "value": v});
    resource(
        "worldgen/noise_settings",
        "overworld",
        json!({
            "sea_level": -100,
            "noise": {"min_y": -64, "height": 384},
            "default_block": "minecraft:stone",
            "default_fluid": "minecraft:air",
            "noise_router": {
                "final_density": {"type": "minecraft:add",
                    "left": gradient("x", -40, 40, -1.0, 1.0),
                    "right": gradient("y", 0, 100, 0.4, -0.4)},
                "temperature": constant(0.0),
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

/// The fixture with something underground: ground below y 50, a sloping noise cave about
/// 4 blocks thick from y 5 in the west to y −25 in the east, a copper vein rule wherever x is
/// positive, and cave and canyon carvers in every biome. The values are made up, not the game's.
fn underground_bundle() -> TestBundle {
    let gradient = |axis: &str, from: i32, to: i32, from_value: f64, to_value: f64| {
        json!({"type": "minecraft:gradient", "axis": axis, "from_coordinate": from,
            "from_value": from_value, "to_coordinate": to, "to_value": to_value})
    };
    let constant = |v: f64| json!({"type": "minecraft:constant", "value": v});
    let block = |name: &str| json!({"type": "minecraft:block", "result_state": {"Name": name}});
    // Negative where |(y + 40)/30 − 1 + x/400| < 0.15.
    let cave = json!({"type": "minecraft:add",
        "left": {"type": "minecraft:abs", "input": {"type": "minecraft:add",
            "left": gradient("y", -40, 20, -1.0, 1.0),
            "right": gradient("x", -200, 200, -0.5, 0.5)}},
        "right": -0.15});
    let settings = resource(
        "worldgen/noise_settings",
        "overworld",
        json!({
            "sea_level": 63,
            "noise": {"min_y": -64, "height": 384},
            "default_block": "minecraft:stone",
            "default_fluid": "minecraft:water",
            "material_rule": {"type": "minecraft:sequence", "sequence": [
                {"type": "minecraft:ore_vein",
                    "ore_block": {"Name": "minecraft:copper_ore"},
                    "raw_ore_block": {"Name": "minecraft:raw_copper_block"},
                    "filler_block": {"Name": "minecraft:granite"},
                    "raw_ore_chance": 0.25,
                    "density": gradient("x", 0, 100, 0.0, 1.0),
                    "richness": 0.6,
                    "filler_gap": gradient("z", -50, 50, -1.0, 1.0)},
                {"type": "minecraft:condition",
                    "if_true": {"type": "minecraft:y_above", "anchor": {"absolute": 0},
                        "surface_depth_multiplier": 0, "add_stone_depth": false},
                    "then_run": block("minecraft:stone")},
                block("minecraft:deepslate")
            ]},
            "noise_router": {
                "chunk_surface_level": 50.0,
                "final_density": {"type": "minecraft:min",
                    "left": gradient("y", 0, 100, 1.0, -1.0), "right": cave},
                "temperature": gradient("x", -1000, 1001, -1.0, 1.0),
                "vegetation": constant(0.0),
                "continents": constant(0.0),
                "erosion": "minecraft:zero",
                "depth": 0.0,
                "ridges": 0.0
            },
            "spawn_target": [{"minecraft:zero": [-1.0, 1.0]}]
        }),
    );
    let uniform = |min: f64, max: f64| json!({"type": "minecraft:uniform", "min_inclusive": min, "max_exclusive": max});
    let heights = json!({"type": "minecraft:uniform", "min_inclusive": {"absolute": -40},
        "max_inclusive": {"absolute": 30}});
    let carvers = json!([
        {"type": "minecraft:cave", "probability": 0.4, "y": heights,
            "count": {"type": "minecraft:uniform", "min_inclusive": 1, "max_inclusive": 4},
            "floor_level": uniform(-1.0, -0.5),
            "horizontal_radius_multiplier": uniform(0.8, 1.2),
            "vertical_radius_multiplier": uniform(0.8, 1.2),
            "room_vertical_radius_multiplier": uniform(0.2, 0.8),
            "thickness": {"type": "minecraft:trapezoid", "min": 0.0, "max": 2.0, "plateau": 1.0}},
        {"type": "minecraft:canyon", "probability": 0.1, "y": heights,
            "vertical_rotation": uniform(-0.1, 0.1),
            "shape": {"distance_factor": uniform(0.8, 1.0),
                "horizontal_radius_factor": uniform(0.8, 1.0),
                "thickness": {"type": "minecraft:trapezoid", "min": 0.0, "max": 5.0,
                    "plateau": 2.0},
                "vertical_radius_center_factor": 0.0, "vertical_radius_default_factor": 1.0,
                "width_smoothness": 3, "y_scale": 3.0}}
    ]);
    let mut files = vec![settings];
    for biome in ["plains", "desert", "forest"] {
        files.push(resource(
            "worldgen/biome",
            biome,
            json!({"temperature": 0.8, "carvers": carvers}),
        ));
    }
    fixtures::overworld(&files, true)
}

/// Every underground layer on, over biomes or not.
fn underground(biomes: bool) -> Layers {
    Layers {
        biomes,
        veins: true,
        noise_caves: true,
        carvers: true,
        ..Layers::default()
    }
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
        // Near the spawn, so tiles cut through a pin and the spawn marker.
        pins: vec![
            Pin {
                position: [3, -5],
                colour: [0xf0, 0xb0, 0x30],
            },
            Pin {
                position: [-700, 40],
                colour: [0x4c, 0x8f, 0xf0],
            },
        ],
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
fn a_view_equals_its_tiles_underground() {
    let fixture = underground_bundle();
    let world = World::open(fixture.path()).unwrap();
    let session = world.session(42).unwrap();
    for (scale, y, biomes) in [(1, -10, true), (2, 0, false), (4, -20, true), (1, 3, false)] {
        let area = MapArea::centred_on([30, 10], [100, 70], scale, y).unwrap();
        let layers = Layers {
            relief: scale == 4,
            ..underground(biomes)
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
fn underground_pixels_are_their_corner_blocks() {
    let fixture = underground_bundle();
    let world = World::open(fixture.path()).unwrap();
    let session = world.session(7).unwrap();
    let mut seen = (0, 0, 0, 0);
    // Each chunk once, before and after carvers: a chunk takes a while.
    let mut chunks = std::collections::HashMap::new();
    for (scale, y, side) in [(1, -12, 40), (2, 2, 24), (4, -20, 16)] {
        let area = MapArea::centred_on([20, 0], [side, side], scale, y).unwrap();
        let carvings = session.carving_map(&area).unwrap();
        let map = session
            .underground_map(&area, &underground(true), Some(&carvings))
            .unwrap();
        assert_eq!(
            map,
            session
                .underground_map(&area, &underground(true), None)
                .unwrap()
        );
        for j in 0..side {
            for i in 0..side {
                let [x, z] = area.block([i, j]);
                let (off, on) = chunks.entry([x >> 4, z >> 4]).or_insert_with(|| {
                    let chunk = [x >> 4, z >> 4];
                    (
                        session.chunk_blocks(chunk, Carvers::Off).unwrap(),
                        session.chunk_blocks(chunk, Carvers::On).unwrap(),
                    )
                });
                let before = off.block([x, y, z]).to_string();
                let after = on.block([x, y, z]);
                let fill = |b: &str| matches!(b, "minecraft:air" | "minecraft:water");
                let covered =
                    (y + 1..off.min_y() + off.height()).any(|a| !fill(off.block([x, a, z])));
                let open = fill(&before);
                assert_eq!(
                    map.is_noise_cave([i, j]),
                    open && covered,
                    "({x}, {y}, {z})"
                );
                let vein = map.vein([i, j]).map(|v| (v.ore, v.part));
                let expected = match before.as_str() {
                    "minecraft:copper_ore" => Some(VeinPart::Ore),
                    "minecraft:raw_copper_block" => Some(VeinPart::RawOre),
                    "minecraft:granite" => Some(VeinPart::Filler),
                    _ => None,
                };
                assert_eq!(
                    vein,
                    expected.map(|p| ("minecraft:copper_ore", p)),
                    "({x}, {y}, {z}): {before}"
                );
                if before != after {
                    assert!(map.carver([i, j]).is_some(), "({x}, {y}, {z}) carved");
                }
                assert_eq!(map.carver([i, j]), carvings.carver([i, j], y));
                seen.0 += usize::from(vein.is_some());
                seen.1 += usize::from(map.is_noise_cave([i, j]));
                seen.2 += usize::from(map.carver([i, j]) == Some(CarverKind::Cave));
                seen.3 += usize::from(map.carver([i, j]) == Some(CarverKind::Canyon));
            }
        }
    }
    let (veins, caves, carved, canyons) = seen;
    assert!(
        veins > 0 && caves > 0 && carved > 0 && canyons > 0,
        "veins {veins}, noise caves {caves}, carver caves {carved}, canyons {canyons}"
    );
}

#[test]
fn underground_layers_tint_the_map() {
    let fixture = underground_bundle();
    let world = World::open(fixture.path()).unwrap();
    let session = world.session(7).unwrap();
    let area = MapArea::centred_on([20, 0], [64, 48], 1, -12).unwrap();
    let map = session
        .underground_map(&area, &underground(false), None)
        .unwrap();
    let base = spyglass::Image::underground_background(area.size);
    let image = session.render(&area, &underground(false)).unwrap();
    let blend = |s: [u8; 3], d: [u8; 3], a: u32| {
        [0, 1, 2].map(|c| ((u32::from(s[c]) * a + u32::from(d[c]) * (255 - a) + 127) / 255) as u8)
    };
    for j in 0..48 {
        for i in 0..64 {
            let mut colour = base.pixel([i, j]);
            if let Some(vein) = map.vein([i, j]) {
                let alpha = if vein.part == VeinPart::Filler {
                    128
                } else {
                    255
                };
                colour = blend([0xe8, 0x74, 0x3c], colour, alpha);
            }
            if map.is_noise_cave([i, j]) {
                colour = blend([0, 0, 0], colour, 160);
            }
            match map.carver([i, j]) {
                Some(CarverKind::Cave) => colour = blend([0xff, 0xd8, 0x30], colour, 200),
                Some(CarverKind::Canyon) => colour = blend([0xff, 0x40, 0xc8], colour, 200),
                _ => {}
            }
            assert_eq!(image.pixel([i, j]), colour, "pixel ({i}, {j})");
        }
    }
    // Each layer alone draws only its own pixels.
    let caves = Layers {
        noise_caves: true,
        ..Layers::default()
    };
    let alone = session.underground_map(&area, &caves, None).unwrap();
    for j in 0..48 {
        for i in 0..64 {
            assert_eq!(alone.vein([i, j]), None);
            assert_eq!(alone.carver([i, j]), None);
            assert_eq!(alone.is_noise_cave([i, j]), map.is_noise_cave([i, j]));
        }
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
fn columns_with_no_blocks_are_the_void() {
    const VOID: [u8; 3] = [0x0b, 0x0b, 0x12];
    // The fixture's level starts at y −64, so an empty column's height is −64.
    const BOTTOM: i32 = -64;
    let fixture = fixtures::overworld(&[void_terrain()], false);
    let world = World::open(fixture.path()).unwrap();
    let session = world.session(3).unwrap();
    for scale in [1, 4] {
        let area = MapArea::centred_on([0, 0], [160 / scale, 4], scale, 64).unwrap();
        let heights = session.height_map(&area).unwrap();
        let with = [
            Layers {
                biomes: true,
                relief: true,
                ..Layers::default()
            },
            Layers {
                biomes: true,
                surface: true,
                ..Layers::default()
            },
            Layers {
                biomes: false,
                relief: true,
                ..Layers::default()
            },
        ];
        for layers in &with {
            let image = session.render(&area, layers).unwrap();
            let (mut void, mut ground) = (0, 0);
            for j in 0..area.size[1] {
                for i in 0..area.size[0] {
                    if heights.height([i, j]) <= BOTTOM {
                        assert_eq!(image.pixel([i, j]), VOID, "({i}, {j}) at 1:{scale}");
                        void += 1;
                    } else {
                        assert_ne!(image.pixel([i, j]), VOID, "({i}, {j}) at 1:{scale}");
                        ground += 1;
                    }
                }
            }
            assert!(void > 0 && ground > 0, "both kinds of column at 1:{scale}");
        }
        // Without heights, nothing knows where the void is: the biome shows everywhere.
        let biomes = session
            .render(
                &area,
                &Layers {
                    biomes: true,
                    ..Layers::default()
                },
            )
            .unwrap();
        for j in 0..area.size[1] {
            for i in 0..area.size[0] {
                assert_ne!(biomes.pixel([i, j]), VOID);
            }
        }
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
fn pins_are_discs_drawn_last_in_order() {
    let fixture = bundle();
    let world = World::open(fixture.path()).unwrap();
    let session = world.session(3).unwrap();
    let area = MapArea::centred_on([0, 0], [40, 40], 2, 64).unwrap();
    let red = [0xe0, 0x50, 0x50];
    let blue = [0x4c, 0x8f, 0xf0];
    let pin = |position, colour| Pin { position, colour };
    let plain = session.render(&area, &Layers::default()).unwrap();
    let layers = Layers {
        // The second overlaps the first and is drawn over it; the third is out of reach.
        pins: vec![pin([0, 0], red), pin([10, 0], blue), pin([200, 0], red)],
        ..Layers::default()
    };
    let image = session.render(&area, &layers).unwrap();
    let centre = area.pixel([0, 0]).map(|p| p as u32);
    let at = |dx: i64, dy: i64| {
        image.pixel([
            (i64::from(centre[0]) + dx) as u32,
            (i64::from(centre[1]) + dy) as u32,
        ])
    };
    // The first pin's centre, its black ring, and past it the map.
    assert_eq!(at(0, 0), red);
    assert_eq!(at(-3, 0), red);
    assert_eq!(at(-4, 0), [0, 0, 0]);
    assert_eq!(at(-4, 2), [0, 0, 0]);
    assert_eq!(at(-5, 0), plain.pixel([centre[0] - 5, centre[1]]));
    // The second pin, 5 pixels to the east, covers the first where they overlap.
    assert_eq!(at(5, 0), blue);
    assert_eq!(at(2, 0), blue);
    assert_eq!(at(1, 0), [0, 0, 0]);
    let features = session.map_features(&area, &layers).unwrap();
    assert_eq!(features.pins, layers.pins[..2]);
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

    // A dimension without ore veins has no vein layer; the underground layers stop at their
    // own scale; a carving map is for its own area.
    let near = MapArea::centred_on([0, 0], [16, 16], 1, 0).unwrap();
    let plain = fixtures::overworld(&[], false);
    let veins = Layers {
        veins: true,
        ..Layers::default()
    };
    let plain_world = World::open(plain.path()).unwrap();
    let result = plain_world.session(0).unwrap().render(&near, &veins);
    assert!(matches!(result, Err(Error::InvalidMap(_))), "{result:?}");
    let underground = underground_bundle();
    let underground_world = World::open(underground.path()).unwrap();
    let session = underground_world.session(0).unwrap();
    let deep =
        MapArea::centred_on([0, 0], [16, 16], spyglass::MAX_UNDERGROUND_SCALE * 2, 0).unwrap();
    for layer in [
        Layers {
            veins: true,
            ..Layers::default()
        },
        Layers {
            carvers: true,
            ..Layers::default()
        },
    ] {
        let result = session.render(&deep, &layer);
        assert!(matches!(result, Err(Error::InvalidMap(_))), "{result:?}");
    }
    let carvings = session.carving_map(&near).unwrap();
    let carvers = Layers {
        carvers: true,
        ..Layers::default()
    };
    let lower = MapArea { y: -30, ..near };
    assert!(
        session
            .underground_map(&lower, &carvers, Some(&carvings))
            .is_ok()
    );
    let shifted = MapArea {
        min: [16, 0],
        ..near
    };
    assert!(matches!(
        session.underground_map(&shifted, &carvers, Some(&carvings)),
        Err(Error::InvalidMap(_))
    ));

    let session = world.session(0).unwrap();
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
#[ignore = "needs bundles/<version> (SPYGLASS_ORACLE_VERSION, else the newest release)"]
fn the_real_world_renders_the_same_in_tiles() {
    let dir = spyglass_oracle::bundle_dir();
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
            features: if scale <= spyglass::MAX_FEATURE_SCALE {
                vec![world.feature_query("minecraft:monster_room").unwrap()]
            } else {
                Vec::new()
            },
            loot: None,
            slime_chunks: scale <= spyglass::MAX_SLIME_SCALE,
            spawn: true,
            confirm: true,
            relief: scale <= spyglass::MAX_TERRAIN_SCALE,
            surface: scale <= spyglass::MAX_TERRAIN_SCALE,
            veins: scale <= spyglass::MAX_UNDERGROUND_SCALE,
            noise_caves: scale <= spyglass::MAX_UNDERGROUND_SCALE,
            carvers: scale <= spyglass::MAX_UNDERGROUND_SCALE,
            pins: vec![Pin {
                position: [200, -300],
                colour: [0xf0, 0xb0, 0x30],
            }],
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
        if scale <= spyglass::MAX_UNDERGROUND_SCALE {
            let deep = MapArea { y: 0, ..area };
            let map = session
                .underground_map(&deep, &underground(true), None)
                .unwrap();
            for [i, j] in [[0, 0], [159, 119], [80, 60], [13, 101]] {
                let [x, z] = deep.block([i, j]);
                let before = session.block_at([x, 0, z], Carvers::Off).unwrap().block;
                let after = session.block_at([x, 0, z], Carvers::On).unwrap().block;
                let open =
                    |b: &str| matches!(b, "minecraft:air" | "minecraft:water" | "minecraft:lava");
                // At y 0 the ground is above, so the fill's air and fluid are caves.
                assert_eq!(map.is_noise_cave([i, j]), open(&before), "({x}, 0, {z})");
                if let Some(vein) = map.vein([i, j]) {
                    assert_ne!(before, "minecraft:stone", "({x}, 0, {z}) {vein:?}");
                }
                if before != after {
                    assert!(map.carver([i, j]).is_some(), "({x}, 0, {z}) carved");
                }
            }
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
