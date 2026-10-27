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

/// Why a normal noise couldn't be built. The ranges are the game codec's, so parameters loaded
/// from game data never fail them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum NoiseError {
    /// `base_octave` outside -32..=32.
    #[error("{}", self.reason())]
    BaseOctave,
    /// `octave_count` outside 1..=32.
    #[error("{}", self.reason())]
    OctaveCount,
    /// `amplitude_modifiers` neither empty nor one per octave (`Parameters.validate`).
    #[error("{}", self.reason())]
    AmplitudeModifiers,
    /// The legacy Nether biome construction made a different number of octaves than there are
    /// non-zero modifiers (`LegacyFbmInitializer`'s `IllegalStateException`).
    #[error("{}", self.reason())]
    LegacyOctaves,
    /// The legacy Nether biome construction with an octave above frequency 1
    /// (`LegacyFbmInitializer`'s "Positive octaves are temporarily disabled").
    #[error("{}", self.reason())]
    PositiveOctaves,
    /// 1.21-family parameters (`firstOctave` and `amplitudes`, as 26.2 has them) given to 26.3's
    /// normal noise, which would sample them wrong.
    #[error("{}", self.reason())]
    Family,
}

impl NoiseError {
    /// The message, as a static string, for errors that carry one.
    pub fn reason(self) -> &'static str {
        match self {
            NoiseError::BaseOctave => "base_octave is outside -32..=32",
            NoiseError::OctaveCount => "octave_count is outside 1..=32",
            NoiseError::AmplitudeModifiers => {
                "amplitude_modifiers must be empty or have octave_count entries"
            }
            NoiseError::LegacyOctaves => {
                "the legacy Nether biome noise made the wrong number of octaves"
            }
            NoiseError::PositiveOctaves => {
                "the legacy Nether biome noise can't have octaves above frequency 1"
            }
            NoiseError::Family => {
                "this is a 26.2 (1.21-family) noise, with firstOctave and amplitudes; 26.3's \
                 noise can't sample it, and the 1.21-family density evaluator must"
            }
        }
    }
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

/// A `worldgen/noise/*.json` entry, in the form its version's data has (ADR 0026). Each form
/// belongs to one noise family, and only that family's noise samples it.
#[derive(Debug, Clone, PartialEq)]
pub enum NormalNoiseParameters {
    /// `{"base_octave", "octave_count", …}`, from 26.3 on (26.3, 26.4): [`NormalNoise`].
    V26_3(Parameters),
    /// `{"firstOctave", "amplitudes"}`, the 1.21 family (26.2):
    /// [`v1_21::NormalNoise`](crate::v1_21::NormalNoise).
    V1_21(crate::v1_21::NoiseParameters),
}

impl From<Parameters> for NormalNoiseParameters {
    fn from(p: Parameters) -> Self {
        NormalNoiseParameters::V26_3(p)
    }
}

impl From<crate::v1_21::NoiseParameters> for NormalNoiseParameters {
    fn from(p: crate::v1_21::NoiseParameters) -> Self {
        NormalNoiseParameters::V1_21(p)
    }
}

/// `NormalNoise.Parameters` (26.3 on), as a `worldgen/noise/*.json` file gives them. Parsing
/// JSON happens above this crate.
#[derive(Debug, Clone, PartialEq)]
pub struct Parameters {
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

impl Parameters {
    /// Parameters with the codec's defaults for everything but `base_octave`.
    pub fn new(base_octave: i32) -> Self {
        Parameters {
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
    /// The octaves' `|amplitude|`, summed as the game sums them (and adjusted for legacy
    /// normalization).
    amplitude_sum: f64,
    /// The parameters the legacy Nether biome construction reads.
    base_octave: i32,
    base_amplitude: f64,
    /// One modifier per octave: the parameters', or all 1.
    modifiers: Vec<f64>,
}

impl NormalNoise {
    /// `NormalNoise(Parameters)`.
    ///
    /// # Errors
    ///
    /// [`NoiseError::Family`] for 1.21-family parameters, which this noise can't sample; the
    /// others for parameters outside the codec's ranges.
    pub fn new(parameters: &NormalNoiseParameters) -> Result<Self, NoiseError> {
        match parameters {
            NormalNoiseParameters::V26_3(p) => Self::from_parameters(p),
            NormalNoiseParameters::V1_21(_) => Err(NoiseError::Family),
        }
    }

    /// `NormalNoise(Parameters)`, for 26.3-form parameters.
    ///
    /// # Errors
    ///
    /// For parameters outside the codec's ranges.
    pub fn from_parameters(parameters: &Parameters) -> Result<Self, NoiseError> {
        if !(-32..=32).contains(&parameters.base_octave) {
            return Err(NoiseError::BaseOctave);
        }
        if !(1..=32).contains(&parameters.octave_count) {
            return Err(NoiseError::OctaveCount);
        }
        let modifiers = parameters.amplitude_modifiers.len();
        if modifiers != 0 && modifiers != parameters.octave_count as usize {
            return Err(NoiseError::AmplitudeModifiers);
        }
        let octaves = build_octaves(parameters);
        // DoubleStream.sum(), which is compensated, over absAmplitude.
        let mut sum = java_double_stream_sum(octaves.iter().map(|o| o.amplitude.abs()));
        let mut factor = normalization_factor(sum, &octaves);
        let modifiers = if parameters.amplitude_modifiers.is_empty() {
            vec![1.0; parameters.octave_count as usize]
        } else {
            parameters.amplitude_modifiers.clone()
        };
        if parameters.normalize == Normalization::Legacy && factor != 0.0 {
            let parity = parity_normalization_factor(parameters.base_amplitude, &modifiers);
            // dload sum; dload parity; dload factor; ddiv; dmul.
            sum *= parity / factor;
            factor = parity;
        }
        Ok(NormalNoise {
            normalization_factor: factor,
            octaves,
            amplitude_sum: sum,
            base_octave: parameters.base_octave,
            base_amplitude: parameters.base_amplitude,
            modifiers,
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

    /// The bound of `range()`: the noise's range is `±` this, which is
    /// `(float)(sum · TARGET_DEVIATION · 6.0)` with `sum` the octaves' summed `|amplitude|`
    /// before normalization. Density functions use it to compile `min` and `max`.
    pub fn range_bound(&self) -> f32 {
        (self.amplitude_sum * TARGET_DEVIATION * 6.0) as f32
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

    /// `createForLegacyNetherBiome(RandomSource)`: two legacy stacks (`LegacyFbmInitializer`) from
    /// `random` in turn, then one stack of the first's layers at their frequencies and the
    /// second's at `INPUT_FACTOR` times theirs, every amplitude times
    /// `(float)(normalizationFactor · base_amplitude)` in `f32`. Not interleaved.
    pub fn create_for_legacy_nether_biome<R: RandomSource>(
        &self,
        random: &mut R,
    ) -> Result<NoiseStack, NoiseError> {
        let first = legacy_fbm(random, self.base_octave, &self.modifiers)?;
        let second = legacy_fbm(random, self.base_octave, &self.modifiers)?;
        let amplitude = (self.normalization_factor * self.base_amplitude) as f32;
        let scaled = |layers: Vec<Layer>, factor: f64| {
            layers.into_iter().map(move |l| Layer {
                frequency: l.frequency * factor,
                amplitude: l.amplitude * amplitude,
                noise: l.noise,
            })
        };
        Ok(NoiseStack::new(
            scaled(first, 1.0)
                .chain(scaled(second, INPUT_FACTOR))
                .collect(),
        ))
    }
}

/// `LegacyFbmInitializer.createForLegacyNetherBiome`: the layers of one legacy stack, lowest
/// frequency first. The octave at frequency 1 is always built first, kept only if it's in the
/// stack; then octaves from the one below it down to the first, each built if its modifier
/// isn't 0, else skipped by 262 draws.
fn legacy_fbm<R: RandomSource>(
    random: &mut R,
    first_octave: i32,
    modifiers: &[f64],
) -> Result<Vec<Layer>, NoiseError> {
    let n = modifiers.len() as i32;
    let zero = first_octave.wrapping_neg();
    let mut levels: Vec<Option<PerlinNoise>> = vec![None; modifiers.len()];
    let at_zero = PerlinNoise::new(random);
    if (0..n).contains(&zero) && modifiers[zero as usize] != 0.0 {
        levels[zero as usize] = Some(at_zero);
    }
    for i in (0..zero).rev() {
        if i < n && modifiers[i as usize] != 0.0 {
            levels[i as usize] = Some(PerlinNoise::new(random));
        } else {
            random.consume_count(262);
        }
    }
    let made = levels.iter().filter(|l| l.is_some()).count();
    if made != modifiers.iter().filter(|&&m| m != 0.0).count() {
        return Err(NoiseError::LegacyOctaves);
    }
    if zero < n - 1 {
        return Err(NoiseError::PositiveOctaves);
    }
    // Math.pow(2, -zero) and pow(2, n - 1) / (pow(2, n) - 1), exact for these integers.
    let mut frequency = exp2i(zero.wrapping_neg());
    let mut value = exp2i(n - 1) / (exp2i(n) - 1.0);
    let mut layers = Vec::new();
    for (level, &modifier) in levels.into_iter().zip(modifiers) {
        if let Some(noise) = level {
            layers.push(Layer {
                noise,
                frequency,
                amplitude: (value * modifier) as f32,
            });
        }
        frequency *= 2.0;
        value /= 2.0;
    }
    Ok(layers)
}

/// `NormalNoise.computeParityNormalizationFactor`: from the first and last octaves with a
/// non-zero modifier.
fn parity_normalization_factor(base_amplitude: f64, modifiers: &[f64]) -> f64 {
    let (mut lo, mut hi) = (i32::MAX, i32::MIN);
    for (i, &m) in modifiers.iter().enumerate() {
        if m != 0.0 {
            lo = lo.min(i as i32);
            hi = hi.max(i as i32);
        }
    }
    // parityExpectedDeviation(hi - lo): 0.1 * (1.0 + 1.0 / (double)(n + 1)), int arithmetic.
    let n = hi.wrapping_sub(lo);
    let deviation = 0.1 * (1.0 + 1.0 / f64::from(n.wrapping_add(1)));
    base_amplitude * 0.5 * TARGET_DEVIATION / deviation
}

/// `NormalNoise.buildOctaves`.
fn build_octaves(p: &Parameters) -> Vec<OctaveInfo> {
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
        let mut p = Parameters::new(-10);
        p.octave_count = 6;
        p.amplitude_modifiers = vec![1.5, 0.0, 1.0, 0.0, 0.0, 0.0];
        let noise = NormalNoise::from_parameters(&p).unwrap();
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
    fn legacy_normalization_uses_the_parity_factor() {
        // Octaves 1 and 2 of three non-zero: lo 1, hi 2, so the deviation is
        // 0.1 · (1 + 1/3), and the factor 2 · 0.5 · (1/3) / that.
        let mut p = Parameters::new(-4);
        p.octave_count = 3;
        p.base_amplitude = 2.0;
        p.amplitude_modifiers = vec![0.0, 1.0, 0.5];
        p.normalize = Normalization::Legacy;
        let legacy = NormalNoise::from_parameters(&p).unwrap();
        let parity = 2.0 * 0.5 * TARGET_DEVIATION / (0.1 * (1.0 + 1.0 / 2.0));
        assert_eq!(legacy.normalization_factor(), parity);
        p.normalize = Normalization::Enabled;
        let enabled = NormalNoise::from_parameters(&p).unwrap();
        let sum = enabled.amplitude_sum * (parity / enabled.normalization_factor());
        assert_eq!(legacy.amplitude_sum, sum);
        assert_eq!(legacy.octaves(), enabled.octaves());
    }

    #[test]
    fn the_legacy_nether_biome_stack() {
        use spyglass_rng::LegacyRandomSource;
        // 26.3's nether/temperature: base octave -7, two octaves.
        let mut p = Parameters::new(-7);
        p.octave_count = 2;
        p.base_amplitude = 0.949_473_105_442_798_1;
        let noise = NormalNoise::from_parameters(&p).unwrap();
        let stack = noise
            .create_for_legacy_nether_biome(&mut LegacyRandomSource::new(5))
            .unwrap();
        let layers = stack.layers();
        assert_eq!(layers.len(), 4);
        let amp = (noise.normalization_factor() * p.base_amplitude) as f32;
        assert_eq!(amp, 1.111_111_2);
        assert_eq!(layers[0].frequency, 1.0 / 128.0);
        assert_eq!(layers[1].frequency, 1.0 / 64.0);
        assert_eq!(layers[2].frequency, 1.0 / 128.0 * INPUT_FACTOR);
        assert_eq!(layers[0].amplitude, (2.0_f64 / 3.0) as f32 * amp);
        assert_eq!(layers[1].amplitude, (1.0_f64 / 3.0) as f32 * amp);
        // Draws: the zero octave built and dropped, five skips of 262, then octaves 1 and 0;
        // the same again for the second stack.
        let mut r = LegacyRandomSource::new(5);
        let _ = PerlinNoise::new(&mut r);
        r.consume_count(5 * 262);
        let one = PerlinNoise::new(&mut r);
        let zero = PerlinNoise::new(&mut r);
        assert_eq!(layers[0].noise.offset(), zero.offset());
        assert_eq!(layers[1].noise.offset(), one.offset());
        let _ = PerlinNoise::new(&mut r);
        r.consume_count(5 * 262);
        let one = PerlinNoise::new(&mut r);
        assert_eq!(layers[3].noise.offset(), one.offset());
    }

    #[test]
    fn legacy_nether_biome_layouts_it_cant_build_are_errors() {
        use spyglass_rng::LegacyRandomSource;
        let error = |base_octave, modifiers: Vec<f64>| {
            let mut p = Parameters::new(base_octave);
            p.octave_count = modifiers.len() as i32;
            p.amplitude_modifiers = modifiers;
            NormalNoise::from_parameters(&p)
                .unwrap()
                .create_for_legacy_nether_biome(&mut LegacyRandomSource::new(0))
                .unwrap_err()
        };
        // The octave count is checked first: octave 2 (frequency 2) is never built.
        assert_eq!(error(-1, vec![1.0, 1.0, 1.0]), NoiseError::LegacyOctaves);
        assert_eq!(error(0, vec![1.0, 0.0]), NoiseError::PositiveOctaves);
    }

    #[test]
    fn parameters_outside_the_codec_are_errors() {
        let error = |f: fn(&mut Parameters)| {
            let mut p = Parameters::new(0);
            f(&mut p);
            NormalNoise::from_parameters(&p).unwrap_err()
        };
        assert_eq!(error(|p| p.base_octave = -2000), NoiseError::BaseOctave);
        assert_eq!(error(|p| p.octave_count = 0), NoiseError::OctaveCount);
        assert_eq!(error(|p| p.octave_count = 5000), NoiseError::OctaveCount);
        assert_eq!(
            error(|p| {
                p.octave_count = 3;
                p.amplitude_modifiers = vec![1.0];
            }),
            NoiseError::AmplitudeModifiers
        );
    }

    #[test]
    fn each_family_samples_only_its_own_parameters() {
        let p = Parameters::new(-3);
        assert_eq!(
            NormalNoise::new(&p.clone().into()),
            NormalNoise::from_parameters(&p)
        );
        let v1_21 = crate::v1_21::NoiseParameters::new(-3, vec![1.0]);
        assert_eq!(
            NormalNoise::new(&v1_21.into()).unwrap_err(),
            NoiseError::Family
        );
    }
}
