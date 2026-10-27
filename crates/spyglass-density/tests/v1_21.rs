//! 26.2's density functions (the 1.21 family) on small hand-written JSON: no game data
//! (ADR 0008). Values are worked out from `docs/specs/density-functions.md#262-evaluation`.

use std::collections::HashMap;

use serde_json::{Map as JsonMap, Value, json};
use spyglass_data::codec::LoadErrorKind;
use spyglass_density::ClimateRouter;
use spyglass_density::v1_21::{
    Binder, Function, Loader, NoiseChunk, NoiseRouter, NoiseSettings, RandomState, RouterEntry,
};

/// Resources from `registry/id` keys.
type Map = HashMap<String, Value>;

fn df(id: &str, value: Value) -> (String, Value) {
    (format!("worldgen/density_function/{id}"), value)
}

fn resources(entries: Vec<(String, Value)>) -> Map {
    entries.into_iter().collect()
}

/// A small 1.21-form noise: two octaves.
fn noise(id: &str) -> (String, Value) {
    (
        format!("worldgen/noise/{id}"),
        json!({"firstOctave": -3, "amplitudes": [1.0, 0.5]}),
    )
}

/// The function `json` (as `minecraft:f`), bound for seed 0 as `RandomState` wires it.
fn bind(map: Map, json: Value) -> Function {
    bind_seeded(map, json, 0)
}

/// [`bind`] for `seed`.
fn bind_seeded(mut map: Map, json: Value, seed: i64) -> Function {
    map.extend([df("minecraft:f", json)]);
    let mut loader = Loader::new(&map);
    let f = loader.function("minecraft:f").unwrap();
    Binder::new(loader.noises(), seed, false).bind(&f).unwrap()
}

fn value(json: Value, x: i32, y: i32, z: i32) -> f64 {
    bind(Map::new(), json).compute(x, y, z)
}

/// `{"type": "minecraft:y_clamped_gradient", ...}` from `(from_y, from_value)` to
/// `(to_y, to_value)`.
fn gradient(from_y: i32, from_value: f64, to_y: i32, to_value: f64) -> Value {
    json!({"type": "minecraft:y_clamped_gradient", "from_y": from_y, "to_y": to_y,
           "from_value": from_value, "to_value": to_value})
}

#[test]
fn constants_are_doubles_and_references_resolve() {
    let map = resources(vec![
        df("minecraft:a", json!(0.1)),
        df(
            "minecraft:b",
            json!({"type": "minecraft:constant", "argument": 0.1}),
        ),
    ]);
    // Read as f64, never narrowed.
    assert_eq!(
        bind(map.clone(), json!("minecraft:a")).compute(0, 0, 0),
        0.1
    );
    assert_eq!(
        bind(
            map,
            json!({"type": "minecraft:add", "argument1": "minecraft:a", "argument2": "minecraft:b"})
        )
        .compute(0, 0, 0),
        0.1 + 0.1
    );
}

#[test]
fn loading_follows_26_2s_codecs() {
    let load = |json: Value| {
        let map = resources(vec![df("minecraft:f", json)]);
        Loader::new(&map).function("minecraft:f").map(|_| ())
    };
    // 26.3's forms aren't 26.2's.
    let err = load(json!({"type": "minecraft:add", "left": 1, "right": 2})).unwrap_err();
    assert!(
        matches!(*err.kind, LoadErrorKind::Missing("argument1")),
        "{err}"
    );
    let err = load(json!({"type": "minecraft:sub", "argument1": 1, "argument2": 2})).unwrap_err();
    assert!(
        matches!(*err.kind, LoadErrorKind::Unsupported { .. }),
        "{err}"
    );
    let err = load(json!({"type": "minecraft:interval_select", "input": 0,
                          "thresholds": [1.0, 0.5], "functions": [0, 1, 2]}))
    .unwrap_err();
    assert_eq!(
        err.to_string(),
        "minecraft:f: threshold values must be ordered from smallest to largest"
    );
    let err = load(json!({"type": "minecraft:interval_select", "input": 0,
                          "thresholds": [1.0], "functions": [0, 1, 2]}))
    .unwrap_err();
    assert!(matches!(*err.kind, LoadErrorKind::Invalid(_)), "{err}");
    assert!(load(json!(2_000_000)).is_err());
    assert!(load(gradient(-5000, 0.0, 0, 1.0)).is_err());
    let cycle = resources(vec![
        df("minecraft:f", json!("minecraft:g")),
        df("minecraft:g", json!("minecraft:f")),
    ]);
    let err = Loader::new(&cycle).function("minecraft:f").unwrap_err();
    assert!(matches!(*err.kind, LoadErrorKind::Cycle(_)), "{err}");
}

#[test]
fn y_clamped_gradient_divides_the_coordinate() {
    let g = gradient(0, 0.0, 3, 1.0);
    // t = (1 − 0) / (3 − 0), then 0 + t·(1 − 0).
    assert_eq!(value(g.clone(), 0, 1, 0), 1.0 / 3.0);
    assert_eq!(value(g.clone(), 0, -5, 0), 0.0);
    assert_eq!(value(g, 0, 10, 0), 1.0);
    // −64..−40 from 0 to 1 at y −50: 14 / 24.
    assert_eq!(value(gradient(-64, 0.0, -40, 1.0), 7, -50, 7), 14.0 / 24.0);
}

#[test]
fn mapped_functions() {
    let at_y = |kind: &str, y: i32| {
        // The gradient is y itself from −8 to 8.
        value(
            json!({"type": kind, "argument": gradient(-8, -8.0, 8, 8.0)}),
            0,
            y,
            0,
        )
    };
    assert_eq!(at_y("minecraft:abs", -3), 3.0);
    assert_eq!(at_y("minecraft:square", -3), 9.0);
    assert_eq!(at_y("minecraft:cube", -3), -27.0);
    assert_eq!(at_y("minecraft:half_negative", -3), -1.5);
    assert_eq!(at_y("minecraft:half_negative", 3), 3.0);
    assert_eq!(at_y("minecraft:quarter_negative", -2), -0.5);
    assert_eq!(at_y("minecraft:invert", 4), 0.25);
    // Clamped to 1: 1/2 − 1/24.
    assert_eq!(at_y("minecraft:squeeze", 2), 0.5 - 1.0 / 24.0);
    assert_eq!(at_y("minecraft:squeeze", -1), -0.5 + 1.0 / 24.0);
    let clamp = |y| {
        value(
            json!({"type": "minecraft:clamp", "input": gradient(-8, -8.0, 8, 8.0),
                   "min": -1.0, "max": 2.0}),
            0,
            y,
            0,
        )
    };
    assert_eq!((clamp(-5), clamp(1), clamp(5)), (-1.0, 1.0, 2.0));
}

/// A function that's `+0.0` at y ≤ 0, rising to 1 at y 8, and its reciprocal (`+∞` at y ≤ 0).
fn zero_and_infinity() -> (Value, Value) {
    let zero = gradient(0, 0.0, 8, 1.0);
    let infinity = json!({"type": "minecraft:invert", "argument": zero.clone()});
    (zero, infinity)
}

#[test]
fn mul_tests_its_first_argument_for_zero() {
    let (zero, infinity) = zero_and_infinity();
    // Ap2: argument1 is 0, so +0 without evaluating argument2 (0 · ∞ would be NaN).
    let ap2 =
        json!({"type": "minecraft:mul", "argument1": zero.clone(), "argument2": infinity.clone()});
    assert_eq!(value(ap2, 0, 0, 0).to_bits(), 0);
    // −0 as argument1 also gives +0.
    let negative_zero =
        json!({"type": "minecraft:mul", "argument1": -1, "argument2": zero.clone()});
    assert_eq!(
        value(negative_zero.clone(), 0, 0, 0).to_bits(),
        (-0.0_f64).to_bits()
    );
    let ap2 =
        json!({"type": "minecraft:mul", "argument1": negative_zero, "argument2": infinity.clone()});
    assert_eq!(value(ap2, 0, 0, 0).to_bits(), 0);
    // A constant operand: MulOrAdd, no zero test: 0 · ∞ is NaN.
    let mul_or_add =
        json!({"type": "minecraft:mul", "argument1": 0, "argument2": infinity.clone()});
    assert!(value(mul_or_add, 0, 0, 0).is_nan());
    // Only argument1 is tested.
    let swapped = json!({"type": "minecraft:mul", "argument1": infinity, "argument2": zero});
    assert!(value(swapped, 0, 0, 0).is_nan());
}

#[test]
fn a_reference_to_a_constant_is_a_constant_only_once_stripped() {
    let (_, infinity) = zero_and_infinity();
    let map = resources(vec![df("minecraft:zero", json!(0.0))]);
    let f = json!({"type": "minecraft:mul", "argument1": "minecraft:zero", "argument2": infinity});
    // RandomState's wiring keeps the reference: Ap2, so +0.
    assert_eq!(bind(map.clone(), f.clone()).compute(0, 0, 0).to_bits(), 0);
    // The climate sampler strips it: MulOrAdd, 0 · ∞ = NaN.
    let state = router(map, &[("temperature", f)]).bind(0).unwrap();
    assert_eq!(state.entry(RouterEntry::Temperature).compute(0, 0, 0), 0.0);
    assert!(state.sampler().sample_f64(0, 0, 0)[0].is_nan());
}

#[test]
fn min_and_max_skip_by_range() {
    let (_, infinity) = zero_and_infinity();
    // 0 · ∞: NaN at y ≤ 0, with the range [0 · 1, ∞ · 0] = [0, NaN].
    let nan = json!({"type": "minecraft:mul", "argument1": 0, "argument2": infinity});
    let min = |a: f64| {
        value(
            json!({"type": "minecraft:min", "argument1": a, "argument2": nan.clone()}),
            0,
            0,
            0,
        )
    };
    // −1 is below argument2's minimum 0: argument2 isn't evaluated, so no NaN.
    assert_eq!(min(-1.0), -1.0);
    // 1 isn't: Math.min(1, NaN) is NaN.
    assert!(min(1.0).is_nan());
    let max = |a: f64| {
        value(
            json!({"type": "minecraft:max", "argument1": a, "argument2": gradient(0, 0.0, 8, 1.0)}),
            0,
            4,
            0,
        )
    };
    assert_eq!(max(2.0), 2.0);
    assert_eq!(max(0.25), 0.5);
}

#[test]
fn choices() {
    let g = gradient(-8, -8.0, 8, 8.0);
    let range = json!({"type": "minecraft:range_choice", "input": g.clone(),
                       "min_inclusive": -1.0, "max_exclusive": 1.0,
                       "when_in_range": 10, "when_out_of_range": 20});
    let r = |y| value(range.clone(), 0, y, 0);
    assert_eq!((r(-2), r(-1), r(0), r(1)), (20.0, 10.0, 10.0, 20.0));
    let select = json!({"type": "minecraft:interval_select", "input": g,
                        "thresholds": [-1.0, 1.0], "functions": [1, 2, 3]});
    let s = |y| value(select.clone(), 0, y, 0);
    assert_eq!((s(-2), s(-1), s(0), s(1), s(5)), (1.0, 2.0, 2.0, 3.0, 3.0));
}

#[test]
fn splines_narrow_their_coordinate() {
    // The identity on [0, 1]: its value is the coordinate as f32, widened.
    let spline = json!({"type": "minecraft:spline", "spline": {
        "coordinate": gradient(0, 0.0, 10, 1.0),
        "points": [
            {"location": 0.0, "value": 0.0, "derivative": 1.0},
            {"location": 1.0, "value": 1.0, "derivative": 1.0}
        ]
    }});
    assert_eq!(value(spline.clone(), 0, 1, 0), f64::from(0.1_f32));
    // Extended past the last point by its derivative.
    assert_eq!(value(spline, 0, 20, 0), 1.0);
}

#[test]
fn find_top_surface_steps_down_cells() {
    // Density positive below y 50; start = floor(70.3 / 8) · 8 = 64, then 56, 48.
    let f = json!({"type": "minecraft:find_top_surface", "density": gradient(0, 1.0, 100, -1.0),
                   "upper_bound": 70.3, "lower_bound": -64, "cell_height": 8});
    assert_eq!(value(f.clone(), 5, 0, 5), 48.0);
    let below = json!({"type": "minecraft:find_top_surface", "density": -1,
                       "upper_bound": 70.3, "lower_bound": -64, "cell_height": 8});
    assert_eq!(value(below, 0, 0, 0), -64.0);
}

#[test]
fn end_islands_central_island_is_quantized() {
    let islands = json!({"type": "minecraft:end_islands"});
    // At the origin: 100 − 0, clamped to 80; (80 − 8) / 128.
    assert_eq!(value(islands.clone(), 0, 0, 0), 0.5625);
    // Blocks 24 to 31 are 3 after dividing by 8, and z 32 is 4: 100 − 5·8 = 60, so
    // (60 − 8) / 128 for all of them (the outer islands are out of reach so close in).
    for x in [24, 31] {
        assert_eq!(value(islands.clone(), x, 70, 32), 52.0 / 128.0);
    }
    assert_ne!(value(islands, 32, 70, 32), 52.0 / 128.0);
}

#[test]
fn shifted_noise_adds_its_shifts() {
    let map = resources(vec![noise("minecraft:n")]);
    let plain = bind(
        map.clone(),
        json!({"type": "minecraft:noise", "noise": "minecraft:n", "xz_scale": 0.5, "y_scale": 2.0}),
    );
    let shifted = bind(
        map,
        json!({"type": "minecraft:shifted_noise", "noise": "minecraft:n", "xz_scale": 0.25,
               "y_scale": 1.0, "shift_x": 3.0, "shift_y": gradient(0, 0.0, 8, 8.0),
               "shift_z": -1.0}),
    );
    // x·0.25 + 3 = x·0.5 at x 12; y + y = 2y; z·0.25 − 1 = z·0.5 at z −4.
    assert_eq!(shifted.compute(12, 3, -4), plain.compute(12, 3, -4));
    assert!(plain.max_value() > 0.0);
    assert_eq!(plain.min_value(), -plain.max_value());
}

/// 26.2 noise settings, `noise` 4 × 8 cells over y −64 to 320, with router `entries` and 0 for
/// every other entry.
fn router(mut map: Map, entries: &[(&str, Value)]) -> NoiseRouter {
    let mut router = JsonMap::new();
    for entry in RouterEntry::ALL {
        router.insert(entry.name().to_string(), json!(0));
    }
    for (name, value) in entries {
        router.insert((*name).to_string(), value.clone());
    }
    map.insert(
        "worldgen/noise_settings/minecraft:test".to_string(),
        json!({
            "noise": {"min_y": -64, "height": 384, "size_horizontal": 1, "size_vertical": 2},
            "noise_router": router,
            "legacy_random_source": false,
            "aquifers_enabled": true,
            "ore_veins_enabled": true,
        }),
    );
    NoiseRouter::load(&map, "test").unwrap()
}

#[test]
fn noise_settings_load_and_clamp() {
    let r = router(Map::new(), &[]);
    let noise = r.noise_settings();
    assert_eq!((noise.cell_width(), noise.cell_height()), (4, 8));
    assert_eq!(noise.clamped(-64, 384), noise);
    let clamped = noise.clamped(0, 256);
    assert_eq!((clamped.min_y, clamped.height), (0, 256));
    let nether = NoiseSettings {
        min_y: 0,
        height: 128,
        size_horizontal: 1,
        size_vertical: 2,
    };
    assert_eq!(nether.clamped(0, 256), nether);
    assert!(r.aquifers_enabled() && r.ore_veins_enabled() && !r.legacy_random_source());
}

#[test]
fn the_climate_router_dispatches_on_the_settings_shape() {
    let mut map = resources(vec![noise("minecraft:n")]);
    let n =
        json!({"type": "minecraft:noise", "noise": "minecraft:n", "xz_scale": 1.0, "y_scale": 0.0});
    let state = router(
        map.clone(),
        &[("erosion", n.clone()), ("depth", json!(0.1))],
    )
    .bind(7)
    .unwrap();
    map.insert(
        "worldgen/noise_settings/minecraft:test".to_string(),
        json!({
            "noise": {"min_y": -64, "height": 384, "size_horizontal": 1, "size_vertical": 2},
            "noise_router": RouterEntry::ALL.iter().map(|e| (e.name().to_string(), match e.name() {
                "erosion" => n.clone(),
                "depth" => json!(0.1),
                _ => json!(0),
            })).collect::<JsonMap<_, _>>(),
            "legacy_random_source": false,
            "aquifers_enabled": true,
            "ore_veins_enabled": true,
        }),
    );
    let climate = ClimateRouter::load(&map, "test").unwrap();
    assert!(climate.functions().is_none() && climate.v1_21().is_some());
    let sampler = climate.sampler(7).unwrap();
    let doubles = state.sampler().sample_f64(3, 5, -2);
    assert_eq!(sampler.sample(3, 5, -2), doubles.map(|v| v as f32));
    // depth is 0.1 in f64; the sampler narrows it.
    assert_eq!(doubles[4], 0.1);
    assert_eq!(sampler.sample(3, 5, -2)[4], 0.1_f32);
    assert_eq!(
        doubles[3],
        state.entry(RouterEntry::Erosion).compute(12, 20, -8)
    );
}

/// A noise that depends on every axis, for the `NoiseChunk` tests.
fn wavy() -> Value {
    json!({"type": "minecraft:noise", "noise": "minecraft:n", "xz_scale": 0.7, "y_scale": 0.3})
}

/// A random state whose final density is `interpolated(wavy)` plus a non-interpolated gradient,
/// with `vein_toggle` the interpolated noise alone and `erosion` a flat-cached noise.
fn chunk_state() -> RandomState {
    let interpolated = json!({"type": "minecraft:interpolated", "argument": wavy()});
    router(
        resources(vec![noise("minecraft:n")]),
        &[
            (
                "final_density",
                json!({"type": "minecraft:add", "argument1": interpolated.clone(),
                       "argument2": gradient(-64, 0.5, 320, -0.5)}),
            ),
            ("vein_toggle", interpolated),
            (
                "erosion",
                json!({"type": "minecraft:flat_cache", "argument": wavy()}),
            ),
        ],
    )
    .bind(42)
    .unwrap()
}

#[test]
fn noise_chunk_interpolates_x_then_y_then_z() {
    let state = chunk_state();
    let raw = bind_seeded(resources(vec![noise("minecraft:n")]), wavy(), 42);
    // clampedLerp(t, 0.5, −0.5) = 0.5 + t·(−0.5 − 0.5).
    let gradient = |y: i32| {
        let t = (f64::from(y) + 64.0) / 384.0;
        0.5 + t * (-0.5 - 0.5)
    };
    let chunk = NoiseChunk::for_chunk(&state, 2, -3, -64, 384);
    assert_eq!((chunk.min_y(), chunk.height()), (-64, 384));
    let lerp = |t: f64, a: f64, b: f64| a + t * (b - a);
    for (x, y, z) in [
        (32_i32, -64_i32, -48_i32),
        (33, 5, -47),
        (35, 71, -45),
        (47, 319, -33),
        (40, 0, -40),
    ] {
        let (cx, cy, cz) = (
            x - x.rem_euclid(4),
            y - y.rem_euclid(8),
            z - z.rem_euclid(4),
        );
        let n = |dx: i32, dy: i32, dz: i32| raw.compute(cx + 4 * dx, cy + 8 * dy, cz + 4 * dz);
        let (tx, ty, tz) = (
            f64::from(x - cx) / 4.0,
            f64::from(y - cy) / 8.0,
            f64::from(z - cz) / 4.0,
        );
        // Mth.lerp3: x, then y, then z.
        let lerp2 = |dz| {
            lerp(
                ty,
                lerp(tx, n(0, 0, dz), n(1, 0, dz)),
                lerp(tx, n(0, 1, dz), n(1, 1, dz)),
            )
        };
        let cell = lerp(tz, lerp2(0), lerp2(1));
        // The gradient's own point value is added per block, then the beardifier's 0.
        let want = (cell + gradient(y)) + 0.0;
        assert_eq!(chunk.interpolated_density(x, y, z), want, "({x}, {y}, {z})");
        // Outside a fill: y, then x, then z.
        let xz = |dx, dz| lerp(ty, n(dx, 0, dz), n(dx, 1, dz));
        let positioned = lerp(
            tz,
            lerp(tx, xz(0, 0), xz(1, 0)),
            lerp(tx, xz(0, 1), xz(1, 1)),
        );
        assert_eq!(
            chunk.compute_positioned(RouterEntry::VeinToggle, x, y, z),
            positioned
        );
        // A SinglePointContext gets the raw input.
        assert_eq!(
            chunk.compute(RouterEntry::VeinToggle, x, y, z),
            raw.compute(x, y, z)
        );
    }
    // On a corner the value is the corner's.
    assert_eq!(
        chunk.interpolated_density(36, 8, -44),
        raw.compute(36, 8, -44) + gradient(8)
    );
    // The value depends only on the block: a column agrees with the chunk.
    for (x, z) in [(33, -47), (46, -34)] {
        let column = NoiseChunk::for_column(&state, x, z, -64, 384);
        assert_eq!((column.min_y(), column.height()), (-64, 384));
        for y in [-64, -1, 100, 319] {
            assert_eq!(
                column.interpolated_density(x, y, z),
                chunk.interpolated_density(x, y, z)
            );
        }
    }
}

#[test]
fn final_density_gains_the_beardifiers_zero() {
    // −1 · 0 = −0 at a point; the NoiseChunk adds the beardifier's 0, giving +0.
    let state = router(
        Map::new(),
        &[(
            "final_density",
            json!({"type": "minecraft:mul", "argument1": -1, "argument2": gradient(0, 0.0, 8, 1.0)}),
        )],
    )
    .bind(0)
    .unwrap();
    let point = state.entry(RouterEntry::FinalDensity).compute(0, -10, 0);
    assert_eq!(point.to_bits(), (-0.0_f64).to_bits());
    let chunk = NoiseChunk::for_chunk(&state, 0, 0, -64, 384);
    assert_eq!(chunk.interpolated_density(0, -10, 0).to_bits(), 0);
}

#[test]
fn flat_cache_quantizes_to_quart_corners_at_y_0_inside_its_window() {
    let state = chunk_state();
    let raw = bind_seeded(resources(vec![noise("minecraft:n")]), wavy(), 42);
    let erosion = |chunk: &NoiseChunk, x, y, z| chunk.compute(RouterEntry::Erosion, x, y, z);
    // Outside a NoiseChunk the marker is transparent.
    assert_eq!(
        state.entry(RouterEntry::Erosion).compute(5, 30, 7),
        raw.compute(5, 30, 7)
    );
    // A chunk's window: quarts 0 to 4 from its first, blocks 0 to 19.
    let chunk = NoiseChunk::for_chunk(&state, 0, 0, -64, 384);
    for (x, y, z) in [(5, 30, 7), (19, -64, 0), (0, 100, 19), (3, 3, 3)] {
        let corner = raw.compute(x & !3, 0, z & !3);
        assert_eq!(erosion(&chunk, x, y, z), corner, "({x}, {y}, {z})");
    }
    for (x, y, z) in [(20, 30, 7), (5, 30, -1), (-3, 8, 25)] {
        assert_eq!(
            erosion(&chunk, x, y, z),
            raw.compute(x, y, z),
            "({x}, {y}, {z})"
        );
    }
    // A column's window is its cell's quarts, plus one: blocks 8 to 15 for the cell at 8.
    let column = NoiseChunk::for_column(&state, 9, 9, -64, 384);
    assert_eq!(erosion(&column, 15, 30, 13), raw.compute(12, 0, 12));
    assert_eq!(erosion(&column, 16, 30, 13), raw.compute(16, 30, 13));
    // The cached climate reads it so too: the y-dependent erosion is the y 0 value.
    let climate = chunk.climate(1, 5, 2);
    assert_eq!(climate[3], raw.compute(4, 0, 8));
    assert_eq!(
        state.sampler().sample_f64(1, 5, 2)[3],
        raw.compute(4, 20, 8)
    );
}

#[test]
fn preliminary_surface_level_floors_at_quart_corners() {
    let state = router(
        resources(vec![noise("minecraft:n")]),
        &[(
            "preliminary_surface_level",
            json!({"type": "minecraft:add", "argument1": 63.5, "argument2": wavy()}),
        )],
    )
    .bind(1)
    .unwrap();
    let raw = bind_seeded(resources(vec![noise("minecraft:n")]), wavy(), 1);
    let chunk = NoiseChunk::for_chunk(&state, 0, 0, -64, 384);
    let want = |x: i32, z: i32| (63.5 + raw.compute(x, 0, z)).floor() as i32;
    assert_eq!(chunk.preliminary_surface_level(7, 30), want(4, 28));
    assert_eq!(chunk.preliminary_surface_level(-1, -5), want(-4, -8));
    let max = [0, 4, 8]
        .iter()
        .flat_map(|&x| [0, 4].map(|z| want(x, z)))
        .max()
        .unwrap();
    assert_eq!(chunk.max_preliminary_surface_level(0, 0, 9, 5), max);
}
