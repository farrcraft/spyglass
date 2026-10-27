//! 26.2's noise parameters against 26.3's, for every noise both bundles have: the same octaves,
//! and a `base_amplitude` that makes 26.3's normalized layer weights 26.2's.
//!
//! 26.2 weights level `i` by `valueFactor · amplitudes[i] · valF0 / 2^i`, with `valueFactor` from
//! the amplitudes' span (docs/specs/noise.md, "26.2 normal noise"). 26.3 weights each layer by
//! `normalizationFactor · octave amplitude`, linear in `base_amplitude`, so 26.3's stored
//! `base_amplitude` is the one that makes the two agree (up to rounding).
//!
//! Needs both bundles (`bundles/26.2` and `bundles/26.3`). Run with
//! `cargo test -p spyglass-noise --test versions -- --ignored`.

use std::path::{Path, PathBuf};

use serde_json::Value;
use spyglass_noise::mth::pow2;
use spyglass_noise::v1_21::NoiseParameters;
use spyglass_noise::{NormalNoise, Parameters};

fn bundle(version: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../bundles")
        .join(version)
}

/// Every `worldgen/noise` file under `dir`, as `(path relative to dir, json)`.
fn noises(dir: &Path) -> Vec<(String, Value)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "json") {
                let rel = path
                    .strip_prefix(dir)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                let json = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
                out.push((rel, json));
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn v1_21(json: &Value) -> NoiseParameters {
    NoiseParameters::new(
        json["firstOctave"].as_i64().unwrap() as i32,
        json["amplitudes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a.as_f64().unwrap())
            .collect(),
    )
}

fn v26_3(json: &Value) -> Parameters {
    let mut p = Parameters::new(json["base_octave"].as_i64().unwrap() as i32);
    if let Some(a) = json.get("base_amplitude") {
        p.base_amplitude = a.as_f64().unwrap();
    }
    if let Some(n) = json.get("octave_count") {
        p.octave_count = n.as_i64().unwrap() as i32;
    }
    assert!(json.get("normalize").is_none(), "normalize isn't handled");
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

/// 26.2's weight of each non-zero level, in order: `valueFactor · amplitude · valF0 / 2^i`.
fn weights_26_2(p: &NoiseParameters) -> Vec<f64> {
    let n = p.amplitudes.len() as i32;
    let mut value = pow2(n - 1) / (pow2(n) - 1.0);
    let mut out = Vec::new();
    for &a in &p.amplitudes {
        if a != 0.0 {
            out.push(p.value_factor() * a * value);
        }
        value /= 2.0;
    }
    out
}

#[test]
#[ignore = "needs bundles/26.2 and bundles/26.3: cargo run -p spyglass-extract"]
fn base_amplitude_carries_26_2_weights_into_26_3() {
    let (old, new) = (bundle("26.2"), bundle("26.3"));
    for b in [&old, &new] {
        assert!(
            b.join("bundle.json").is_file(),
            "no bundle at {}",
            b.display()
        );
    }
    let sub = "data/minecraft/worldgen/noise";
    let new_noises = noises(&new.join(sub));
    let mut failures = Vec::new();
    let mut compared = 0;
    let mut worst: f64 = 0.0;
    let (mut weights, mut f32_differ) = (0, 0);
    for (name, old_json) in noises(&old.join(sub)) {
        let Some((_, new_json)) = new_noises.iter().find(|(n, _)| *n == name) else {
            continue;
        };
        compared += 1;
        let p2 = v1_21(&old_json);
        let p3 = v26_3(new_json);
        // The same octaves: base_octave is firstOctave, one modifier per amplitude (or none,
        // meaning all 1).
        let modifiers = if p3.amplitude_modifiers.is_empty() {
            vec![1.0; p3.octave_count as usize]
        } else {
            p3.amplitude_modifiers.clone()
        };
        if p3.base_octave != p2.first_octave || modifiers != p2.amplitudes {
            failures.push(format!(
                "{name}: octaves differ: 26.2 {} {:?}, 26.3 {} {modifiers:?}",
                p2.first_octave, p2.amplitudes, p3.base_octave
            ));
            continue;
        }
        let noise = NormalNoise::from_parameters(&p3).unwrap();
        let w3: Vec<f64> = noise
            .octaves()
            .iter()
            .map(|o| noise.normalization_factor() * o.amplitude)
            .collect();
        let w2 = weights_26_2(&p2);
        assert_eq!(w2.len(), w3.len(), "{name}");
        for (i, (a, b)) in w2.iter().zip(&w3).enumerate() {
            let rel = (a - b).abs() / a.abs();
            worst = worst.max(rel);
            weights += 1;
            f32_differ += usize::from(*a as f32 != *b as f32);
            if rel > 1e-12 {
                failures.push(format!(
                    "{name}: octave {i}: 26.2 weight {a}, 26.3 weight {b} (relative {rel:e})"
                ));
            }
        }
    }
    assert!(compared > 0, "no noise in both bundles");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    // 26.3 narrows each weight to f32; 26.2 never does, so this is only for the record.
    println!(
        "{compared} noises agree; worst relative difference {worst:e}; \
         {f32_differ} of {weights} weights round to a different f32"
    );
}
