//! 26.2's `PerlinSimplexNoise`: 2D simplex octaves summed in `f64`, which 26.2's `Biome` uses
//! for temperature (`docs/specs/noise.md`, section "26.2 simplex octaves").

use std::collections::BTreeSet;

use spyglass_rng::{LegacyRandomSource, RandomSource};

use super::SimplexNoise;

/// `PerlinSimplexNoise`: one [`SimplexNoise`] per octave, highest frequency first.
#[derive(Debug, Clone)]
pub struct PerlinSimplexNoise {
    /// `noiseLevels`: `None` where an octave between the lowest and highest is left out.
    levels: Vec<Option<SimplexNoise>>,
    /// `highestFreqInputFactor`: `2^last`.
    input_factor: f64,
    /// `highestFreqValueFactor`: `1 / (2^count − 1)`.
    value_factor: f64,
}

impl PerlinSimplexNoise {
    /// `PerlinSimplexNoise(RandomSource, List<Integer>)`: octave 0 first, then the higher
    /// octaves from `random`, then, if there are octaves above 0, the lower ones from a legacy
    /// random seeded by octave 0's value at its own offsets. A left-out octave consumes 262
    /// draws.
    ///
    /// # Panics
    ///
    /// If `octaves` is empty, as the game throws.
    pub fn new<R: RandomSource>(random: &mut R, octaves: &[i32]) -> Self {
        let octaves: BTreeSet<i32> = octaves.iter().copied().collect();
        let (Some(&first), Some(&last)) = (octaves.first(), octaves.last()) else {
            panic!("Need some octaves!");
        };
        let count = (-first) + last + 1;
        let zero = SimplexNoise::new(random);
        let top = last;
        let mut levels: Vec<Option<SimplexNoise>> = vec![None; count as usize];
        if top >= 0 && top < count && octaves.contains(&0) {
            levels[top as usize] = Some(zero.clone());
        }
        // Index `m` holds octave `top − m`, so the indices above `top` are the lower octaves.
        // A left-out octave skips the game's fixed 262 draws (see `legacy_levels`).
        for m in top + 1..count {
            if m >= 0 && octaves.contains(&(top - m)) {
                levels[m as usize] = Some(SimplexNoise::new(random));
            } else {
                random.consume_count(262);
            }
        }
        if last > 0 {
            // The higher octaves come from a random seeded by octave 0's own value at its
            // offsets, scaled to the whole `long` range.
            let [xo, yo, zo] = zero.offset();
            // d2l: saturating, NaN 0.
            let seed = (zero.get_value(xo, yo, zo) * 9.223_372_036_854_776E18) as i64;
            let mut lower = LegacyRandomSource::new(seed);
            for o in (0..top).rev() {
                if o < count && octaves.contains(&(top - o)) {
                    levels[o as usize] = Some(SimplexNoise::new(&mut lower));
                } else {
                    lower.consume_count(262);
                }
            }
        }
        PerlinSimplexNoise {
            levels,
            input_factor: 2.0_f64.powf(f64::from(last)),
            value_factor: 1.0 / (2.0_f64.powf(f64::from(count)) - 1.0),
        }
    }

    /// `getValue(x, z, false)`: each octave's 2D value at `(x · e, z · e)`, times `f`, summed in
    /// order, with `e` halving and `f` doubling from octave to octave. The offsets aren't
    /// added.
    pub fn get_value(&self, x: f64, z: f64) -> f64 {
        let mut sum = 0.0;
        // The spec's `e` and `f`.
        let mut frequency = self.input_factor;
        let mut weight = self.value_factor;
        for level in &self.levels {
            if let Some(noise) = level {
                // `x · e + 0.0`: the offset the game adds only with useNoiseOffsets.
                sum += noise.get_value_2d(x * frequency + 0.0, z * frequency + 0.0) * weight;
            }
            frequency /= 2.0;
            weight *= 2.0;
        }
        sum
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_octave_is_its_simplex_noise() {
        let noise = PerlinSimplexNoise::new(&mut LegacyRandomSource::new(1234), &[0]);
        let simplex = SimplexNoise::new(&mut LegacyRandomSource::new(1234));
        for (x, z) in [(0.0, 0.0), (12.5, -3.25), (-1000.0, 77.0)] {
            assert_eq!(noise.get_value(x, z), simplex.get_value_2d(x, z));
        }
    }

    #[test]
    fn lower_octaves_halve_the_frequency_and_double_the_weight() {
        let noise = PerlinSimplexNoise::new(&mut LegacyRandomSource::new(3456), &[-2, -1, 0]);
        let mut random = LegacyRandomSource::new(3456);
        let octaves: Vec<SimplexNoise> = (0..3).map(|_| SimplexNoise::new(&mut random)).collect();
        let (x, z) = (31.0, -17.5);
        let want = octaves[0].get_value_2d(x, z) * (1.0 / 7.0)
            + octaves[1].get_value_2d(x * 0.5, z * 0.5) * (2.0 / 7.0)
            + octaves[2].get_value_2d(x * 0.25, z * 0.25) * (4.0 / 7.0);
        assert_eq!(noise.get_value(x, z), want);
    }
}
