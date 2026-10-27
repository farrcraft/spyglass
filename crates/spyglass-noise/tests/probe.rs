//! Every noise sample in the oracle's probe files, reproduced bit for bit, with the game's parity
//! descriptions as a structural check.
//!
//! Needs `cargo xtask oracle probe` and a bundle for the version under test
//! (`spyglass_oracle::version`). Run with `cargo test -p spyglass-noise -- --ignored`.
//!
//! The bundle's noise files say which family the version has (ADR 0026): 26.3's `base_octave`
//! form, sampled in `f32`, or 26.2's `firstOctave` form, the 1.21 family, sampled in `f64`.

use std::collections::HashMap;
use std::path::Path;

use serde_json::Value;
use spyglass_noise::{
    NoiseStack, NormalNoise, NormalNoiseParameters, Parameters, PerlinNoise, v1_21,
};
use spyglass_oracle::probe::{Case, ProbeFile, probe_file};
use spyglass_rng::{
    LegacyRandomSource, PositionalRandomFactory, RandomSource, XoroshiroRandomSource,
};

/// A noise's parameters from `bundles/<version>/data/<ns>/worldgen/noise/<path>.json`, in
/// whichever form the file has. Only what the climate noises use; the real parser belongs to the
/// density layer.
fn parameters(bundle: &Path, id: &str) -> NormalNoiseParameters {
    let (namespace, path) = id.split_once(':').expect("a namespaced id");
    let file = bundle.join(format!("data/{namespace}/worldgen/noise/{path}.json"));
    let json: Value = serde_json::from_slice(
        &std::fs::read(&file).unwrap_or_else(|e| panic!("{}: {e}", file.display())),
    )
    .unwrap();
    let numbers = |v: &Value| -> Vec<f64> {
        v.as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect()
    };
    if json.get("base_octave").is_none() {
        return v1_21::NoiseParameters::new(
            json["firstOctave"].as_i64().unwrap() as i32,
            numbers(&json["amplitudes"]),
        )
        .into();
    }
    let mut p = Parameters::new(json["base_octave"].as_i64().unwrap() as i32);
    if let Some(a) = json.get("base_amplitude") {
        p.base_amplitude = a.as_f64().unwrap();
    }
    if let Some(n) = json.get("octave_count") {
        p.octave_count = n.as_i64().unwrap() as i32;
    }
    assert!(
        json.get("normalize").is_none(),
        "{id}: normalize isn't handled here"
    );
    if let Some(m) = json.get("amplitude_modifiers") {
        p.amplitude_modifiers = numbers(m);
    }
    p.into()
}

/// `xo`, `yo`, `zo`, `p0` and `p255` from each `xo=..., p255=...` group of a parity string.
fn parity_groups(text: &str) -> Vec<([f64; 3], [i64; 2])> {
    text.split("xo=")
        .skip(1)
        .map(|group| {
            let field = |name: &str| -> &str {
                let start = group.find(name).map(|i| i + name.len()).unwrap_or(0);
                let rest = &group[start..];
                let end = rest.find([',', ']', '}']).unwrap_or(rest.len());
                rest[..end].trim()
            };
            let xo: f64 = group[..group.find(',').unwrap()].parse().unwrap();
            (
                [
                    xo,
                    field("yo=").parse().unwrap(),
                    field("zo=").parse().unwrap(),
                ],
                [
                    field("p0=").parse().unwrap(),
                    field("p255=").parse().unwrap(),
                ],
            )
        })
        .collect()
}

/// One octave's offsets and its permutation's first and last entries, as signed bytes.
type Description = ([f64; 3], [i64; 2]);

fn perlin_description(noise: &PerlinNoise) -> Description {
    (
        noise.offset(),
        [i64::from(noise.perm(0)), i64::from(noise.perm(255))],
    )
}

fn improved_description(noise: &v1_21::ImprovedNoise) -> Description {
    (
        noise.offset(),
        [i64::from(noise.perm(0)), i64::from(noise.perm(255))],
    )
}

/// The game's description of an octave against spyglass's. Offsets are compared to within the
/// game's 3-decimal formatting of the `float` value.
fn check_parity(label: &str, game: Description, ours: Description) -> Option<String> {
    let offsets = ours.0.map(|o| f64::from(o as f32));
    let close = game
        .0
        .iter()
        .zip(offsets)
        .all(|(g, o)| (g - o).abs() <= 0.000_51);
    (!close || game.1 != ours.1).then(|| {
        format!(
            "{label}: parity differs\n    game:     offsets {:?}, perms {:?}\n    spyglass: offsets {offsets:?}, perms {:?}",
            game.0, game.1, ours.1
        )
    })
}

/// Every group of the game's parity text against spyglass's octaves, in order.
fn check_all_parity(case: &Case, ours: Vec<Description>) -> Option<String> {
    let groups = parity_groups(case.text());
    if groups.len() != ours.len() {
        return Some(format!(
            "{}: {} octaves in the game, {} in spyglass",
            case.label(),
            groups.len(),
            ours.len()
        ));
    }
    groups
        .into_iter()
        .zip(ours)
        .find_map(|(g, o)| check_parity(&case.label(), g, o))
}

/// A sample, `f32` (26.3) or `f64` (26.2) as the case's values are, compared bit for bit.
fn sample(case: &Case, get: impl Fn(&[f64]) -> f64) -> Option<String> {
    let point = case.point_f64();
    let got = get(&point);
    let (want, got) = if case.kind_of_values == "f64" {
        (case.f64_bits()[0], got.to_bits())
    } else {
        (
            u64::from(case.f32_bits()[0]),
            u64::from((got as f32).to_bits()),
        )
    };
    (want != got).then(|| {
        format!(
            "{}\n    game:     {want:016x}\n    spyglass: {got:016x}",
            case.label()
        )
    })
}

fn stack_sample(case: &Case, stack: &NoiseStack) -> Option<String> {
    match case.method.as_deref() {
        Some("get_3d") => sample(case, |p| f64::from(stack.get(p[0], p[1], p[2]))),
        Some("get_2d") => sample(case, |p| f64::from(stack.get_2d(p[0], p[1]))),
        other => panic!("{}: unknown method {other:?}", case.label()),
    }
}

/// A normal noise of either family, seeded.
enum Normal {
    V26_3(NoiseStack),
    V1_21(v1_21::NormalNoise),
}

/// A probe case's kind, or `""`.
fn kind(case: &Case) -> String {
    case.kind.clone().unwrap_or_default()
}

/// 26.2's (1.21-family) cases: every primitive on its own, sampled in `f64`.
fn v1_21_case(
    case: &Case,
    seed: i64,
    improveds: &mut HashMap<i64, v1_21::ImprovedNoise>,
    sets: &mut HashMap<(String, i64), v1_21::PerlinNoise>,
    simplexes: &mut HashMap<i64, v1_21::SimplexNoise>,
    blendeds: &mut HashMap<(String, i64), v1_21::BlendedNoise>,
) -> Option<String> {
    let method = case.method.as_deref();
    let unknown = || -> ! { panic!("{}: unknown method {method:?}", case.label()) };
    match case.test.as_str() {
        // One lattice straight from a seeded random.
        "improved_noise_parity" | "improved_noise" => {
            let noise = improveds.entry(seed).or_insert_with(|| {
                v1_21::ImprovedNoise::new(&mut XoroshiroRandomSource::new(seed))
            });
            match (case.test.as_str(), method) {
                ("improved_noise_parity", _) => {
                    check_all_parity(case, vec![improved_description(noise)])
                }
                (_, Some("noise")) => sample(case, |p| noise.noise(p[0], p[1], p[2])),
                (_, Some("noise_smeared")) => {
                    sample(case, |p| noise.noise_smeared(p[0], p[1], p[2], p[3], p[4]))
                }
                _ => unknown(),
            }
        }
        // Octave sets: kind "new" is PerlinNoise.create(Xoroshiro(seed), -4, 1, 0.5, 0, 2);
        // "legacy" is createLegacyForBlendedNoise(Legacy(seed), -7..=0).
        "perlin_noise_parity" | "perlin_noise" => {
            let kind = kind(case);
            let set = sets
                .entry((kind.clone(), seed))
                .or_insert_with(|| match kind.as_str() {
                    "new" => v1_21::PerlinNoise::new(
                        &mut XoroshiroRandomSource::new(seed),
                        -4,
                        vec![1.0, 0.5, 0.0, 2.0],
                    ),
                    "legacy" => v1_21::PerlinNoise::new_legacy_for_blended_noise(
                        &mut LegacyRandomSource::new(seed),
                        -7,
                        0,
                    )
                    .unwrap(),
                    other => panic!("{}: unknown kind {other}", case.label()),
                });
            match (case.test.as_str(), method) {
                ("perlin_noise_parity", _) => check_all_parity(
                    case,
                    set.levels()
                        .iter()
                        .flatten()
                        .map(improved_description)
                        .collect(),
                ),
                (_, Some("get_3d")) => sample(case, |p| set.get_value(p[0], p[1], p[2])),
                (_, Some("get_smeared")) => sample(case, |p| {
                    set.get_value_smeared(p[0], p[1], p[2], p[3], p[4])
                }),
                _ => unknown(),
            }
        }
        // The End islands': Legacy(seed), 17292 values skipped.
        "simplex_noise" => {
            let noise = simplexes.entry(seed).or_insert_with(|| {
                let mut random = LegacyRandomSource::new(seed);
                random.consume_count(17292);
                v1_21::SimplexNoise::new(&mut random)
            });
            match method {
                Some("get_2d") => sample(case, |p| noise.get_value_2d(p[0], p[1])),
                Some("get_3d") => sample(case, |p| noise.get_value(p[0], p[1], p[2])),
                _ => unknown(),
            }
        }
        // The overworld's old_blended_noise, seeded from the factory's "minecraft:terrain"
        // (kind "xoroshiro") or Legacy(seed) (kind "legacy").
        "blended_noise" => {
            let kind = kind(case);
            let noise =
                blendeds
                    .entry((kind.clone(), seed))
                    .or_insert_with(|| match kind.as_str() {
                        "xoroshiro" => v1_21::BlendedNoise::new(
                            &mut XoroshiroRandomSource::new(seed)
                                .fork_positional()
                                .from_hash_of("minecraft:terrain"),
                            0.25,
                            0.125,
                            80.0,
                            160.0,
                            8.0,
                        ),
                        "legacy" => v1_21::BlendedNoise::new(
                            &mut LegacyRandomSource::new(seed),
                            0.25,
                            0.125,
                            80.0,
                            160.0,
                            8.0,
                        ),
                        other => panic!("{}: unknown kind {other}", case.label()),
                    });
            let [x, y, z] = case.pos.expect("a block position");
            let (want, got) = (case.f64_bits()[0], noise.compute(x, y, z).to_bits());
            (want != got).then(|| {
                format!(
                    "{}\n    game:     {want:016x}\n    spyglass: {got:016x}",
                    case.label()
                )
            })
        }
        other => panic!("{}: unknown noise test {other}", case.label()),
    }
}

#[test]
#[ignore = "needs the oracle's probe files and a bundle: cargo xtask oracle probe"]
fn every_noise_sample_matches_the_game() {
    let path = probe_file("noise.json");
    let file = ProbeFile::read(&path).unwrap();
    let bundle = spyglass_oracle::bundle_dir();
    assert!(
        bundle.join("bundle.json").is_file(),
        "no bundle at {}; run spyglass-extract {}",
        bundle.display(),
        file.version
    );

    let mut perlins: HashMap<i64, PerlinNoise> = HashMap::new();
    let mut normals: HashMap<(String, i64, String), Normal> = HashMap::new();
    let mut improveds = HashMap::new();
    let mut sets = HashMap::new();
    let mut simplexes = HashMap::new();
    let mut blendeds = HashMap::new();
    let mut failures = Vec::new();
    for case in &file.cases {
        let seed = case.seed.expect("noise cases have a seed");
        let failure = match case.test.as_str() {
            "perlin_parity" | "perlin" => {
                let noise = perlins
                    .entry(seed)
                    .or_insert_with(|| PerlinNoise::new(&mut XoroshiroRandomSource::new(seed)));
                if case.test == "perlin_parity" {
                    check_parity(
                        &case.label(),
                        parity_groups(case.text())[0],
                        perlin_description(noise),
                    )
                } else {
                    match case.method.as_deref() {
                        Some("get_3d") => sample(case, |p| f64::from(noise.get(p[0], p[1], p[2]))),
                        Some("get_2d") => sample(case, |p| f64::from(noise.get_2d(p[0], p[1]))),
                        other => panic!("{}: unknown method {other:?}", case.label()),
                    }
                }
            }
            // The climate noises as a world makes them (kind absent in 26.3, "create" in 26.2),
            // and in 26.2 the Nether's biome noises (kind "legacy_nether_biome"). The bundle's
            // form decides the family.
            "normal_noise" | "normal_noise_parity" => {
                let id = case.name.clone().expect("a noise id");
                let kind = kind(case);
                let normal = normals
                    .entry((kind.clone(), seed, id.clone()))
                    .or_insert_with(|| match (parameters(&bundle, &id), kind.as_str()) {
                        // RandomState's factory for a Xoroshiro world, then the noise's own
                        // source, as Noises.instantiate does.
                        (NormalNoiseParameters::V26_3(p), "") => {
                            let factory = XoroshiroRandomSource::new(seed).fork_positional();
                            Normal::V26_3(
                                NormalNoise::from_parameters(&p)
                                    .unwrap()
                                    .create(&mut factory.from_hash_of(&id)),
                            )
                        }
                        (NormalNoiseParameters::V1_21(p), "create") => {
                            let factory = XoroshiroRandomSource::new(seed).fork_positional();
                            Normal::V1_21(v1_21::NormalNoise::create(
                                &mut factory.from_hash_of(&id),
                                &p,
                            ))
                        }
                        // LegacyRandomSource(seed + 0) for temperature, seed + 1 for vegetation.
                        (NormalNoiseParameters::V1_21(p), "legacy_nether_biome") => {
                            let offset = match id.as_str() {
                                "minecraft:nether/temperature" => 0,
                                "minecraft:nether/vegetation" => 1,
                                other => panic!("{other} isn't a Nether biome noise"),
                            };
                            let mut random = LegacyRandomSource::new(seed.wrapping_add(offset));
                            Normal::V1_21(
                                v1_21::NormalNoise::create_legacy_nether_biome(&mut random, &p)
                                    .unwrap(),
                            )
                        }
                        (_, kind) => panic!("{}: unknown kind {kind:?}", case.label()),
                    });
                match (normal, case.test.as_str()) {
                    (Normal::V26_3(stack), "normal_noise") => stack_sample(case, stack),
                    (Normal::V26_3(stack), _) => {
                        // The text lists the first stack's octaves, then the second's; the
                        // stack interleaves them.
                        let layers = stack.layers();
                        let ordered = layers
                            .iter()
                            .step_by(2)
                            .chain(layers.iter().skip(1).step_by(2))
                            .map(|l| perlin_description(&l.noise))
                            .collect();
                        check_all_parity(case, ordered)
                    }
                    (Normal::V1_21(noise), "normal_noise") => match case.method.as_deref() {
                        Some("get_3d") => sample(case, |p| noise.get_value(p[0], p[1], p[2])),
                        other => panic!("{}: unknown method {other:?}", case.label()),
                    },
                    (Normal::V1_21(noise), _) => {
                        // The text lists the first set's levels, lowest frequency first, with
                        // `null` for empty ones, then the second's.
                        let ordered = [noise.first(), noise.second()]
                            .into_iter()
                            .flat_map(|set| set.levels().iter().flatten())
                            .map(improved_description)
                            .collect();
                        check_all_parity(case, ordered)
                    }
                }
            }
            _ => v1_21_case(
                case,
                seed,
                &mut improveds,
                &mut sets,
                &mut simplexes,
                &mut blendeds,
            ),
        };
        failures.extend(failure);
    }
    assert!(
        failures.is_empty(),
        "{}: {} of {} cases differ:\n{}",
        path.display(),
        failures.len(),
        file.cases.len(),
        failures[..failures.len().min(20)].join("\n")
    );
    println!("{}: all {} cases match", path.display(), file.cases.len());
}
