//! `NormalNoise`: octaves of Perlin noise in two stacks, normalized toward a target deviation.

use spyglass_rng::{PositionalRandomFactory, RandomSource};

use crate::mth::exp2i;
use crate::perlin::PerlinNoise;
use crate::stack::{Layer, NoiseStack};

/// `NormalNoise.INPUT_FACTOR`: the second stack's frequency scale.
const INPUT_FACTOR: f64 = 1.018_126_888_217_522_7;
/// `NormalNoise.TARGET_DEVIATION`: 1/3.
const TARGET_DEVIATION: f64 = 0.333_333_333_333_333_3;
/// The per-octave deviation factor in `NormalNoise.estimateDeviation`.
const OCTAVE_DEVIATION: f64 = 0.270_224_783_124_521_1;

/// Why a normal noise couldn't be built.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NoiseError {
    /// `normalize: "legacy"`, which no 26.3 noise uses, isn't ported yet.
    #[error("legacy noise normalization isn't supported yet")]
    LegacyNormalization,
}

/// `NormalNoise.Normalization`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Normalization {
    /// `DISABLED`.
    Disabled,
    /// `ENABLED`, the codec's default when `normalize` is absent.
    #[default]
    Enabled,
    /// `LEGACY`.
    Legacy,
}

/// `NormalNoise.Parameters`, as a `worldgen/noise/*.json` file gives them. Parsing JSON happens
/// above this crate.
#[derive(Debug, Clone, PartialEq)]
pub struct NormalNoiseParameters {
    /// `base_amplitude`, default 1.0.
    pub base_amplitude: f64,
    /// `base_octave`: the first octave's frequency is `2^base_octave`.
    pub base_octave: i32,
    /// `octave_count`, default 1.
    pub octave_count: i32,
    /// `normalize`, default [`Normalization::Enabled`].
    pub normalize: Normalization,
    /// `amplitude_modifiers` per octave; empty (the default) means 1 for every octave, and an
    /// octave with modifier 0 is left out.
    pub amplitude_modifiers: Vec<f64>,
}

impl NormalNoiseParameters {
    /// Parameters with the codec's defaults for everything but `base_octave`.
    pub fn new(base_octave: i32) -> Self {
        NormalNoiseParameters {
            base_amplitude: 1.0,
            base_octave,
            octave_count: 1,
            normalize: Normalization::Enabled,
            amplitude_modifiers: Vec::new(),
        }
    }
}

/// `NormalNoise.OctaveInfo`: one octave that's present (non-zero modifier).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OctaveInfo {
    /// `base_octave + i`; also names the octave's random source (`"octave_<index>"`).
    pub octave_index: i32,
    /// `2^octave_index`.
    pub frequency: f64,
    /// Before normalization.
    pub amplitude: f64,
}

/// `NormalNoise`: the octave layout for a set of parameters. [`create`](Self::create) makes the
/// seeded noise.
#[derive(Debug, Clone, PartialEq)]
pub struct NormalNoise {
    octaves: Vec<OctaveInfo>,
    normalization_factor: f64,
}

impl NormalNoise {
    /// `NormalNoise(Parameters)`.
    pub fn new(parameters: &NormalNoiseParameters) -> Result<Self, NoiseError> {
        if parameters.normalize == Normalization::Legacy {
            return Err(NoiseError::LegacyNormalization);
        }
        let octaves = build_octaves(parameters);
        // DoubleStream.sum(), which is compensated, over absAmplitude.
        let sum = java_double_stream_sum(octaves.iter().map(|o| o.amplitude.abs()));
        Ok(NormalNoise {
            normalization_factor: normalization_factor(sum, &octaves),
            octaves,
        })
    }

    /// The octaves that are present, in order.
    pub fn octaves(&self) -> &[OctaveInfo] {
        &self.octaves
    }

    /// `normalizationFactor`.
    pub fn normalization_factor(&self) -> f64 {
        self.normalization_factor
    }

    /// `create(RandomSource)`: two positional factories forked from `random`, and for each
    /// octave a Perlin noise from each, seeded by `"octave_<index>"`, the second at
    /// `INPUT_FACTOR` (1.0181268882175227) times the frequency. Layers alternate first, second.
    pub fn create<R: RandomSource>(&self, random: &mut R) -> NoiseStack {
        let first = random.fork_positional();
        let second = random.fork_positional();
        let mut layers = Vec::with_capacity(2 * self.octaves.len());
        for octave in &self.octaves {
            let seed = format!("octave_{}", octave.octave_index);
            let a = PerlinNoise::new(&mut first.from_hash_of(&seed));
            let b = PerlinNoise::new(&mut second.from_hash_of(&seed));
            let amplitude = (self.normalization_factor * octave.amplitude) as f32;
            layers.push(Layer {
                noise: a,
                frequency: octave.frequency,
                amplitude,
            });
            layers.push(Layer {
                noise: b,
                frequency: octave.frequency * INPUT_FACTOR,
                amplitude,
            });
        }
        NoiseStack::new(layers)
    }
}

/// `NormalNoise.buildOctaves`.
fn build_octaves(p: &NormalNoiseParameters) -> Vec<OctaveInfo> {
    let mut frequency = exp2i(p.base_octave);
    let mut amplitude = p.base_amplitude;
    if p.normalize != Normalization::Disabled {
        // pow(0.5, -(count - 1)) / (pow(0.5, -count) - 1).
        amplitude *= exp2i(p.octave_count - 1) / (exp2i(p.octave_count) - 1.0);
    }
    let mut octaves = Vec::new();
    for i in 0..p.octave_count {
        // getAmplitudeModifier: 1 for every octave when the list is empty.
        let modifier = if p.amplitude_modifiers.is_empty() {
            1.0
        } else {
            p.amplitude_modifiers[i as usize]
        };
        if modifier != 0.0 {
            octaves.push(OctaveInfo {
                octave_index: p.base_octave + i,
                frequency,
                amplitude: amplitude * modifier,
            });
        }
        frequency *= 2.0;
        amplitude *= 0.5;
    }
    octaves
}

/// `NormalNoise.computeNormalizationFactor`, with `estimateDeviation` inlined.
fn normalization_factor(sum: f64, octaves: &[OctaveInfo]) -> f64 {
    let deviation = octaves
        .iter()
        .fold(0.0, |acc: f64, o| {
            let d = OCTAVE_DEVIATION * o.amplitude.abs();
            acc + d * d
        })
        .sqrt();
    if deviation == 0.0 {
        return 0.0;
    }
    sum * TARGET_DEVIATION / (deviation * std::f64::consts::SQRT_2)
}

/// `DoubleStream.sum()` on a sequential stream: Kahan compensated summation
/// (`Collectors.sumWithCompensation` and `computeFinalSum`), so the result can differ from a
/// plain loop's in the last bits.
fn java_double_stream_sum(values: impl IntoIterator<Item = f64>) -> f64 {
    let (mut sum, mut compensation, mut simple) = (0.0_f64, 0.0_f64, 0.0_f64);
    for value in values {
        let tmp = value - compensation;
        let velvel = sum + tmp;
        compensation = (velvel - sum) - tmp;
        sum = velvel;
        simple += value;
    }
    let total = sum - compensation;
    if total.is_nan() && simple.is_infinite() {
        simple
    } else {
        total
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqrt_2_matches_math_sqrt() {
        // computeNormalizationFactor calls Math.sqrt(2.0), which is correctly rounded.
        assert_eq!(std::f64::consts::SQRT_2, 2.0_f64.sqrt());
    }

    #[test]
    fn octaves_skip_zero_modifiers() {
        let mut p = NormalNoiseParameters::new(-10);
        p.octave_count = 6;
        p.amplitude_modifiers = vec![1.5, 0.0, 1.0, 0.0, 0.0, 0.0];
        let noise = NormalNoise::new(&p).unwrap();
        let indices: Vec<_> = noise.octaves().iter().map(|o| o.octave_index).collect();
        assert_eq!(indices, [-10, -8]);
        assert_eq!(noise.octaves()[0].frequency, 1.0 / 1024.0);
        assert_eq!(noise.octaves()[1].frequency, 1.0 / 256.0);
    }

    #[test]
    fn compensated_sum_differs_from_a_plain_loop() {
        // Ten 0.1s: a plain loop accumulates rounding error, Kahan summation doesn't.
        let values = [0.1; 10];
        let plain = values.iter().fold(0.0, |acc, v| acc + v);
        assert_eq!(plain, 0.999_999_999_999_999_9);
        assert_eq!(java_double_stream_sum(values), 1.0);
    }

    #[test]
    fn legacy_normalization_is_refused() {
        let mut p = NormalNoiseParameters::new(0);
        p.normalize = Normalization::Legacy;
        assert_eq!(NormalNoise::new(&p), Err(NoiseError::LegacyNormalization));
    }
}
