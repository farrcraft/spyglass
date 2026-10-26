//! Loading and evaluation on small hand-written JSON: no game data (ADR 0008).

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{Value, json};
use spyglass_data::codec::LoadErrorKind;
use spyglass_density::{Binary, Binder, ClimateRouter, DensityFunction, Loader, Sampler};
use spyglass_noise::Normalization;

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

#[test]
fn types_without_a_point_path_load_but_dont_bind() {
    let map = resources(vec![
        df(
            "minecraft:i",
            json!({"type": "interpolated", "input": 1, "cell_size_xz": 4, "cell_size_y": 8}),
        ),
        df(
            "minecraft:d",
            json!({"type": "distance_to_point", "point": [1, 2, 3], "metric": "manhattan"}),
        ),
        df("minecraft:e", json!({"type": "end_outer_islands"})),
    ]);
    let mut loader = Loader::new(&map);
    for id in ["i", "d", "e"] {
        let f = loader.function(id).unwrap();
        let err = Binder::new(loader.noises(), 0).bind(&f).unwrap_err();
        assert!(
            matches!(*err.kind, LoadErrorKind::Unsupported { .. }),
            "{err}"
        );
    }
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
