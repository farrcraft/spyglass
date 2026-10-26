//! Loading and evaluation on small hand-written JSON: no game data (ADR 0008).

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{Value, json};
use spyglass_data::bundle::BundleError;
use spyglass_density::{
    Binder, ClimateRouter, DensityFunction, LoadErrorKind, Loader, Resources, Sampler,
};
use spyglass_noise::Normalization;

/// Resources from a map of `registry/id` to JSON.
struct Map(HashMap<String, Value>);

impl Map {
    fn new(entries: &[(&str, Value)]) -> Self {
        Map(entries
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect())
    }
}

impl Resources for Map {
    fn resource(&self, registry: &str, id: &str) -> Result<Value, BundleError> {
        let key = format!("{registry}/{id}");
        self.0.get(&key).cloned().ok_or(BundleError::NotFound(key))
    }
}

fn df(id: &str, value: Value) -> (String, Value) {
    (format!("worldgen/density_function/{id}"), value)
}

fn bind_one(map: &Map, id: &str) -> Sampler {
    let mut loader = Loader::new(map);
    let f = loader.function(id).unwrap();
    Binder::new(loader.noises(), 0).bind(&f).unwrap()
}

fn resources(entries: Vec<(String, Value)>) -> Map {
    Map(entries.into_iter().collect())
}

#[test]
fn numbers_strings_and_objects() {
    let map = resources(vec![
        df("minecraft:a", json!(0.5)),
        df(
            "minecraft:b",
            json!({"type": "minecraft:add", "left": "minecraft:a", "right": 2}),
        ),
        df(
            "minecraft:c",
            json!({"type": "mul", "left": "a", "right": {"type": "constant", "value": -3}}),
        ),
    ]);
    assert_eq!(bind_one(&map, "minecraft:b").sample(0, 0, 0), 2.5);
    // Ids and types without a namespace are in minecraft.
    assert_eq!(bind_one(&map, "c").sample(9, 9, 9), -1.5);
}

#[test]
fn references_are_shared() {
    let map = resources(vec![
        df(
            "minecraft:leaf",
            json!({"type": "minecraft:abs", "input": -1}),
        ),
        df(
            "minecraft:root",
            json!({"type": "minecraft:add", "left": "minecraft:leaf", "right": "minecraft:leaf"}),
        ),
    ]);
    let mut loader = Loader::new(&map);
    let root = loader.function("minecraft:root").unwrap();
    let DensityFunction::Add(a, b) = &*root else {
        panic!("{root:?}")
    };
    assert!(Arc::ptr_eq(a, b));
    assert!(Arc::ptr_eq(a, &loader.function("minecraft:leaf").unwrap()));
}

#[test]
fn cycles_are_errors() {
    let map = resources(vec![
        df(
            "minecraft:a",
            json!({"type": "minecraft:abs", "input": "minecraft:b"}),
        ),
        df(
            "minecraft:b",
            json!({"type": "minecraft:cache", "input": "minecraft:a"}),
        ),
    ]);
    let err = Loader::new(&map).function("minecraft:a").unwrap_err();
    let LoadErrorKind::Cycle(cycle) = &*err.kind else {
        panic!("{err}")
    };
    assert_eq!(cycle, &["minecraft:a", "minecraft:b", "minecraft:a"]);
}

#[test]
fn errors_name_the_resource_and_path() {
    let map = resources(vec![df(
        "minecraft:x",
        json!({"type": "minecraft:add", "left": 1, "right": {"type": "minecraft:lerp", "alpha": 0, "first": {"type": "minecraft:wobble"}}}),
    )]);
    let err = Loader::new(&map).function("minecraft:x").unwrap_err();
    assert_eq!(
        err.to_string(),
        r#"minecraft:x: right.first.type: unsupported density function type "minecraft:wobble""#
    );

    let map = resources(vec![df(
        "minecraft:y",
        json!({"type": "minecraft:lerp", "alpha": 0, "first": 1}),
    )]);
    let err = Loader::new(&map).function("minecraft:y").unwrap_err();
    assert_eq!(err.to_string(), r#"minecraft:y: missing field "second""#);

    // NOISE_VALUE_CODEC accepts -1e6..=1e6.
    let map = resources(vec![df(
        "minecraft:z",
        json!({"type": "minecraft:abs", "input": 1000001}),
    )]);
    let err = Loader::new(&map).function("minecraft:z").unwrap_err();
    assert!(
        matches!(*err.kind, LoadErrorKind::OutOfRange { .. }),
        "{err}"
    );
    assert_eq!(err.path, "input");
}

#[test]
fn router_entries_may_be_inline() {
    let settings = |depth: Value| {
        (
            "worldgen/noise_settings/minecraft:test".to_string(),
            json!({"noise_router": {
                "temperature": "minecraft:t",
                "vegetation": 0.25,
                "continents": {"type": "minecraft:mul", "left": "t", "right": 2},
                "erosion": -1,
                "depth": depth,
                "ridges": 0
            }}),
        )
    };
    let map = resources(vec![df("minecraft:t", json!(0.5)), settings(json!(3))]);
    let climate = ClimateRouter::load(&map, "test")
        .unwrap()
        .sampler(0)
        .unwrap();
    assert_eq!(climate.sample(1, 2, 3), [0.5, 0.25, 1.0, -1.0, 3.0, 0.0]);

    let map = resources(vec![
        df("minecraft:t", json!(0.5)),
        settings(json!({"type": "minecraft:abs"})),
    ]);
    let err = ClimateRouter::load(&map, "test").unwrap_err();
    assert_eq!(
        err.to_string(),
        r#"minecraft:test: noise_router.depth: missing field "input""#
    );
}

#[test]
fn gradient_clamps_to_its_edges() {
    let map = resources(vec![df(
        "minecraft:g",
        json!({"type": "minecraft:gradient", "axis": "y", "from_coordinate": -64, "from_value": 1.5, "to_coordinate": 320, "to_value": -1.5}),
    )]);
    let g = bind_one(&map, "minecraft:g");
    assert_eq!(g.sample(0, -64, 0), 1.5);
    assert_eq!(g.sample(0, -1000, 0), 1.5);
    assert_eq!(g.sample(0, 320, 0), -1.5);
    assert_eq!(g.sample(0, 9999, 0), -1.5);
    // Halfway: 1.5 + 192 * (-3 / 384) = 0, exactly.
    assert_eq!(g.sample(5, 128, 5), 0.0);

    let equal = resources(vec![df(
        "minecraft:g",
        json!({"type": "minecraft:gradient", "axis": "x", "from_coordinate": 1, "from_value": 0, "to_coordinate": 1, "to_value": 1}),
    )]);
    let err = Loader::new(&equal).function("minecraft:g").unwrap_err();
    assert!(matches!(*err.kind, LoadErrorKind::Invalid(_)), "{err}");
}

#[test]
fn lerp_returns_endpoints_exactly() {
    let map = resources(vec![
        df(
            "minecraft:one",
            json!({"type": "minecraft:lerp", "alpha": {"type": "minecraft:blend_alpha"}, "first": {"type": "minecraft:blend_offset"}, "second": -0.0}),
        ),
        df(
            "minecraft:quarter",
            json!({"type": "minecraft:lerp", "alpha": 0.25, "first": 2, "second": 6}),
        ),
    ]);
    // Without blending, alpha is 1 and the result is `second` itself, even a negative zero that
    // 0 + 1 * (-0 - 0) would turn positive.
    assert_eq!(
        bind_one(&map, "minecraft:one").sample(0, 0, 0).to_bits(),
        (-0.0f32).to_bits()
    );
    assert_eq!(bind_one(&map, "minecraft:quarter").sample(0, 0, 0), 3.0);
}

#[test]
fn splines_follow_the_cubic_hermite_form() {
    let spline = |d_last: f32| {
        json!({"type": "minecraft:spline", "spline": {
            "coordinate": {"type": "minecraft:gradient", "axis": "x", "from_coordinate": 0, "from_value": 0, "to_coordinate": 4, "to_value": 1},
            "points": [
                {"location": 0, "value": 0, "derivative": 0},
                {"location": 1, "value": {"coordinate": 0, "points": [{"location": 0, "value": 1, "derivative": 0}]}, "derivative": d_last}
            ]
        }})
    };
    let map = resources(vec![
        df("minecraft:s", spline(0.0)),
        df("minecraft:t", spline(2.0)),
    ]);
    let s = bind_one(&map, "minecraft:s");
    // x = 1 gives coordinate 0.25: smoothstep 3t² − 2t³ = 0.15625 with zero derivatives.
    assert_eq!(s.sample(1, 0, 0), 0.156_25);
    assert_eq!(s.sample(2, 0, 0), 0.5);
    // At and past the last location, the last value, extended by its derivative.
    assert_eq!(s.sample(4, 0, 0), 1.0);
    let t = bind_one(&map, "minecraft:t");
    assert_eq!(
        t.sample(99, 0, 0),
        1.0,
        "the gradient clamps the coordinate to 1"
    );
}

#[test]
fn noise_parameters_have_the_codec_defaults() {
    let map = Map::new(&[
        ("worldgen/noise/minecraft:plain", json!({"base_octave": -3})),
        (
            "worldgen/noise/minecraft:full",
            json!({"base_octave": -7, "base_amplitude": 2.0, "octave_count": 2, "normalize": false, "amplitude_modifiers": [1, 0.5]}),
        ),
        (
            "worldgen/noise/minecraft:legacy",
            json!({"base_octave": 0, "normalize": "legacy"}),
        ),
        (
            "worldgen/noise/minecraft:bad",
            json!({"base_octave": 0, "octave_count": 3, "amplitude_modifiers": [1]}),
        ),
    ]);
    let mut loader = Loader::new(&map);
    let plain = loader.noise("minecraft:plain").unwrap();
    assert_eq!(
        (
            plain.base_amplitude,
            plain.octave_count,
            plain.normalize,
            plain.amplitude_modifiers.len()
        ),
        (1.0, 1, Normalization::Enabled, 0)
    );
    let full = loader.noise("full").unwrap();
    assert_eq!(full.normalize, Normalization::Disabled);
    assert_eq!(full.amplitude_modifiers, [1.0, 0.5]);
    assert_eq!(
        loader.noise("legacy").unwrap().normalize,
        Normalization::Legacy
    );
    let err = loader.noise("bad").unwrap_err();
    assert!(matches!(*err.kind, LoadErrorKind::Invalid(_)), "{err}");
}

#[test]
fn columns_fold_only_what_doesnt_depend_on_y() {
    let map = resources(vec![
        (
            "worldgen/noise/minecraft:n".to_string(),
            json!({"base_octave": -4, "octave_count": 2}),
        ),
        df(
            "minecraft:flat",
            json!({"type": "noise", "noise": "n", "xz_scale": 1, "y_scale": 0}),
        ),
        df(
            "minecraft:tall",
            json!({"type": "noise", "noise": "n", "xz_scale": 1, "y_scale": 1}),
        ),
        df(
            "minecraft:depth",
            json!({
                "type": "add",
                "left": {"type": "gradient", "axis": "y", "from_coordinate": -64,
                         "from_value": 1.5, "to_coordinate": 320, "to_value": -1.5},
                "right": {"type": "spline", "spline": {"coordinate": "flat", "points": [
                    {"location": -1, "value": 0.5, "derivative": 0},
                    {"location": 1, "value": {"coordinate": "tall", "points": [
                        {"location": 0, "value": 1, "derivative": 2}]}, "derivative": 1}]}}
            }),
        ),
    ]);
    let flat = bind_one(&map, "flat");
    let tall = bind_one(&map, "tall");
    let depth = bind_one(&map, "depth");
    assert!(!flat.depends_on_y());
    assert!(tall.depends_on_y() && depth.depends_on_y());
    // A y scale of 0 gives the same value above and below y = 0.
    assert_eq!(
        flat.sample(13, -40, 7).to_bits(),
        flat.sample(13, 40, 7).to_bits()
    );
    for (x, z) in [(0, 0), (-37, 1000), (123_456, -987_654)] {
        let column = depth.at_column(x, z);
        for y in -64..320 {
            assert_eq!(
                column.sample(x, y, z).to_bits(),
                depth.sample(x, y, z).to_bits(),
                "({x}, {y}, {z})"
            );
        }
    }
}

#[test]
fn a_dropped_function_never_answers_for_a_new_one() {
    let noises = HashMap::new();
    let mut binder = Binder::new(&noises, 0);
    // A new function of the same size is likely to land where the dropped one was; the binder
    // must still bind the new one.
    for i in 0..64 {
        let f = Arc::new(DensityFunction::Constant(i as f32));
        assert_eq!(binder.bind(&f).unwrap().sample(0, 0, 0), i as f32);
    }
}
