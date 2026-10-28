//! Loading and evaluation on small hand-written JSON: no game data (ADR 0008).

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{Value, json};
use spyglass_data::bundle::BundleError;
use spyglass_data::codec::LoadErrorKind;
use spyglass_data::versions::Behavior;
use spyglass_density::{
    Binary, Binder, ClimateRouter, DensityFunction, Loader, Resources, Sampler, Volume,
};
use spyglass_noise::{NormalNoiseParameters, Normalization, v1_21};

/// Resources from `registry/id` keys.
type Map = HashMap<String, Value>;

fn df(id: &str, value: Value) -> (String, Value) {
    (format!("worldgen/density_function/{id}"), value)
}

fn bind_one(map: &Map, id: &str) -> Sampler {
    let mut loader = Loader::new(map);
    let f = loader.function(id).unwrap();
    Binder::new(loader.noises(), 0).bind(&f).unwrap()
}

fn resources<K: Into<String>>(entries: Vec<(K, Value)>) -> Map {
    entries.into_iter().map(|(k, v)| (k.into(), v)).collect()
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
    let DensityFunction::Binary(Binary::Add, a, b) = &*root else {
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

/// `n` functions `minecraft:f0`, `f1`, ..., each the `abs` of the next, and the last 1.
fn chain(n: usize) -> Map {
    resources(
        (0..n)
            .map(|i| {
                let value = if i + 1 < n {
                    json!({"type": "abs", "input": format!("minecraft:f{}", i + 1)})
                } else {
                    json!(1)
                };
                df(&format!("minecraft:f{i}"), value)
            })
            .collect(),
    )
}

#[test]
fn reference_chains_have_a_depth_limit() {
    // A data pack can chain references without end; the graph is limited to 128 levels
    // rather than overflowing the stack when it's loaded, bound, evaluated or dropped. Each
    // link is two: the reference, then the function it names. The deepest allowed must fit
    // a thread's default 2 MiB stack, even in a debug build.
    std::thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(|| {
            for n in [65, 10_000] {
                let err = Loader::new(&chain(n)).function("minecraft:f0").unwrap_err();
                assert!(matches!(*err.kind, LoadErrorKind::TooDeep(128)), "{err}");
            }
            let map = chain(64);
            let mut loader = Loader::new(&map);
            let f = loader.function("minecraft:f0").unwrap();
            let sampler = Binder::new(loader.noises(), 0).bind(&f).unwrap();
            assert_eq!(sampler.sample(1, 2, 3), 1.0);
            drop((sampler, f, loader));
            // A function loaded once counts its whole height where it's referenced again: f0
            // nests 79 deep and is loaded first, so `again` reaches it from the cache, and 50
            // levels around a reference to it are still too deep.
            let mut map = chain(40);
            let wrapped = (0..50).fold(
                json!("minecraft:f0"),
                |inner, _| json!({"type": "abs", "input": inner}),
            );
            map.extend(resources(vec![df("minecraft:again", wrapped)]));
            let mut loader = Loader::new(&map);
            loader.function("minecraft:f0").unwrap();
            let err = loader.function("minecraft:again").unwrap_err();
            assert!(matches!(*err.kind, LoadErrorKind::TooDeep(128)), "{err}");
        })
        .unwrap()
        .join()
        .unwrap();
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
    let map = resources(vec![
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
    let mut v26_3 = |id| match loader.noise(id).unwrap() {
        NormalNoiseParameters::V26_3(p) => p,
        other => panic!("{id}: {other:?}"),
    };
    let plain = v26_3("minecraft:plain");
    assert_eq!(
        (
            plain.base_amplitude,
            plain.octave_count,
            plain.normalize,
            plain.amplitude_modifiers.len()
        ),
        (1.0, 1, Normalization::Enabled, 0)
    );
    let full = v26_3("full");
    assert_eq!(full.normalize, Normalization::Disabled);
    assert_eq!(full.amplitude_modifiers, [1.0, 0.5]);
    assert_eq!(v26_3("legacy").normalize, Normalization::Legacy);
    let err = loader.noise("bad").unwrap_err();
    assert!(matches!(*err.kind, LoadErrorKind::Invalid(_)), "{err}");
}

#[test]
fn noise_parameters_in_the_26_2_form() {
    let map = resources(vec![
        (
            "worldgen/noise/minecraft:old",
            json!({"firstOctave": -9, "amplitudes": [1, 0, 1.5, 0]}),
        ),
        // No range checks in 26.2's codec.
        (
            "worldgen/noise/minecraft:wide",
            json!({"firstOctave": 4000, "amplitudes": []}),
        ),
        // Both forms' keys: base_octave decides.
        (
            "worldgen/noise/minecraft:both",
            json!({"firstOctave": -9, "amplitudes": [1], "base_octave": -2}),
        ),
        (
            "worldgen/noise/minecraft:no_amplitudes",
            json!({"firstOctave": -9}),
        ),
        (
            "worldgen/noise/minecraft:bad_amplitude",
            json!({"firstOctave": -9, "amplitudes": [1, "x"]}),
        ),
    ]);
    let mut loader = Loader::new(&map);
    assert_eq!(
        loader.noise("old").unwrap(),
        NormalNoiseParameters::V1_21(v1_21::NoiseParameters::new(-9, vec![1.0, 0.0, 1.5, 0.0]))
    );
    assert_eq!(
        loader.noise("wide").unwrap(),
        NormalNoiseParameters::V1_21(v1_21::NoiseParameters::new(4000, vec![]))
    );
    assert!(matches!(
        loader.noise("both").unwrap(),
        NormalNoiseParameters::V26_3(p) if p.base_octave == -2
    ));
    let err = loader.noise("no_amplitudes").unwrap_err();
    assert!(
        matches!(*err.kind, LoadErrorKind::Missing("amplitudes")),
        "{err}"
    );
    let err = loader.noise("bad_amplitude").unwrap_err();
    assert!(err.to_string().contains("amplitudes[1]"), "{err}");
}

/// Until the 1.21-family evaluator binds them, a 26.2 noise in a 26.3 density function is an
/// error that says so, never 26.3's answer for 26.2's data.
#[test]
fn a_26_2_noise_doesnt_bind_into_the_26_3_evaluator() {
    let map = resources(vec![
        (
            "worldgen/noise/minecraft:old".to_string(),
            json!({"firstOctave": -4, "amplitudes": [1, 1]}),
        ),
        df(
            "minecraft:f",
            json!({"type": "noise", "noise": "old", "xz_scale": 1, "y_scale": 1}),
        ),
    ]);
    let mut loader = Loader::new(&map);
    let f = loader.function("f").unwrap();
    let err = Binder::new(loader.noises(), 0).bind(&f).unwrap_err();
    assert!(err.to_string().contains("1.21-family"), "{err}");
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
    // Sampling elsewhere first forgets the column, so this value is computed from scratch.
    let fresh = |x: i32, y: i32, z: i32| {
        depth.sample(x.wrapping_add(1), y, z);
        depth.sample(x, y, z)
    };
    for (x, z) in [(0, 0), (-37, 1000), (123_456, -987_654)] {
        let expected: Vec<u32> = (-64..320).map(|y| fresh(x, y, z).to_bits()).collect();
        // Down the column, y-independent parts are remembered; up it, and twice per point too.
        let down: Vec<u32> = (-64..320)
            .map(|y| depth.sample(x, y, z).to_bits())
            .collect();
        let up: Vec<u32> = (-64..320)
            .rev()
            .map(|y| {
                depth.sample(x, y, z);
                depth.sample(x, y, z).to_bits()
            })
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        assert_eq!(down, expected, "({x}, {z})");
        assert_eq!(up, expected, "({x}, {z})");
    }
}

#[test]
fn binders_never_share_remembered_values() {
    let map = resources(vec![
        (
            "worldgen/noise/minecraft:n".to_string(),
            json!({"base_octave": -4, "octave_count": 2}),
        ),
        df(
            "minecraft:tall",
            json!({"type": "noise", "noise": "n", "xz_scale": 1, "y_scale": 1}),
        ),
    ]);
    let mut loader = Loader::new(&map);
    let f = loader.function("tall").unwrap();
    let a = Binder::new(loader.noises(), 0).bind(&f).unwrap();
    let b = Binder::new(loader.noises(), 1).bind(&f).unwrap();
    let alone = |s: &Sampler| {
        s.sample(99, 9, 9);
        s.sample(5, 6, 7)
    };
    let (a0, b0) = (alone(&a), alone(&b));
    assert_ne!(a0, b0);
    // The same position, alternating between two seeds' samplers.
    for _ in 0..3 {
        assert_eq!(a.sample(5, 6, 7), a0);
        assert_eq!(b.sample(5, 6, 7), b0);
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

/// The value of `function` (written inline) at `(x, y, z)`, bound for seed 0.
fn value(function: Value, x: i32, y: i32, z: i32) -> f32 {
    bind_one(&resources(vec![df("minecraft:f", function)]), "minecraft:f").sample(x, y, z)
}

/// The value of `function` at the origin.
fn at0(function: Value) -> f32 {
    value(function, 0, 0, 0)
}

/// `{"type": <kind>, "input": <input>}`.
fn op(kind: &str, input: Value) -> Value {
    json!({"type": kind, "input": input})
}

/// `{"type": <kind>, "left": <left>, "right": <right>}`.
fn bin(kind: &str, left: Value, right: Value) -> Value {
    json!({"type": kind, "left": left, "right": right})
}

/// A constant the compiler doesn't see as one: a `cache` isn't a `ConstantFunction`.
fn hidden(v: f32) -> Value {
    op("cache", json!(v))
}

fn bits(v: f32) -> u32 {
    v.to_bits()
}

fn load_error(function: Value) -> LoadErrorKind {
    let map = resources(vec![df("minecraft:x", function)]);
    *Loader::new(&map).function("x").unwrap_err().kind
}

#[test]
fn mul_and_div_skip_the_right_operand_after_a_zero() {
    let infinity = op("reciprocal", json!(0));
    assert_eq!(at0(infinity.clone()), f32::INFINITY);
    // MulSampler: a left −0 gives +0, and ∞ on the right is never multiplied.
    assert_eq!(bits(at0(bin("mul", hidden(-0.0), infinity.clone()))), 0);
    // ConstMulSampler has no such test: 0 · ∞ is NaN.
    assert!(at0(bin("mul", json!(0), infinity)).is_nan());
    // DivSampler likewise: 0 / 0 would be NaN.
    assert_eq!(bits(at0(bin("div", hidden(-0.0), hidden(0.0)))), 0);
    // ConstDivSampler (a constant left) divides.
    assert!(at0(bin("div", json!(0), hidden(0.0))).is_nan());
    assert_eq!(at0(bin("div", json!(3), hidden(2.0))), 1.5);
}

#[test]
fn div_by_a_constant_multiplies_by_its_reciprocal() {
    let got = at0(bin("div", hidden(5.0), json!(3)));
    assert_eq!(got, 5.0 * (1.0_f32 / 3.0));
    assert_ne!(got, 5.0_f32 / 3.0, "the two differ in the last bit here");
    assert_eq!(at0(bin("sub", json!(5), hidden(2.0))), 3.0);
    assert_eq!(at0(bin("sub", hidden(5.0), json!(2))), 3.0);
}

#[test]
fn min_and_max_compile_by_range() {
    // MinSampler returns the left value when it's at or below the right's range: +0 here,
    // where Math.min would give −0.
    assert_eq!(bits(at0(bin("min", hidden(0.0), hidden(-0.0)))), 0);
    // With a constant operand it's Math.min, which orders −0 below +0.
    assert_eq!(bits(at0(bin("min", hidden(0.0), json!(-0.0)))), bits(-0.0));
    // MaxSampler, mirrored.
    assert_eq!(bits(at0(bin("max", hidden(-0.0), hidden(0.0)))), bits(-0.0));
    // An operand whose range is wholly on the winning side is the value, without comparing.
    // `nan` is NaN with range [0, 0]: 0 · ∞ is 0 at range bounds, NaN in ConstMulSampler.
    let nan = bin(
        "mul",
        json!(0),
        json!({"type": "pow", "base": 2, "exponent": 1000}),
    );
    assert!(at0(nan.clone()).is_nan());
    assert_eq!(at0(bin("max", json!(2), nan.clone())), 2.0);
    assert_eq!(at0(bin("min", json!(-1), nan.clone())), -1.0);
    // Overlapping ranges compare, and Math.max propagates NaN.
    assert!(at0(bin("max", json!(0), nan)).is_nan());
}

#[test]
fn unary_functions() {
    assert_eq!(at0(op("abs", json!(-2.5))), 2.5);
    assert_eq!(at0(op("square", json!(-3))), 9.0);
    assert_eq!(at0(op("cube", json!(-2))), -8.0);
    assert_eq!(at0(op("sqrt", json!(2))), 2.0_f64.sqrt() as f32);
    assert!(at0(op("sqrt", json!(-1))).is_nan());
    assert_eq!(at0(op("half_negative", json!(-2))), -1.0);
    assert_eq!(at0(op("half_negative", json!(2))), 2.0);
    assert_eq!(at0(op("quarter_negative", json!(-2))), -0.5);
    assert_eq!(at0(op("reciprocal", json!(4))), 0.25);
    assert_eq!(bits(at0(op("negate", json!(0)))), bits(-0.0));
    // squeeze: c/2 − c³/24 with c clamped to [−1, 1].
    assert_eq!(at0(op("squeeze", json!(2))), 0.5 - 1.0_f32 / 24.0);
    assert_eq!(at0(op("squeeze", json!(0.5))), 0.25 - 0.125_f32 / 24.0);
    assert_eq!(at0(op("log", json!(1))), 0.0);
    assert_eq!(at0(op("sign", json!(-3))), -1.0);
    assert_eq!(bits(at0(op("sign", json!(-0.0)))), bits(-0.0));
}

#[test]
fn rounding_functions() {
    let round = |kind: &str, input: f32, multiple: Option<Value>| {
        let mut f = json!({"type": kind, "input": hidden(input)});
        if let Some(m) = multiple {
            f["multiple"] = m;
        }
        at0(f)
    };
    assert_eq!(round("floor", -1.5, None), -2.0);
    assert_eq!(round("ceil", -1.5, None), -1.0);
    // Math.round: halves round up.
    assert_eq!(round("round", -2.5, None), -2.0);
    assert_eq!(round("round", 2.5, None), 3.0);
    assert_eq!(round("truncate", -1.7, None), -1.0);
    assert_eq!(round("truncate", 1.7, None), 1.0);
    // round(x / m) · m.
    assert_eq!(round("floor", 7.0, Some(json!(2))), 6.0);
    assert_eq!(round("ceil", 7.0, Some(hidden(2.0))), 8.0);
    // round(−1.5) is −1, and round(1.5) is 2.
    assert_eq!(round("round", -7.5, Some(json!(5))), -5.0);
    assert_eq!(round("round", -7.5, Some(json!(-5))), -10.0);
    // A zero multiple leaves the input as it is.
    assert_eq!(round("floor", 7.3, Some(hidden(0.0))), 7.3);
}

#[test]
fn clamp_and_pow() {
    let clamp = |input: Value| at0(json!({"type": "clamp", "input": input, "min": -1, "max": 1}));
    assert_eq!(clamp(json!(5)), 1.0);
    assert_eq!(clamp(json!(-5)), -1.0);
    assert!(clamp(op("sqrt", json!(-1))).is_nan());
    let reversed = load_error(json!({"type": "clamp", "input": 0, "min": 1, "max": -1}));
    assert!(matches!(reversed, LoadErrorKind::Invalid(_)), "{reversed}");

    let pow = |base: Value, exponent: Value| {
        at0(json!({"type": "pow", "base": base, "exponent": exponent}))
    };
    // Constant exponents of magnitude ½, 1, 2 and 3 compile to sqrt, identity, square and cube,
    // then a reciprocal for negative ones.
    assert_eq!(pow(hidden(3.0), json!(2)), 9.0);
    assert_eq!(pow(hidden(4.0), json!(-0.5)), 0.5);
    assert_eq!(pow(hidden(2.0), json!(-1)), 0.5);
    assert_eq!(pow(hidden(-2.0), json!(3)), -8.0);
    assert_eq!(pow(hidden(2.0), json!(10)), 1024.0);
    assert_eq!(pow(json!(2), hidden(3.0)), 8.0);
    assert_eq!(pow(hidden(2.0), hidden(-2.0)), 0.25);
    assert!(pow(hidden(-8.0), hidden(1.0 / 3.0)).is_nan());
}

#[test]
fn choices_evaluate_one_branch() {
    let choice = |input: Value| {
        at0(
            json!({"type": "range_choice", "input": input, "min_inclusive": 0,
            "max_exclusive": 2, "when_in_range": 10, "when_out_of_range": 20}),
        )
    };
    assert_eq!(choice(json!(0)), 10.0);
    assert_eq!(choice(json!(1.9)), 10.0);
    assert_eq!(choice(json!(2)), 20.0);
    assert_eq!(choice(json!(-1)), 20.0);
    assert_eq!(choice(op("sqrt", json!(-1))), 20.0, "NaN is out of range");

    let select = |input: Value, thresholds: Value, functions: Value| {
        json!({"type": "interval_select", "input": input, "thresholds": thresholds,
            "functions": functions})
    };
    let three = |input: Value| at0(select(input, json!([0, 1]), json!([10, 20, 30])));
    assert_eq!(three(json!(-1)), 10.0);
    assert_eq!(three(json!(0)), 20.0);
    assert_eq!(three(json!(1)), 30.0);
    assert_eq!(three(op("sqrt", json!(-1))), 30.0, "NaN selects the last");
    assert_eq!(at0(select(json!(0.5), json!([0.5]), json!([1, 2]))), 2.0);

    // Thresholds in Float.compare order, so −0 can't follow +0; one more function than
    // thresholds; at least two functions.
    for (thresholds, functions) in [
        (json!([1, 0]), json!([1, 2, 3])),
        (json!([0.0, -0.0]), json!([1, 2, 3])),
        (json!([0, 1]), json!([1, 2])),
        (json!([]), json!([1])),
    ] {
        let err = load_error(select(json!(0), thresholds, functions));
        assert!(matches!(err, LoadErrorKind::Invalid(_)), "{err}");
    }
}

#[test]
fn slices_fix_a_coordinate() {
    let x = json!({"type": "gradient", "axis": "x", "from_coordinate": 0, "from_value": 0,
        "to_coordinate": 100, "to_value": 100});
    let slice = json!({"type": "slice", "axis": "x", "coordinate": 10, "input": x.clone()});
    assert_eq!(value(slice.clone(), 70, 3, 4), 10.0);
    // The slice's input is evaluated at x 10 and the other operand at x 70, in one sample.
    assert_eq!(value(bin("add", slice, x), 70, 3, 4), 80.0);
}

#[test]
fn find_top_surface_scans_down_by_cells() {
    // density = 1 − y/50, positive below y 50.
    let density = json!({"type": "gradient", "axis": "y", "from_coordinate": 0, "from_value": 1,
        "to_coordinate": 100, "to_value": -1});
    // The upper bound is sampled at y 0, where this one is 80, whatever y is asked for.
    let upper = json!({"type": "gradient", "axis": "y", "from_coordinate": 0, "from_value": 80,
        "to_coordinate": 100, "to_value": 0});
    let surface = |density: Value, upper: Value| {
        json!({"type": "find_top_surface", "density": density, "upper_bound": upper,
            "lower_bound": -64, "cell_height": 8})
    };
    let f = surface(density.clone(), upper);
    // From floor(80 / 8) · 8 = 80 down: 48 is the first multiple of 8 with positive density.
    assert_eq!(value(f.clone(), 3, 0, 4), 48.0);
    assert_eq!(value(f, 3, 100, 4), 48.0);
    // Nothing positive: the lower bound.
    assert_eq!(at0(surface(json!(-1), json!(80))), -64.0);
    // A start at or below the lower bound is the lower bound.
    assert_eq!(at0(surface(density, json!(-100))), -64.0);
    let err = load_error(
        json!({"type": "find_top_surface", "density": 0, "upper_bound": 0,
        "lower_bound": -5000, "cell_height": 8}),
    );
    assert!(matches!(err, LoadErrorKind::OutOfRange { .. }), "{err}");
}

#[test]
fn gradients_repeat_and_mirror() {
    let gradient = |tiling: &str, from: i32, to: i32| {
        json!({"type": "gradient", "axis": "x", "tiling": tiling, "from_coordinate": from,
            "from_value": 0, "to_coordinate": to, "to_value": 1})
    };
    let repeat = gradient("repeat", 0, 4);
    assert_eq!(value(repeat.clone(), 5, 0, 0), 0.25);
    assert_eq!(value(repeat, -1, 0, 0), 0.75);
    let mirrored = gradient("mirrored_repeat", 0, 4);
    assert_eq!(value(mirrored.clone(), 5, 0, 0), 0.75);
    assert_eq!(value(mirrored.clone(), -1, 0, 0), 0.25);
    assert_eq!(value(mirrored, 9, 0, 0), 0.25);
    // A reversed range is negative, and floorMod follows its sign: floorMod(1, −4) = −3.
    assert_eq!(value(gradient("repeat", 4, 0), 5, 0, 0), 0.75);
    let err = load_error(gradient("wobble", 0, 4));
    assert!(matches!(err, LoadErrorKind::Shape(_)), "{err}");
}

#[test]
fn context_functions_without_blending_or_structures() {
    assert_eq!(at0(json!({"type": "beardifier"})), 0.0);
    assert_eq!(at0(op("blend_density", json!(1.25))), 1.25);
}

#[test]
fn old_blended_noise_depends_on_the_seed() {
    let noise = |smear: f64| {
        json!({"type": "old_blended_noise", "xz_scale": 0.25, "y_scale": 0.125, "xz_factor": 80,
            "y_factor": 160, "smear_scale_multiplier": smear})
    };
    let map = resources(vec![df("minecraft:b", noise(8.0))]);
    let mut loader = Loader::new(&map);
    let f = loader.function("b").unwrap();
    let sample = |seed| {
        Binder::new(loader.noises(), seed)
            .bind(&f)
            .unwrap()
            .sample(123, 45, -678)
    };
    let (a, b) = (sample(0), sample(1));
    assert!(a.is_finite() && b.is_finite());
    assert_ne!(a, b);
    assert_eq!(sample(0), a);
    let err = load_error(noise(0.5));
    assert!(matches!(err, LoadErrorKind::OutOfRange { .. }), "{err}");
}

/// A noise that varies along x, y and z, as `minecraft:f`, with whatever else `extra` adds.
fn with_noise(f: Value, extra: Vec<(String, Value)>) -> Map {
    let mut entries = vec![
        (
            "worldgen/noise/minecraft:n".to_string(),
            json!({"base_octave": -3, "octave_count": 2}),
        ),
        df("minecraft:f", f),
    ];
    entries.extend(extra);
    resources(entries)
}

fn noise(xz: f64, y: f64) -> Value {
    json!({"type": "noise", "noise": "n", "xz_scale": xz, "y_scale": y})
}

fn interpolated(input: Value, xz: i32, y: i32) -> Value {
    json!({"type": "interpolated", "input": input, "cell_size_xz": xz, "cell_size_y": y})
}

/// `(x, y, z)` from a volume's position index, in its index order.
fn positions(v: &Volume) -> Vec<(i32, i32, i32)> {
    let mut out = Vec::new();
    for iz in 0..v.size[2] {
        for ix in 0..v.size[0] {
            for iy in 0..v.size[1] {
                out.push((v.block_x(ix), v.block_y(iy), v.block_z(iz)));
            }
        }
    }
    out
}

#[test]
fn interpolated_points_lerp_the_cell_corners() {
    let map = with_noise(
        interpolated(noise(1.0, 1.0), 4, 8),
        vec![df("minecraft:raw", noise(1.0, 1.0))],
    );
    let f = bind_one(&map, "f");
    let raw = bind_one(&map, "raw");
    // On a corner, the input itself.
    assert_eq!(f.sample(8, -16, 4), raw.sample(8, -16, 4));
    // Inside a cell, lerp3 of the eight corners with t = offset / size.
    let c = |x, y, z| raw.sample(x, y, z);
    let lerp = |t: f32, a: f32, b: f32| a + t * (b - a);
    let (tx, ty, tz) = (1.0 / 4.0, 3.0 / 8.0, 2.0 / 4.0);
    let at_y = |y| {
        let z0 = lerp(tx, c(4, y, -8), c(8, y, -8));
        let z1 = lerp(tx, c(4, y, -4), c(8, y, -4));
        (z0, z1)
    };
    let ((a0, a1), (b0, b1)) = (at_y(16), at_y(24));
    let want = lerp(tz, lerp(ty, a0, b0), lerp(ty, a1, b1));
    assert_eq!(f.sample(5, 19, -6), want);
}

#[test]
fn interpolated_volumes_accumulate_down_each_cell() {
    let map = with_noise(
        interpolated(noise(1.0, 1.0), 4, 8),
        vec![df("minecraft:raw", noise(1.0, 1.0))],
    );
    let f = bind_one(&map, "f");
    let raw = bind_one(&map, "raw");
    let column = Volume::blocks([1, 20, 1], [5, -3, -6]);
    let values = f.sample_volume(&column);
    // fillCell: z, then x lerps, then b0 + step · dy, adding step for each next y.
    let c = |x, y, z| raw.sample(x, y, z);
    let lerp = |t: f32, a: f32, b: f32| a + t * (b - a);
    let (tx, tz) = (1.0 * 0.25, 2.0 * 0.25);
    let b = |y| {
        lerp(
            tx,
            lerp(tz, c(4, y, -8), c(4, y, -4)),
            lerp(tz, c(8, y, -8), c(8, y, -4)),
        )
    };
    let mut want = Vec::new();
    for (y0, dys) in [(-8, 5..8), (0, 0..8), (8, 0..8), (16, 0..1)] {
        let (b0, b1) = (b(y0), b(y0 + 8));
        let step = (b1 - b0) * 0.125;
        let mut v = b0 + step * dys.start as f32;
        for _ in dys {
            want.push(v);
            v += step;
        }
    }
    assert_eq!(values, want);
    // Points inside cells use lerp3 instead, which isn't always the same float.
    let points: Vec<f32> = (-3..17).map(|y| f.sample(5, y, -6)).collect();
    assert_ne!(points, values);
}

#[test]
fn volumes_agree_wherever_they_overlap_in_y_phase() {
    // A column and a chunk both start at the same y, so they give a block the same value; a
    // volume with steps samples the same blocks as one without.
    let f = interpolated(
        bin("add", noise(1.0, 1.0), op("square", noise(0.5, 2.0))),
        4,
        8,
    );
    let sampler = bind_one(&with_noise(f, vec![]), "f");
    let chunk = Volume::blocks([16, 40, 16], [-16, -64, 32]);
    let values = sampler.sample_volume(&chunk);
    for (x, z) in [(-16, 32), (-13, 39), (-1, 47)] {
        let column = sampler.sample_volume(&Volume::blocks([1, 40, 1], [x, -64, z]));
        for (iy, v) in column.iter().enumerate() {
            assert_eq!(*v, values[chunk.index(x + 16, iy as i32, z - 32)]);
        }
    }
    let stepped = Volume::new([5, 9, 3], [-15, -63, 33], [3, 4, 5]);
    let got = sampler.sample_volume(&stepped);
    let full = sampler.sample_volume(&Volume::blocks([15, 36, 15], [-15, -63, 33]));
    let full_volume = Volume::blocks([15, 36, 15], [-15, -63, 33]);
    for (i, (x, y, z)) in positions(&stepped).into_iter().enumerate() {
        assert_eq!(got[i], full[full_volume.index(x + 15, y + 63, z - 33)]);
    }
    // A volume on the corners of its cells, a cell apart, is the input's own volume.
    let corners = Volume::new([3, 4, 2], [-16, -64, 32], [4, 8, 4]);
    let input = bind_one(
        &with_noise(
            bin("add", noise(1.0, 1.0), op("square", noise(0.5, 2.0))),
            vec![],
        ),
        "f",
    );
    assert_eq!(
        sampler.sample_volume(&corners),
        input.sample_volume(&corners)
    );
}

#[test]
fn interpolated_points_remember_nothing_wrong() {
    // Points inside cells run the input's volume path inside the point path; values down and up
    // a column, mixed with volumes, must equal a fresh sampler's at each point.
    let f = bin(
        "add",
        interpolated(op("abs", noise(1.0, 1.0)), 4, 8),
        op("abs", noise(0.5, 0.0)),
    );
    let map = with_noise(f, vec![]);
    let kept = bind_one(&map, "f");
    for y in (-20..20).chain((-20..20).rev()) {
        let fresh = bind_one(&map, "f");
        assert_eq!(kept.sample(3, y, -5), fresh.sample(3, y, -5), "y {y}");
        if y % 7 == 0 {
            let v = Volume::blocks([1, 3, 1], [3, y, -5]);
            assert_eq!(kept.sample_volume(&v), fresh.sample_volume(&v));
        }
    }
}

#[test]
fn volumes_follow_the_volume_samplers() {
    // Volumes of the leaf types are their points.
    let y = json!({"type": "gradient", "axis": "y", "from_coordinate": 0, "from_value": 0,
        "to_coordinate": 1, "to_value": 1});
    let column = Volume::blocks([1, 3, 1], [0, 0, 0]);
    let at = |f: Value| bind_one(&with_noise(f, vec![]), "f").sample_volume(&column);
    // −y is +0 at y 0, and −0 · ... below: y · −1 at y 0 is −0.
    let minus_y = bin("mul", y.clone(), json!(-1));
    // MinSampler over a volume: the right replaces the left only if below it, so +0 stays
    // where Math.min, on the point path, gives −0.
    let min = bin("min", y.clone(), minus_y.clone());
    assert_eq!(bits(at(min.clone())[0]), 0);
    assert_eq!(bits(value(min, 0, 0, 0)), bits(-0.0));
    let max = bin("max", minus_y.clone(), y.clone());
    assert_eq!(bits(at(max.clone())[0]), bits(-0.0));
    assert_eq!(bits(value(max, 0, 0, 0)), 0);
    // MulSampler over a volume multiplies a zero left too: +0 · −1 is −0, where the point
    // path returns +0 without evaluating the right.
    let mul = bin("mul", y.clone(), bin("add", minus_y.clone(), json!(-1)));
    assert_eq!(bits(at(mul.clone())[0]), bits(-0.0));
    assert_eq!(bits(value(mul, 0, 0, 0)), 0);
    // Away from those cases a volume is its points.
    let mixed = bin(
        "add",
        noise(1.0, 1.0),
        json!({"type": "range_choice", "input": y.clone(), "min_inclusive": 1, "max_exclusive": 2,
            "when_in_range": op("cube", noise(1.0, 1.0)), "when_out_of_range": -1}),
    );
    let sampler = bind_one(&with_noise(mixed, vec![]), "f");
    let volume = Volume::blocks([2, 3, 2], [-1, 0, 7]);
    let points: Vec<f32> = positions(&volume)
        .into_iter()
        .map(|(x, y, z)| sampler.sample(x, y, z))
        .collect();
    assert_eq!(sampler.sample_volume(&volume), points);
}

#[test]
fn y_independent_parts_of_a_volume_are_its_points() {
    // shift_b, a spline over y-independent coordinates, and a slice sampled over a tall volume.
    let shifted = json!({"type": "noise", "noise": "n", "xz_scale": 0.25, "y_scale": 0,
        "shift_x": {"type": "shift_a", "noise": "n"}, "shift_z": {"type": "shift_b", "noise": "n"}});
    let spline = json!({"type": "spline", "spline": {"coordinate": shifted.clone(), "points": [
        {"location": -0.5, "value": -1, "derivative": 0},
        {"location": 0.5, "value": {"coordinate": "minecraft:g", "points": [
            {"location": 0, "value": 0, "derivative": 1}, {"location": 1, "value": 2, "derivative": 0}]},
            "derivative": 2}]}});
    let f = bin(
        "add",
        spline,
        json!({"type": "slice", "axis": "y", "coordinate": 5, "input": noise(1.0, 1.0)}),
    );
    let g = json!({"type": "gradient", "axis": "x", "from_coordinate": -10, "from_value": 0,
        "to_coordinate": 10, "to_value": 1});
    let sampler = bind_one(&with_noise(f, vec![df("minecraft:g", g)]), "f");
    let volume = Volume::blocks([3, 7, 2], [-9, -2, 100]);
    let points: Vec<f32> = positions(&volume)
        .into_iter()
        .map(|(x, y, z)| sampler.sample(x, y, z))
        .collect();
    assert_eq!(sampler.sample_volume(&volume), points);
}

#[test]
fn find_top_surface_over_a_volume() {
    let density = json!({"type": "gradient", "axis": "y", "from_coordinate": 0, "from_value": 1,
        "to_coordinate": 100, "to_value": -1});
    let upper = json!({"type": "gradient", "axis": "x", "from_coordinate": 0, "from_value": 40,
        "to_coordinate": 100, "to_value": 140});
    let f = json!({"type": "find_top_surface", "density": density, "upper_bound": upper,
        "lower_bound": -64, "cell_height": 8});
    let sampler = bind_one(&with_noise(f, vec![]), "f");
    let volume = Volume::blocks([3, 4, 1], [10, -10, 0]);
    let points: Vec<f32> = positions(&volume)
        .into_iter()
        .map(|(x, y, z)| sampler.sample(x, y, z))
        .collect();
    assert_eq!(sampler.sample_volume(&volume), points);
    assert_eq!(points[0], 48.0);
}

#[test]
fn distances_to_a_point() {
    let point = |metric: &str| {
        let map = resources(vec![df(
            "minecraft:d",
            json!({"type": "distance_to_point", "point": [1, 2, 3], "metric": metric}),
        )]);
        bind_one(&map, "d")
    };
    // From (-2, -2, 3) the point is (3, 4, 0) away; from (4, 2, -1), (-3, 0, 4).
    for (metric, a, b) in [
        ("euclidean", 5.0, 5.0),
        ("euclidean_squared", 25.0, 25.0),
        ("manhattan", 7.0, 7.0),
        ("chebyshev", 4.0, 4.0),
    ] {
        let s = point(metric);
        assert_eq!(s.sample(-2, -2, 3), a, "{metric}");
        assert_eq!(s.sample(4, 2, -1), b, "{metric}");
        assert!(s.depends_on_y());
        let volume = Volume::blocks([3, 4, 2], [-2, -3, 1]);
        let points: Vec<f32> = positions(&volume)
            .into_iter()
            .map(|(x, y, z)| s.sample(x, y, z))
            .collect();
        assert_eq!(s.sample_volume(&volume), points, "{metric}");
    }
    // Euclidean: (float) Math.sqrt of the float sum, so √2 rounds once, to f32.
    assert_eq!(point("euclidean").sample(0, 1, 3), 2.0_f64.sqrt() as f32);
}

#[test]
fn end_islands_near_the_origin() {
    // Within 64 cells (512 blocks) of the origin no outer island counts, so the height is -100
    // and the value (-100 - 8) / 128 for every seed; y is ignored.
    let map = resources(vec![df(
        "minecraft:e",
        json!({"type": "end_outer_islands"}),
    )]);
    for seed in [0, 7, -12_345] {
        let mut loader = Loader::new(&map);
        let f = loader.function("e").unwrap();
        let s = Binder::new(loader.noises(), seed).bind(&f).unwrap();
        assert!(!s.depends_on_y());
        for (x, y, z) in [(0, 0, 0), (-7, 100, 7), (100, -64, -150)] {
            assert_eq!(
                s.sample(x, y, z),
                -0.84375,
                "seed {seed} at ({x}, {y}, {z})"
            );
        }
        let volume = Volume::blocks([2, 3, 2], [-1, 0, -1]);
        assert_eq!(s.sample_volume(&volume), vec![-0.84375; 12]);
    }
}

#[test]
fn legacy_binding_uses_legacy_sources() {
    use spyglass_noise::{NormalNoise, Parameters};
    use spyglass_rng::{LegacyRandomSource, PositionalRandomFactory, RandomSource};
    let noise = json!({"base_octave": -7, "octave_count": 2, "base_amplitude": 0.9});
    let map = resources(vec![
        ("worldgen/noise/minecraft:n".to_string(), noise.clone()),
        (
            "worldgen/noise/minecraft:nether/temperature".to_string(),
            noise.clone(),
        ),
        df(
            "minecraft:a",
            json!({"type": "noise", "noise": "n", "xz_scale": 1, "y_scale": 1}),
        ),
        df(
            "minecraft:t",
            json!({"type": "noise", "noise": "nether/temperature", "xz_scale": 1, "y_scale": 0}),
        ),
    ]);
    let mut loader = Loader::new(&map);
    let (a, t) = (loader.function("a").unwrap(), loader.function("t").unwrap());
    let mut p = Parameters::new(-7);
    p.octave_count = 2;
    p.base_amplitude = 0.9;
    let normal = NormalNoise::from_parameters(&p).unwrap();
    let seed = 42;
    let mut legacy = Binder::with_legacy_random(loader.noises(), seed, true);
    // An ordinary noise: NormalNoise.create from the legacy factory's fromHashOf(id).
    let factory = LegacyRandomSource::new(seed).fork_positional();
    let expected = normal.create(&mut factory.from_hash_of("minecraft:n"));
    assert_eq!(
        legacy.bind(&a).unwrap().sample(10, 20, 30),
        expected.get(10.0, 20.0, 30.0)
    );
    // The Nether's temperature: its own construction from LegacyRandomSource(seed), with or
    // without legacy randomness.
    let nether = normal
        .create_for_legacy_nether_biome(&mut LegacyRandomSource::new(seed))
        .unwrap();
    let want = nether.get(10.0, 0.0, 30.0);
    assert_eq!(legacy.bind(&t).unwrap().sample(10, 20, 30), want);
    let mut xoroshiro = Binder::new(loader.noises(), seed);
    assert_eq!(xoroshiro.bind(&t).unwrap().sample(10, 20, 30), want);
    assert_ne!(
        xoroshiro.bind(&a).unwrap().sample(10, 20, 30),
        expected.get(10.0, 20.0, 30.0)
    );
}

#[test]
fn bad_parameters_are_errors() {
    let zero_cells = load_error(
        json!({"type": "interpolated", "input": 1, "cell_size_xz": 0, "cell_size_y": 8}),
    );
    assert!(
        matches!(zero_cells, LoadErrorKind::OutOfRange { .. }),
        "{zero_cells}"
    );
    let short =
        load_error(json!({"type": "distance_to_point", "point": [1, 2], "metric": "manhattan"}));
    assert!(matches!(short, LoadErrorKind::Shape(_)), "{short}");
}

/// `{"type": "upscale", ...}` with `(mode, cell_size)` for x, y and z.
fn upscale(input: Value, [x, y, z]: [(&str, i32); 3]) -> Value {
    let axis = |(mode, cell): (&str, i32)| json!({"mode": mode, "cell_size": cell});
    json!({"type": "upscale", "input": input, "x": axis(x), "y": axis(y), "z": axis(z)})
}

/// 26.4's form of `interpolated(input, xz, y)`: all three axes linear.
fn linear(input: Value, xz: i32, y: i32) -> Value {
    let l = "interpolate_linear";
    upscale(input, [(l, xz), (l, y), (l, xz)])
}

/// A linear upscale's weights: `(1 − t)·a + t·b`.
fn weigh(t: f32, a: f32, b: f32) -> f32 {
    (1.0 - t) * a + t * b
}

#[test]
fn upscale_points_weigh_z_then_x_then_y() {
    let map = with_noise(
        linear(noise(1.0, 1.0), 4, 8),
        vec![df("minecraft:raw", noise(1.0, 1.0))],
    );
    let f = bind_one(&map, "f");
    let raw = bind_one(&map, "raw");
    // On a corner, the input itself.
    assert_eq!(f.sample(8, -16, 4), raw.sample(8, -16, 4));
    // Inside a cell, the corners weighed along z, then x, then y, each t a division.
    let c = |x, y, z| raw.sample(x, y, z);
    let (tx, ty, tz) = (1.0 / 4.0, 3.0 / 8.0, 2.0 / 4.0);
    let along_x = |y| {
        weigh(
            tx,
            weigh(tz, c(4, y, -8), c(4, y, -4)),
            weigh(tz, c(8, y, -8), c(8, y, -4)),
        )
    };
    assert_eq!(f.sample(5, 19, -6), weigh(ty, along_x(16), along_x(24)));
}

#[test]
fn upscale_volumes_step_down_each_cell_from_its_corner() {
    let map = with_noise(
        linear(noise(1.0, 1.0), 4, 8),
        vec![df("minecraft:raw", noise(1.0, 1.0))],
    );
    let f = bind_one(&map, "f");
    let raw = bind_one(&map, "raw");
    let values = f.sample_volume(&Volume::blocks([1, 20, 1], [5, -3, -6]));
    // z and x weighed as on the point path; y is a + ((b − a) · (1/8)) · dy, from each cell's
    // corner, not accumulated from the volume's first y.
    let c = |x, y, z| raw.sample(x, y, z);
    let along_x = |y| {
        weigh(
            0.25,
            weigh(0.5, c(4, y, -8), c(4, y, -4)),
            weigh(0.5, c(8, y, -8), c(8, y, -4)),
        )
    };
    let mut want = Vec::new();
    for (y0, dys) in [(-8, 5..8), (0, 0..8), (8, 0..8), (16, 0..1)] {
        // The last block is on a corner, and its cell's far corner is clamped to that corner.
        let (a, b) = (along_x(y0), along_x((y0 + 8).min(16)));
        for dy in dys {
            want.push(a + ((b - a) * 0.125) * dy as f32);
        }
    }
    assert_eq!(values, want);
    // Points use the weights along y instead, which aren't always the same float.
    let points: Vec<f32> = (-3..17).map(|y| f.sample(5, y, -6)).collect();
    assert_ne!(points, values);
    // A block's value depends only on where it is in its cell: any volume gives it.
    let chunk = Volume::blocks([16, 30, 16], [0, -10, -16]);
    let in_chunk = f.sample_volume(&chunk);
    for (iy, v) in values.iter().enumerate() {
        assert_eq!(*v, in_chunk[chunk.index(5, iy as i32 + 7, 10)]);
    }
}

#[test]
fn linear_upscale_is_not_interpolated() {
    // The same input and cells, in 26.3's form and 26.4's. Corners agree; inside cells the
    // weights, the order of the axes and the y steps differ in the low bits.
    let input = bin("add", noise(1.0, 1.0), op("square", noise(0.5, 2.0)));
    let old = bind_one(&with_noise(interpolated(input.clone(), 4, 8), vec![]), "f");
    let new = bind_one(&with_noise(linear(input, 4, 8), vec![]), "f");
    assert_eq!(old.sample(-16, 8, 12), new.sample(-16, 8, 12));
    let points = |s: &Sampler| -> Vec<f32> {
        (0..64)
            .map(|i| s.sample(i % 7 - 3, i - 30, i / 5))
            .collect()
    };
    assert_ne!(points(&old), points(&new));
    let chunk = Volume::blocks([16, 24, 16], [-16, -61, 32]);
    let (a, b) = (old.sample_volume(&chunk), new.sample_volume(&chunk));
    assert_eq!(a.len(), b.len());
    assert_ne!(a, b);
    // interpolated's volume depends on where the volume starts in y; upscale's doesn't.
    let column = |s: &Sampler, y0| s.sample_volume(&Volume::blocks([1, 16, 1], [-13, y0, 35]));
    let shifted = |s: &Sampler| -> Vec<(f32, f32)> {
        let (low, high) = (column(s, -61), column(s, -59));
        (0..14).map(|i| (low[i + 2], high[i])).collect()
    };
    assert!(shifted(&old).iter().any(|(l, h)| l != h));
    assert!(shifted(&new).iter().all(|(l, h)| l == h));
}

#[test]
fn upscale_steps_x_when_y_is_nearest() {
    // chunk_surface_level's shape: y cells of 1 are always on a corner, so y is nearest over a
    // volume, and x takes the step form a + ((b − a) · (1/4)) · dx.
    let l = "interpolate_linear";
    let map = with_noise(
        upscale(noise(1.0, 1.0), [(l, 4), (l, 1), (l, 4)]),
        vec![df("minecraft:raw", noise(1.0, 1.0))],
    );
    let f = bind_one(&map, "f");
    let raw = bind_one(&map, "raw");
    let c = |x, y, z| raw.sample(x, y, z);
    let along_z = |x, y| weigh(0.5, c(x, y, -8), c(x, y, -4));
    let row = f.sample_volume(&Volume::blocks([8, 1, 1], [0, 5, -6]));
    let mut want = Vec::new();
    for x0 in [0, 4] {
        let (a, b) = (along_z(x0, 5), along_z(x0 + 4, 5));
        for dx in 0..4 {
            want.push(a + ((b - a) * 0.25) * dx as f32);
        }
    }
    assert_eq!(row, want);
    // The point path weighs x, and samples y and y + 1 with weights 1 and 0.
    let along_x = |y| weigh(0.25, along_z(0, y), along_z(4, y));
    assert_eq!(f.sample(1, 5, -6), 1.0 * along_x(5) + 0.0 * along_x(6));
}

#[test]
fn stepped_upscale_volumes_pick_from_blocks() {
    let f = linear(
        bin("add", noise(1.0, 1.0), op("square", noise(0.5, 2.0))),
        4,
        8,
    );
    let sampler = bind_one(&with_noise(f, vec![]), "f");
    let full_volume = Volume::blocks([16, 40, 16], [-16, -64, 32]);
    let full = sampler.sample_volume(&full_volume);
    let picks = |stepped: Volume| {
        let got = sampler.sample_volume(&stepped);
        for (i, (x, y, z)) in positions(&stepped).into_iter().enumerate() {
            assert_eq!(
                got[i],
                full[full_volume.index(x + 16, y + 64, z - 32)],
                "{stepped:?} at {x} {y} {z}"
            );
        }
    };
    // Steps that don't divide the cells: no plan, so the blocks are upscaled and picked from.
    picks(Volume::new([5, 9, 3], [-15, -63, 33], [3, 4, 5]));
    // Steps that divide them, from a corner: planned in cells of steps.
    picks(Volume::new([8, 10, 8], [-16, -64, 32], [2, 4, 2]));
    // From off a corner, with steps that divide the cells: no plan either.
    picks(Volume::new([7, 9, 7], [-14, -62, 34], [2, 4, 2]));
    // On corners, a cell apart: the input's own volume.
    let corners = Volume::new([3, 4, 2], [-16, -64, 32], [4, 8, 4]);
    let input = bind_one(
        &with_noise(
            bin("add", noise(1.0, 1.0), op("square", noise(0.5, 2.0))),
            vec![],
        ),
        "f",
    );
    assert_eq!(
        sampler.sample_volume(&corners),
        input.sample_volume(&corners)
    );
}

#[test]
fn cubic_upscales_along_one_axis() {
    // x² along x (0 to 64), in cells of 4 along x only.
    let x = json!({"type": "gradient", "axis": "x", "from_coordinate": 0, "from_value": 0,
        "to_coordinate": 64, "to_value": 64});
    let along_x = |mode| {
        let n = "nearest_neighbor";
        let map = resources(vec![df(
            "minecraft:f",
            upscale(op("square", x.clone()), [(mode, 4), (n, 1), (n, 1)]),
        )]);
        bind_one(&map, "f")
    };
    let cubic = along_x("interpolate_cubic");
    // Catmull-Rom is exact on a corner, and reproduces a quadratic between: at x = 10 the
    // samples 16, 64, 144, 256 weigh (−1/16, 9/16, 9/16, −1/16).
    assert_eq!(cubic.sample(8, 0, 0), 64.0);
    assert_eq!(cubic.sample(10, 0, 0), 100.0);
    // A B-spline isn't exact on a corner: (1/6, 4/6, 1/6, 0) of the samples around it.
    let b_spline = along_x("b_spline_cubic");
    let (c1, c2, c3) = (1.0_f32 / 6.0, 4.0_f32 / 6.0, 1.0_f32 / 6.0);
    assert_eq!(
        b_spline.sample(8, 0, 0),
        ((c1 * 16.0 + c2 * 64.0) + c3 * 144.0) + 0.0 * 256.0
    );
    // With y and z nearest, a volume weighs x as a point does.
    let row = Volume::blocks([17, 1, 1], [3, 0, 0]);
    for s in [&cubic, &b_spline] {
        let points: Vec<f32> = (3..20).map(|x| s.sample(x, 0, 0)).collect();
        assert_eq!(s.sample_volume(&row), points);
    }
}

#[test]
fn nearest_upscales_take_the_cell_corner() {
    let n = "nearest_neighbor";
    let map = with_noise(
        upscale(noise(1.0, 1.0), [(n, 4), (n, 8), (n, 4)]),
        vec![df("minecraft:raw", noise(1.0, 1.0))],
    );
    let f = bind_one(&map, "f");
    let raw = bind_one(&map, "raw");
    assert_eq!(f.sample(5, 19, -6), raw.sample(4, 16, -8));
    assert_eq!(f.sample(-1, -1, -1), raw.sample(-4, -8, -4));
    // Over a volume off the grid, the corners are sampled once and copied to their cells.
    let volume = Volume::new([6, 10, 3], [5, -3, -6], [1, 2, 3]);
    let want: Vec<f32> = positions(&volume)
        .into_iter()
        .map(|(x, y, z)| f.sample(x, y, z))
        .collect();
    assert_eq!(f.sample_volume(&volume), want);
    // On the grid, the input's own volume.
    let grid = Volume::new([2, 2, 2], [4, 8, -8], [4, 8, 4]);
    assert_eq!(f.sample_volume(&grid), raw.sample_volume(&grid));
}

#[test]
fn upscale_axes_follow_the_codec() {
    let bad = |axis: Value| {
        let l = json!({"mode": "interpolate_linear", "cell_size": 4});
        load_error(json!({"type": "upscale", "input": 1, "x": l, "y": axis, "z": l}))
    };
    for cell in [0, 65] {
        let e = bad(json!({"mode": "interpolate_linear", "cell_size": cell}));
        assert!(matches!(e, LoadErrorKind::OutOfRange { .. }), "{e}");
    }
    let e = bad(json!({"mode": "interpolate_quadratic", "cell_size": 4}));
    assert!(matches!(e, LoadErrorKind::Shape(_)), "{e}");
    let e = bad(json!({"mode": "interpolate_linear"}));
    assert!(matches!(e, LoadErrorKind::Missing("cell_size")), "{e}");
    let e = load_error(json!({"type": "upscale", "input": 1, "x": {}, "z": {}}));
    assert!(matches!(e, LoadErrorKind::Missing(_)), "{e}");
    // Every mode and the largest cell load.
    for mode in [
        "nearest_neighbor",
        "interpolate_linear",
        "interpolate_cosine",
        "interpolate_cubic",
        "b_spline_cubic",
    ] {
        let a = json!({"mode": mode, "cell_size": 64});
        assert_eq!(
            at0(json!({"type": "upscale", "input": 2, "x": a, "y": a, "z": a})),
            2.0,
            "{mode}"
        );
    }
}

/// Hand-built resources with some behavior switches on.
struct Switched {
    map: Map,
    behavior: Behavior,
}

impl Resources for Switched {
    fn resource(&self, registry: &str, id: &str) -> Result<Value, BundleError> {
        self.map.resource(registry, id)
    }

    fn behavior(&self) -> Behavior {
        self.behavior
    }
}

/// `function` bound for seed 0, from resources with `behavior`.
fn bind_with(function: Value, behavior: Behavior) -> Sampler {
    let resources = Switched {
        map: resources(vec![df("minecraft:f", function)]),
        behavior,
    };
    let mut loader = Loader::new(&resources);
    let f = loader.function("f").unwrap();
    Binder::new(loader.noises(), 0).bind(&f).unwrap()
}

#[test]
fn clamp_positive_zero_switch() {
    let mut on = Behavior::default();
    on.clamp_positive_zero = true;
    let off = Behavior::default();
    let point = |f: &Value, b| bits(bind_with(f.clone(), b).sample(0, 0, 0));
    let volume = |f: &Value, b| {
        bits(bind_with(f.clone(), b).sample_volume(&Volume::blocks([1, 1, 1], [0, 0, 0]))[0])
    };
    let clamp = |input: Value, min: f32, max: f32| json!({"type": "clamp", "input": input, "min": min, "max": max});
    // −y along a gradient: −0 at y 0, in [−1, 0]; and the same over [−1, 2], in [−2, 1].
    let gradient = |from: i32, to: i32| {
        json!({"type": "gradient", "axis": "y", "from_coordinate": from, "from_value": from,
            "to_coordinate": to, "to_value": to})
    };
    let narrow = bin("mul", gradient(0, 1), json!(-1));
    let wide = bin("mul", gradient(-1, 2), json!(-1));
    let (neg, pos) = ((-0.0_f32).to_bits(), 0);
    // Mth.clamp keeps −0: it isn't below +0.
    let f = clamp(wide.clone(), 0.0, 0.5);
    assert_eq!((point(&f, off), volume(&f, off)), (neg, neg));
    // Math.clamp lifts it to +0, at a point and over a volume.
    assert_eq!((point(&f, on), volume(&f, on)), (pos, pos));
    // max holds the range's top, so only the lower bound is applied, by ConstMaxSampler:
    // Math.max at a point, but over a volume +0 replaces −0 only if above it.
    let f = clamp(narrow, 0.0, 1.0);
    assert_eq!(point(&f, off), neg);
    assert_eq!((point(&f, on), volume(&f, on)), (pos, neg));
    // Both bounds hold the range: the input itself, unclamped.
    let f = clamp(hidden(-0.0), 0.0, 1.0);
    assert_eq!((point(&f, on), volume(&f, on)), (neg, neg));
}

#[test]
fn upscale_offset_by_step_switch() {
    let mut on = Behavior::default();
    on.upscale_offset_by_step = true;
    let off = Behavior::default();
    // Linear in x over cells of 6, from 0 at x 0 to 7 at x 6 and beyond.
    let f = json!({"type": "upscale",
        "input": {"type": "gradient", "axis": "x", "from_coordinate": 0, "from_value": 0,
            "to_coordinate": 6, "to_value": 7},
        "x": {"mode": "interpolate_linear", "cell_size": 6},
        "y": {"mode": "nearest_neighbor", "cell_size": 1},
        "z": {"mode": "nearest_neighbor", "cell_size": 1}});
    // x 3, 6, 9: three blocks apart, starting half a cell (one step) into cell 0.
    let volume = Volume::new([3, 1, 1], [3, 0, 0], [3, 1, 1]);
    let values = |b| bind_with(f.clone(), b).sample_volume(&volume);
    let b = 7.0_f32;
    // Off: the offset −3 isn't a multiple of the cell, so every block is upscaled and every third
    // picked: x 3 is 3 blocks into a cell of 6.
    assert_eq!(values(off), vec![(b * (1.0 / 6.0)) * 3.0, b, b]);
    // On: planned in steps, two to a cell, so x 3 is one step into a cell of 2. 1/6 isn't exact
    // in f32, so the two differ: 3.5000002 and 3.5.
    assert_eq!(values(on), vec![b * 0.5, b, b]);
    assert_ne!(values(on)[0], values(off)[0]);
}

#[test]
fn spline_nan_is_nan_switch() {
    let mut on = Behavior::default();
    on.spline_nan_is_nan = true;
    let off = Behavior::default();
    let spline = |coordinate: Value, first: f32| {
        json!({"type": "spline", "spline": {"coordinate": coordinate, "points": [
            {"location": 0, "value": first, "derivative": 0},
            {"location": 1, "value": 7, "derivative": 0}
        ]}})
    };
    let at = |f: &Value, b| bind_with(f.clone(), b).sample(0, 0, 0);
    // A NaN coordinate: 26.3 finds no location above it and takes the last point; 26.4 is NaN.
    let nan = spline(op("sqrt", json!(-1)), 0.0);
    assert_eq!(at(&nan, off), 7.0);
    assert!(at(&nan, on).is_nan());
    // On the first location: 26.3 computes the first interval at t = 0, −0 + 0·(7 − −0) = +0;
    // 26.4 extends the first point, −0.
    let first = spline(json!(0), -0.0);
    assert_eq!(bits(at(&first, off)), 0);
    assert_eq!(bits(at(&first, on)), (-0.0_f32).to_bits());
    // Elsewhere the two agree: inside an interval, and past either end.
    let x = json!({"type": "gradient", "axis": "x", "from_coordinate": -4, "from_value": -1,
        "to_coordinate": 8, "to_value": 2});
    let inside = spline(x, 3.0);
    for x in -6..10 {
        let (a, b) = (
            bind_with(inside.clone(), off).sample(x, 0, 0),
            bind_with(inside.clone(), on).sample(x, 0, 0),
        );
        assert_eq!(bits(a), bits(b), "x {x}");
    }
}
