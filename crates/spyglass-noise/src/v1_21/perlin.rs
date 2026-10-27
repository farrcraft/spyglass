//! 26.2's `PerlinNoise`: an octave set of [`ImprovedNoise`]s, summed in `f64`.

use spyglass_rng::{PositionalRandomFactory, RandomSource};

use super::ImprovedNoise;
use crate::NoiseError;
use crate::mth;

/// `PerlinNoise.ROUND_OFF`: 2^25.
const ROUND_OFF: f64 = 33_554_432.0;

/// `PerlinNoise.wrap(double)`: `d − (double) lfloor(d / 2^25 + 0.5) · 2^25`, with no fast path.
///
/// It returns `d` wherever 26.3's `GradientNoise.wrap` fast path does. Far out, `lfloor`
/// saturates where 26.3 floors in `f64`, so the two differ once `|d|` passes about 2^88, and
/// at the infinities (26.2 keeps them, 26.3 gives NaN).
pub fn wrap(d: f64) -> f64 {
    d - mth::lfloor(d / ROUND_OFF + 0.5) as f64 * ROUND_OFF
}

/// `PerlinNoise`: `n` levels, level `i` the octave `first_octave + i` with weight
/// `amplitudes[i]`, lowest frequency first. A level is empty where its amplitude is zero
/// (`-0.0` included; NaN counts as non-zero).
#[derive(Debug, Clone)]
pub struct PerlinNoise {
    levels: Vec<Option<ImprovedNoise>>,
    first_octave: i32,
    amplitudes: Vec<f64>,
    /// `lowestFreqInputFactor`: `2^first_octave`.
    input_factor: f64,
    /// `lowestFreqValueFactor`: `2^(n − 1) / (2^n − 1)`.
    value_factor: f64,
}

impl PerlinNoise {
    /// `PerlinNoise.create(RandomSource, int, DoubleList)`, the new initialization:
    /// `random.forkPositional()`, then for each level with a non-zero amplitude an
    /// [`ImprovedNoise`] from `fromHashOf("octave_" + (first_octave + i))`. No octave is
    /// rejected.
    pub fn new<R: RandomSource>(random: &mut R, first_octave: i32, amplitudes: Vec<f64>) -> Self {
        let factory = random.fork_positional();
        let levels = amplitudes
            .iter()
            .enumerate()
            .map(|(i, &a)| {
                (a != 0.0).then(|| {
                    // iadd: wraps, as the game's int sum does.
                    let octave = first_octave.wrapping_add(i as i32);
                    ImprovedNoise::new(&mut factory.from_hash_of(&format!("octave_{octave}")))
                })
            })
            .collect();
        Self::with_levels(levels, first_octave, amplitudes)
    }

    /// The legacy initialization (`createLegacyForLegacyNetherBiome`, and through
    /// [`new_legacy_for_blended_noise`](Self::new_legacy_for_blended_noise)
    /// `createLegacyForBlendedNoise`): every level drawn from `random` itself. The octave at
    /// frequency 1 (level `−first_octave`) is always built first, kept only if it's a level with
    /// a non-zero amplitude; then levels from the one below it down to 0, each built if it
    /// exists with a non-zero amplitude, else skipped by 262 draws.
    ///
    /// # Errors
    ///
    /// [`NoiseError::LegacyOctaves`] if the number of levels built isn't the number of non-zero
    /// amplitudes (an octave above frequency 1 was asked for), checked first, then
    /// [`NoiseError::PositiveOctaves`] if the last level is above frequency 1.
    pub fn new_legacy<R: RandomSource>(
        random: &mut R,
        first_octave: i32,
        amplitudes: Vec<f64>,
    ) -> Result<Self, NoiseError> {
        let n = amplitudes.len() as i32;
        let zero = first_octave.wrapping_neg();
        let mut levels: Vec<Option<ImprovedNoise>> = vec![None; amplitudes.len()];
        let at_zero = ImprovedNoise::new(random);
        if (0..n).contains(&zero) && amplitudes[zero as usize] != 0.0 {
            levels[zero as usize] = Some(at_zero);
        }
        for i in (0..zero).rev() {
            if i < n && amplitudes[i as usize] != 0.0 {
                levels[i as usize] = Some(ImprovedNoise::new(random));
            } else {
                // skipOctave.
                random.consume_count(262);
            }
        }
        let made = levels.iter().filter(|l| l.is_some()).count();
        if made != amplitudes.iter().filter(|&&a| a != 0.0).count() {
            return Err(NoiseError::LegacyOctaves);
        }
        if zero < n - 1 {
            return Err(NoiseError::PositiveOctaves);
        }
        Ok(Self::with_levels(levels, first_octave, amplitudes))
    }

    /// `createLegacyForBlendedNoise(RandomSource, IntStream.rangeClosed(first, last))`: the
    /// legacy initialization with amplitude 1 at every octave from `first_octave` to
    /// `last_octave` (`makeAmplitudes`).
    ///
    /// # Errors
    ///
    /// As [`new_legacy`](Self::new_legacy), when `last_octave` is above 0;
    /// [`NoiseError::OctaveCount`] when the range is empty ("Need some octaves!").
    pub fn new_legacy_for_blended_noise<R: RandomSource>(
        random: &mut R,
        first_octave: i32,
        last_octave: i32,
    ) -> Result<Self, NoiseError> {
        if last_octave < first_octave {
            return Err(NoiseError::OctaveCount);
        }
        let n = (i64::from(last_octave) - i64::from(first_octave) + 1) as usize;
        Self::new_legacy(random, first_octave, vec![1.0; n])
    }

    /// The constructor's tail: the input and value factors from the level count.
    fn with_levels(
        levels: Vec<Option<ImprovedNoise>>,
        first_octave: i32,
        amplitudes: Vec<f64>,
    ) -> Self {
        let n = amplitudes.len() as i32;
        // Math.pow(2.0, (double) -zero) with zero = -firstOctave, both negations ineg, and
        // Math.pow(2.0, n - 1) / (Math.pow(2.0, n) - 1.0).
        let input_factor = mth::pow2(first_octave.wrapping_neg().wrapping_neg());
        let value_factor = mth::pow2(n.wrapping_sub(1)) / (mth::pow2(n) - 1.0);
        PerlinNoise {
            levels,
            first_octave,
            amplitudes,
            input_factor,
            value_factor,
        }
    }

    /// `firstOctave`.
    pub fn first_octave(&self) -> i32 {
        self.first_octave
    }

    /// `amplitudes`, one per level.
    pub fn amplitudes(&self) -> &[f64] {
        &self.amplitudes
    }

    /// `noiseLevels`, lowest frequency first, `None` where the amplitude is zero.
    pub fn levels(&self) -> &[Option<ImprovedNoise>] {
        &self.levels
    }

    /// `getOctaveNoise(int)`: level `n − 1 − i`, so 0 is the highest frequency. `None` for an
    /// empty level, or for `i` outside `0..n` (where the game throws).
    pub fn octave_noise(&self, i: usize) -> Option<&ImprovedNoise> {
        let index = self.levels.len().checked_sub(1)?.checked_sub(i)?;
        self.levels[index].as_ref()
    }

    /// `getValue(double, double, double)`: [`get_value_smeared`](Self::get_value_smeared) with
    /// no smear.
    pub fn get_value(&self, x: f64, y: f64, z: f64) -> f64 {
        self.get_value_smeared(x, y, z, 0.0, 0.0)
    }

    /// `getValue(double, double, double, double, double)`: for each level, from
    /// `inF = 2^first_octave` and `valF` = [`value_factor`](Self::value_factor),
    /// `sum = sum + (amplitude · noise(wrap(x·inF), wrap(y·inF), wrap(z·inF), y_scale·inF,
    /// y_max·inF)) · valF`; then `inF *= 2` and `valF /= 2`, empty levels included.
    pub fn get_value_smeared(&self, x: f64, y: f64, z: f64, y_scale: f64, y_max: f64) -> f64 {
        let mut sum = 0.0;
        let mut input = self.input_factor;
        let mut value = self.value_factor;
        for (level, &amplitude) in self.levels.iter().zip(&self.amplitudes) {
            if let Some(noise) = level {
                let v = noise.noise_smeared(
                    wrap(x * input),
                    wrap(y * input),
                    wrap(z * input),
                    y_scale * input,
                    y_max * input,
                );
                sum += amplitude * v * value;
            }
            input *= 2.0;
            value /= 2.0;
        }
        sum
    }

    /// `lowestFreqValueFactor`: `2^(n − 1) / (2^n − 1)`, with `n` the number of levels, empty
    /// ones included.
    pub fn value_factor(&self) -> f64 {
        self.value_factor
    }

    /// `maxValue()`: `edgeValue(2.0)`.
    pub fn max_value(&self) -> f64 {
        self.edge_value(2.0)
    }

    /// `maxBrokenValue(double)`: `edgeValue(y + 2.0)`.
    pub fn max_broken_value(&self, y: f64) -> f64 {
        self.edge_value(y + 2.0)
    }

    /// `edgeValue(double)`: the sum of `(amplitude · m) · valF` over the non-empty levels, `valF`
    /// halving per level from [`value_factor`](Self::value_factor).
    fn edge_value(&self, m: f64) -> f64 {
        let mut sum = 0.0;
        let mut value = self.value_factor;
        for (level, &amplitude) in self.levels.iter().zip(&self.amplitudes) {
            if level.is_some() {
                sum += amplitude * m * value;
            }
            value /= 2.0;
        }
        sum
    }
}

#[cfg(test)]
mod tests {
    use spyglass_rng::{LegacyRandomSource, XoroshiroRandomSource};

    use super::*;

    #[test]
    fn wrap_matches_26_3_near_the_origin() {
        for d in [
            0.0,
            -0.0,
            123.5,
            -16_777_216.0,
            16_777_215.999,
            3e7,
            ROUND_OFF + 1.5,
            -1e12,
            1e20,
        ] {
            assert_eq!(wrap(d).to_bits(), crate::perlin::wrap(d).to_bits(), "{d}");
        }
        assert!(wrap(f64::NAN).is_nan());
        // Where they part: lfloor saturates.
        assert_eq!(wrap(f64::INFINITY), f64::INFINITY);
        assert!(crate::perlin::wrap(f64::INFINITY).is_nan());
        assert_eq!(wrap(1e30), 1e30 - i64::MAX as f64 * ROUND_OFF);
    }

    #[test]
    fn new_initialization_seeds_octaves_by_name() {
        let amplitudes = vec![1.0, 0.0, -0.0, 2.0, f64::NAN];
        let noise = PerlinNoise::new(&mut XoroshiroRandomSource::new(9), -3, amplitudes);
        let factory = XoroshiroRandomSource::new(9).fork_positional();
        let present: Vec<bool> = noise.levels().iter().map(Option::is_some).collect();
        assert_eq!(present, [true, false, false, true, true]);
        for (i, octave) in [(0, -3), (3, 0), (4, 1)] {
            let want = ImprovedNoise::new(&mut factory.from_hash_of(&format!("octave_{octave}")));
            assert_eq!(noise.levels()[i].as_ref().unwrap().offset(), want.offset());
        }
        assert_eq!(noise.input_factor, 0.125);
        assert_eq!(noise.value_factor(), 16.0 / 31.0);
        // getOctaveNoise counts from the highest frequency.
        assert!(noise.octave_noise(1).is_some());
        assert!(noise.octave_noise(2).is_none());
        assert!(noise.octave_noise(5).is_none());
    }

    #[test]
    fn legacy_initialization_draws_from_frequency_1_down() {
        // Octaves -4..=-2 (zero = 4): build octave 0 and drop it, skip octave -1 (no level),
        // build -2, skip -3 (zero amplitude), build -4.
        let amplitudes = vec![0.5, 0.0, 1.0];
        let noise =
            PerlinNoise::new_legacy(&mut LegacyRandomSource::new(4), -4, amplitudes).unwrap();
        let mut r = LegacyRandomSource::new(4);
        let _ = ImprovedNoise::new(&mut r);
        r.consume_count(262);
        let minus_2 = ImprovedNoise::new(&mut r);
        r.consume_count(262);
        let minus_4 = ImprovedNoise::new(&mut r);
        let levels = noise.levels();
        assert_eq!(levels[0].as_ref().unwrap().offset(), minus_4.offset());
        assert!(levels[1].is_none());
        assert_eq!(levels[2].as_ref().unwrap().offset(), minus_2.offset());
        assert_eq!(noise.input_factor, 1.0 / 16.0);
        assert_eq!(noise.value_factor(), 4.0 / 7.0);
    }

    #[test]
    fn legacy_initialization_keeps_the_zero_octave() {
        let noise =
            PerlinNoise::new_legacy_for_blended_noise(&mut LegacyRandomSource::new(8), -2, 0)
                .unwrap();
        let mut r = LegacyRandomSource::new(8);
        let zero = ImprovedNoise::new(&mut r);
        let minus_1 = ImprovedNoise::new(&mut r);
        let minus_2 = ImprovedNoise::new(&mut r);
        let offsets: Vec<_> = noise
            .levels()
            .iter()
            .map(|l| l.as_ref().unwrap().offset())
            .collect();
        assert_eq!(offsets, [minus_2.offset(), minus_1.offset(), zero.offset()]);
        assert_eq!(noise.octave_noise(0).unwrap().offset(), zero.offset());
    }

    #[test]
    fn legacy_layouts_it_cant_build_are_errors() {
        let error = |first, amplitudes: Vec<f64>| {
            PerlinNoise::new_legacy(&mut LegacyRandomSource::new(0), first, amplitudes).unwrap_err()
        };
        assert_eq!(error(-1, vec![1.0, 1.0, 1.0]), NoiseError::LegacyOctaves);
        assert_eq!(error(0, vec![1.0, 0.0]), NoiseError::PositiveOctaves);
        let empty =
            PerlinNoise::new_legacy_for_blended_noise(&mut LegacyRandomSource::new(0), 0, -1);
        assert_eq!(empty.unwrap_err(), NoiseError::OctaveCount);
    }

    #[test]
    fn value_sums_weighted_octaves_in_f64() {
        let amplitudes = vec![1.5, 0.0, -0.75];
        let noise = PerlinNoise::new(&mut XoroshiroRandomSource::new(2), -5, amplitudes);
        let (x, y, z) = (1234.5, -77.25, 9.0e6);
        let level = |i: usize| noise.levels()[i].as_ref().unwrap();
        let valf = 4.0 / 7.0;
        let a = level(0).noise(wrap(x / 32.0), wrap(y / 32.0), wrap(z / 32.0));
        let c = level(2).noise(wrap(x / 8.0), wrap(y / 8.0), wrap(z / 8.0));
        let want = (0.0 + 1.5 * a * valf) + -0.75 * c * (valf / 4.0);
        assert_eq!(noise.get_value(x, y, z), want);
        // edgeValue: 1.5·2·valF + (−0.75)·2·valF/4.
        assert_eq!(
            noise.max_value(),
            (0.0 + 1.5 * 2.0 * valf) + -0.75 * 2.0 * (valf / 4.0)
        );
    }

    #[test]
    fn smeared_value_scales_the_smear_by_frequency() {
        let noise = PerlinNoise::new(&mut XoroshiroRandomSource::new(6), -1, vec![1.0]);
        let level = noise.levels()[0].as_ref().unwrap();
        let want = level.noise_smeared(wrap(5.0), wrap(3.0), wrap(-7.0), 0.75, 1.5) * 1.0;
        assert_eq!(
            noise.get_value_smeared(10.0, 6.0, -14.0, 1.5, 3.0),
            0.0 + want
        );
    }
}
