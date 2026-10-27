//! 26.2's `NormalNoise`: two octave sets, summed and scaled by a factor from the amplitudes'
//! span.

use spyglass_rng::RandomSource;

use super::PerlinNoise;
use crate::NoiseError;

/// `NormalNoise.INPUT_FACTOR`: the second set's input scale.
const INPUT_FACTOR: f64 = 1.018_126_888_217_522_7;

/// `NormalNoise.NoiseParameters`, as a 26.2 `worldgen/noise/*.json` file gives them
/// (`{"firstOctave", "amplitudes"}`, both required, no range checks). Parsing JSON happens above
/// this crate.
#[derive(Debug, Clone, PartialEq)]
pub struct NoiseParameters {
    /// `firstOctave`: level 0's frequency is `2^first_octave`.
    pub first_octave: i32,
    /// `amplitudes`, one per level, lowest frequency first; a zero leaves its level empty.
    pub amplitudes: Vec<f64>,
}

impl NoiseParameters {
    /// Parameters from their two fields.
    pub fn new(first_octave: i32, amplitudes: Vec<f64>) -> Self {
        NoiseParameters {
            first_octave,
            amplitudes,
        }
    }

    /// `NormalNoise`'s `valueFactor`: `0.16666666666666666 / expectedDeviation(hi − lo)`, where
    /// `lo` and `hi` are the first and last indices with a non-zero amplitude (NaN counts) and
    /// `expectedDeviation(k) = 0.1 · (1 + 1 / (double)(k + 1))`, in wrapping `int` arithmetic.
    /// Only the span counts, not the amplitudes' sizes. With no non-zero amplitude,
    /// `hi − lo` is `MIN_VALUE − MAX_VALUE`, which wraps to 1.
    pub fn value_factor(&self) -> f64 {
        let (mut lo, mut hi) = (i32::MAX, i32::MIN);
        for (i, &a) in self.amplitudes.iter().enumerate() {
            if a != 0.0 {
                lo = lo.min(i as i32);
                hi = hi.max(i as i32);
            }
        }
        let k = hi.wrapping_sub(lo);
        let deviation = 0.1 * (1.0 + 1.0 / f64::from(k.wrapping_add(1)));
        0.166_666_666_666_666_66 / deviation
    }
}

/// `NormalNoise`: `(first(x, y, z) + second(x·s, y·s, z·s)) · valueFactor`, with
/// `s` = 1.0181268882175227 and both sets from the same parameters.
#[derive(Debug, Clone)]
pub struct NormalNoise {
    first: PerlinNoise,
    second: PerlinNoise,
    value_factor: f64,
}

impl NormalNoise {
    /// `NormalNoise.create(RandomSource, NoiseParameters)`: `first`, then `second`, each a
    /// new-initialization octave set ([`PerlinNoise::new`]), each forking its own positional
    /// factory from `random` in turn.
    pub fn create<R: RandomSource>(random: &mut R, parameters: &NoiseParameters) -> Self {
        let first = PerlinNoise::new(
            random,
            parameters.first_octave,
            parameters.amplitudes.clone(),
        );
        let second = PerlinNoise::new(
            random,
            parameters.first_octave,
            parameters.amplitudes.clone(),
        );
        NormalNoise {
            first,
            second,
            value_factor: parameters.value_factor(),
        }
    }

    /// `NormalNoise.createLegacyNetherBiome(RandomSource, NoiseParameters)`: both sets with the
    /// legacy initialization ([`PerlinNoise::new_legacy`]), drawn from `random` in turn.
    ///
    /// # Errors
    ///
    /// As [`PerlinNoise::new_legacy`].
    pub fn create_legacy_nether_biome<R: RandomSource>(
        random: &mut R,
        parameters: &NoiseParameters,
    ) -> Result<Self, NoiseError> {
        let first = PerlinNoise::new_legacy(
            random,
            parameters.first_octave,
            parameters.amplitudes.clone(),
        )?;
        let second = PerlinNoise::new_legacy(
            random,
            parameters.first_octave,
            parameters.amplitudes.clone(),
        )?;
        Ok(NormalNoise {
            first,
            second,
            value_factor: parameters.value_factor(),
        })
    }

    /// The first octave set.
    pub fn first(&self) -> &PerlinNoise {
        &self.first
    }

    /// The second octave set, sampled at 1.0181268882175227 times the input.
    pub fn second(&self) -> &PerlinNoise {
        &self.second
    }

    /// `valueFactor`; see [`NoiseParameters::value_factor`].
    pub fn value_factor(&self) -> f64 {
        self.value_factor
    }

    /// `getValue(double, double, double)`: each set's sum on its own, added, then scaled, in
    /// `f64`.
    pub fn get_value(&self, x: f64, y: f64, z: f64) -> f64 {
        let x2 = x * INPUT_FACTOR;
        let y2 = y * INPUT_FACTOR;
        let z2 = z * INPUT_FACTOR;
        (self.first.get_value(x, y, z) + self.second.get_value(x2, y2, z2)) * self.value_factor
    }

    /// `maxValue()`: `(first.maxValue() + second.maxValue()) · valueFactor`.
    pub fn max_value(&self) -> f64 {
        (self.first.max_value() + self.second.max_value()) * self.value_factor
    }
}

#[cfg(test)]
mod tests {
    use spyglass_rng::{LegacyRandomSource, PositionalRandomFactory, XoroshiroRandomSource};

    use super::*;
    use crate::v1_21::ImprovedNoise;

    fn factor(amplitudes: &[f64]) -> f64 {
        NoiseParameters::new(-3, amplitudes.to_vec()).value_factor()
    }

    /// `(1/6) / (0.1 · (1 + 1/(k + 1)))` for a span of `k + 1` levels.
    fn by_span(k: i32) -> f64 {
        0.166_666_666_666_666_66 / (0.1 * (1.0 + 1.0 / f64::from(k + 1)))
    }

    #[test]
    fn value_factor_depends_only_on_the_span() {
        // One level: 0.1 · 2 = 0.2, and (1/6) / 0.2.
        assert_eq!(factor(&[1.0]), 0.166_666_666_666_666_66 / 0.2);
        assert_eq!(factor(&[1.0]), 0.833_333_333_333_333_3);
        // Sizes don't matter, nor do zeros inside the span.
        assert_eq!(factor(&[1.0, 1.0, 1.0]), by_span(2));
        assert_eq!(factor(&[1.5, 0.0, 1.0, 2.0]), by_span(3));
        assert_eq!(factor(&[1.5, 0.0, 1.0, 2.0]), factor(&[9.0, 9.0, 9.0, 9.0]));
        // Leading and trailing zeros don't count: the span is from the first non-zero to the last.
        assert_eq!(factor(&[0.0, 0.0, 1.0, 0.5, 0.0]), by_span(1));
        assert_eq!(factor(&[0.0, 1.0]), factor(&[1.0]));
        assert_eq!(factor(&[1.0, 0.0, 0.0]), factor(&[1.0]));
        // -0.0 is zero, NaN isn't.
        assert_eq!(factor(&[-0.0, 1.0, -0.0]), factor(&[1.0]));
        assert_eq!(factor(&[f64::NAN, 0.0, 1.0]), by_span(2));
        // Negative amplitudes count like positive ones.
        assert_eq!(factor(&[-1.0, 0.0, 1.0]), by_span(2));
    }

    #[test]
    fn value_factor_with_no_non_zero_amplitude() {
        // hi − lo = MIN_VALUE − MAX_VALUE = 1, wrapping: 0.1 · 1.5 = 0.15000000000000002.
        let all_zero = 0.166_666_666_666_666_66 / 0.150_000_000_000_000_02;
        assert_eq!(0.1 * 1.5, 0.150_000_000_000_000_02);
        assert_eq!(factor(&[0.0, 0.0, 0.0]), all_zero);
        assert_eq!(factor(&[]), all_zero);
        assert_eq!(factor(&[-0.0]), all_zero);
        // That's the span of two levels.
        assert_eq!(all_zero, by_span(1));
    }

    #[test]
    fn value_factor_is_26_3_legacy_parity_at_amplitude_1() {
        // 26.3's computeParityNormalizationFactor: base_amplitude · 0.5 · (1/3) / deviation.
        assert_eq!(0.5 * 0.333_333_333_333_333_3, 0.166_666_666_666_666_66);
    }

    #[test]
    fn all_zero_noise_is_zero() {
        let p = NoiseParameters::new(-4, vec![0.0, 0.0]);
        let noise = NormalNoise::create(&mut XoroshiroRandomSource::new(1), &p);
        assert_eq!(noise.get_value(100.0, 5.0, -3.0), 0.0);
        assert_eq!(noise.max_value(), 0.0);
    }

    #[test]
    fn create_forks_first_then_second() {
        let p = NoiseParameters::new(-2, vec![1.0, 0.0, 0.5]);
        let noise = NormalNoise::create(&mut XoroshiroRandomSource::new(12), &p);
        let mut r = XoroshiroRandomSource::new(12);
        let first = r.fork_positional();
        let second = r.fork_positional();
        for (set, factory) in [(noise.first(), &first), (noise.second(), &second)] {
            for (i, octave) in [(0, -2), (2, 0)] {
                let want =
                    ImprovedNoise::new(&mut factory.from_hash_of(&format!("octave_{octave}")));
                assert_eq!(set.levels()[i].as_ref().unwrap().offset(), want.offset());
            }
        }
    }

    #[test]
    fn value_adds_the_sets_then_scales() {
        let p = NoiseParameters::new(-6, vec![1.0, 1.0, 0.0, 2.0]);
        let noise = NormalNoise::create(&mut XoroshiroRandomSource::new(77), &p);
        let (x, y, z) = (311.0, -40.0, 1.0e5);
        let a = noise.first().get_value(x, y, z);
        let b = noise
            .second()
            .get_value(x * INPUT_FACTOR, y * INPUT_FACTOR, z * INPUT_FACTOR);
        assert_eq!(noise.get_value(x, y, z), (a + b) * by_span(3));
        assert_eq!(
            noise.max_value(),
            (noise.first().max_value() + noise.second().max_value()) * by_span(3)
        );
    }

    #[test]
    fn legacy_nether_biome_sets_draw_in_turn() {
        // 26.2's nether/temperature: firstOctave -7, amplitudes [1, 1]: per set, the zero
        // octave built and dropped, five skips of 262, then octaves -6 and -7.
        let p = NoiseParameters::new(-7, vec![1.0, 1.0]);
        let noise =
            NormalNoise::create_legacy_nether_biome(&mut LegacyRandomSource::new(5), &p).unwrap();
        let mut r = LegacyRandomSource::new(5);
        for set in [noise.first(), noise.second()] {
            let _ = ImprovedNoise::new(&mut r);
            r.consume_count(5 * 262);
            let minus_6 = ImprovedNoise::new(&mut r);
            let minus_7 = ImprovedNoise::new(&mut r);
            assert_eq!(set.levels()[1].as_ref().unwrap().offset(), minus_6.offset());
            assert_eq!(set.levels()[0].as_ref().unwrap().offset(), minus_7.offset());
        }
        assert_eq!(noise.value_factor(), by_span(1));
    }
}
