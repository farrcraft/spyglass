//! Every noise sample in the oracle's probe files, reproduced bit for bit, with the game's parity
//! descriptions as a structural check.
//!
//! Needs `cargo xtask oracle probe` and a bundle for the same version
//! (`spyglass-extract <version>`). Run with `cargo test -p spyglass-noise -- --ignored`.

use std::collections::HashMap;
use std::path::Path;

use serde_json::Value;
use spyglass_noise::{NoiseStack, NormalNoise, NormalNoiseParameters, PerlinNoise};
use spyglass_oracle::probe::{Case, ProbeFile, probe_files};
use spyglass_rng::{PositionalRandomFactory, RandomSource, XoroshiroRandomSource};

/// A noise's parameters from `bundles/<version>/data/<ns>/worldgen/noise/<path>.json`. Only
/// what 26.3's climate noises use; the real parser belongs to the density layer.
fn parameters(bundle: &Path, id: &str) -> NormalNoiseParameters {
    let (namespace, path) = id.split_once(':').expect("a namespaced id");
    let file = bundle.join(format!("data/{namespace}/worldgen/noise/{path}.json"));
    let json: Value = serde_json::from_slice(
        &std::fs::read(&file).unwrap_or_else(|e| panic!("{}: {e}", file.display())),
    )
    .unwrap();
    let mut p = NormalNoiseParameters::new(json["base_octave"].as_i64().unwrap() as i32);
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
        p.amplitude_modifiers = m
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();
    }
    p
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

/// The same description for one of spyglass's Perlin noises. Offsets are compared to within the
/// game's 3-decimal formatting of the `float` value.
fn check_parity(label: &str, game: ([f64; 3], [i64; 2]), noise: &PerlinNoise) -> Option<String> {
    let offsets = noise.offset().map(|o| f64::from(o as f32));
    let perms = [i64::from(noise.perm(0)), i64::from(noise.perm(255))];
    let close = game
        .0
        .iter()
        .zip(offsets)
        .all(|(g, o)| (g - o).abs() <= 0.000_51);
    (!close || game.1 != perms).then(|| {
        format!(
            "{label}: parity differs\n    game:     offsets {:?}, perms {:?}\n    spyglass: offsets {offsets:?}, perms {perms:?}",
            game.0, game.1
        )
    })
}

fn sample(case: &Case, get: impl Fn(&[f64]) -> f32) -> Option<String> {
    let point = case.point_f64();
    let want = case.f32_bits()[0];
    let got = get(&point).to_bits();
    (want != got).then(|| {
        format!(
            "{}\n    game:     {:08x} ({})\n    spyglass: {got:08x} ({})",
            case.label(),
            want,
            f32::from_bits(want),
            f32::from_bits(got)
        )
    })
}

fn stack_sample(case: &Case, stack: &NoiseStack) -> Option<String> {
    match case.method.as_deref() {
        Some("get_3d") => sample(case, |p| stack.get(p[0], p[1], p[2])),
        Some("get_2d") => sample(case, |p| stack.get_2d(p[0], p[1])),
        other => panic!("{}: unknown method {other:?}", case.label()),
    }
}

#[test]
#[ignore = "needs the oracle's probe files and a bundle: cargo xtask oracle probe"]
fn every_noise_sample_matches_the_game() {
    let root = spyglass_oracle::default_dir();
    let files = probe_files(&root, "noise.json");
    assert!(
        !files.is_empty(),
        "no probe files; run cargo xtask oracle probe"
    );
    for path in files {
        let file = ProbeFile::read(&path).unwrap();
        let bundle = root.join("../bundles").join(&file.version);
        assert!(
            bundle.join("bundle.json").is_file(),
            "no bundle at {}; run spyglass-extract {}",
            bundle.display(),
            file.version
        );

        let mut perlins: HashMap<i64, PerlinNoise> = HashMap::new();
        let mut normals: HashMap<(i64, String), (NoiseStack, NormalNoise)> = HashMap::new();
        let mut failures = Vec::new();
        for case in &file.cases {
            let seed = case.seed.expect("noise cases have a seed");
            let perlin = || PerlinNoise::new(&mut XoroshiroRandomSource::new(seed));
            let failure = match case.test.as_str() {
                "perlin_parity" => {
                    let groups = parity_groups(case.text());
                    check_parity(
                        &case.label(),
                        groups[0],
                        perlins.entry(seed).or_insert_with(perlin),
                    )
                }
                "perlin" => {
                    let noise = perlins.entry(seed).or_insert_with(perlin);
                    match case.method.as_deref() {
                        Some("get_3d") => sample(case, |p| noise.get(p[0], p[1], p[2])),
                        Some("get_2d") => sample(case, |p| noise.get_2d(p[0], p[1])),
                        other => panic!("{}: unknown method {other:?}", case.label()),
                    }
                }
                "normal_noise" | "normal_noise_parity" => {
                    let id = case.name.clone().expect("a noise id");
                    let (stack, _) = normals.entry((seed, id.clone())).or_insert_with(|| {
                        let normal = NormalNoise::new(&parameters(&bundle, &id)).unwrap();
                        // RandomState's factory for a Xoroshiro world, then the noise's own
                        // source, as Noises.instantiate does.
                        let factory = XoroshiroRandomSource::new(seed).fork_positional();
                        let stack = normal.create(&mut factory.from_hash_of(&id));
                        (stack, normal)
                    });
                    if case.test == "normal_noise" {
                        stack_sample(case, stack)
                    } else {
                        // The text lists the first stack's octaves, then the second's; the
                        // stack interleaves them.
                        let groups = parity_groups(case.text());
                        let layers = stack.layers();
                        let ordered = layers
                            .iter()
                            .step_by(2)
                            .chain(layers.iter().skip(1).step_by(2));
                        if groups.len() != layers.len() {
                            Some(format!(
                                "{}: {} octaves in the game, {} in spyglass",
                                case.label(),
                                groups.len(),
                                layers.len()
                            ))
                        } else {
                            groups.iter().zip(ordered).find_map(|(g, layer)| {
                                check_parity(&case.label(), *g, &layer.noise)
                            })
                        }
                    }
                }
                other => panic!("{}: unknown noise test {other}", case.label()),
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
}
