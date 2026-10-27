//! 26.2's `BlendedNoise`: old blended noise, three legacy octave sets of smeared noise, in `f64`.

use spyglass_rng::RandomSource;

use super::PerlinNoise;
use super::perlin::wrap;
use crate::mth;

/// `BlendedNoise`: the `old_blended_noise` density function of
/// 26.2, seeded (`docs/specs/noise.md`, "26.2 blended noise").
#[derive(Debug, Clone)]
pub struct BlendedNoise {
    min_limit: PerlinNoise,
    max_limit: PerlinNoise,
    main: PerlinNoise,
    /// `xzMultiplier`: `684.412 · xz_scale`.
    xz_multiplier: f64,
    /// `yMultiplier`: `684.412 · y_scale`.
    y_multiplier: f64,
    xz_factor: f64,
    y_factor: f64,
    smear_scale_multiplier: f64,
}

impl BlendedNoise {
    /// `BlendedNoise(RandomSource, double, double, double, double, double)`: from `random` in
    /// turn, `minLimitNoise` and `maxLimitNoise` (octaves −15 to 0), then `mainNoise` (−7 to 0),
    /// each a legacy-initialized octave set with every amplitude 1.
    pub fn new<R: RandomSource>(
        random: &mut R,
        xz_scale: f64,
        y_scale: f64,
        xz_factor: f64,
        y_factor: f64,
        smear_scale_multiplier: f64,
    ) -> Self {
        let mut set = |first| {
            PerlinNoise::new_legacy_for_blended_noise(random, first, 0)
                .expect("octaves up to 0 always build")
        };
        let min_limit = set(-15);
        let max_limit = set(-15);
        let main = set(-7);
        BlendedNoise {
            min_limit,
            max_limit,
            main,
            xz_multiplier: 684.412 * xz_scale,
            y_multiplier: 684.412 * y_scale,
            xz_factor,
            y_factor,
            smear_scale_multiplier,
        }
    }

    /// `minLimitNoise`, `maxLimitNoise` and `mainNoise`.
    pub fn octave_sets(&self) -> [&PerlinNoise; 3] {
        [&self.min_limit, &self.max_limit, &self.main]
    }

    /// `compute(FunctionContext)` at block `(x, y, z)`.
    ///
    /// The main noise picks `t = (main / 10 + 1) / 2`, then the limit noises' sums are
    /// blended by `Mth.clampedLerp(t, min / 512, max / 512) / 128`. As the game does, the `min`
    /// sum is skipped (left 0) when `t ≥ 1` and the `max` sum when `t ≤ 0`; that's not only an
    /// optimization, since at `t = 1` exactly the lerp reads the skipped `min` as 0.
    pub fn compute(&self, block_x: i32, block_y: i32, block_z: i32) -> f64 {
        let x = f64::from(block_x) * self.xz_multiplier;
        let y = f64::from(block_y) * self.y_multiplier;
        let z = f64::from(block_z) * self.xz_multiplier;
        let mx = x / self.xz_factor;
        let my = y / self.y_factor;
        let mz = z / self.xz_factor;
        let smear = self.y_multiplier * self.smear_scale_multiplier;
        let main_smear = smear / self.y_factor;
        let (mut min, mut max, mut main) = (0.0, 0.0, 0.0);
        let mut f = 1.0;
        for i in 0..8 {
            if let Some(noise) = self.main.octave_noise(i) {
                main += noise.noise_smeared(
                    wrap(mx * f),
                    wrap(my * f),
                    wrap(mz * f),
                    main_smear * f,
                    my * f,
                ) / f;
            }
            f /= 2.0;
        }
        let t = (main / 10.0 + 1.0) / 2.0;
        // dcmpl/iflt and dcmpg/ifgt: a NaN t skips neither.
        let skip_min = t >= 1.0;
        let skip_max = t <= 0.0;
        f = 1.0;
        for i in 0..16 {
            let wx = wrap(x * f);
            let wy = wrap(y * f);
            let wz = wrap(z * f);
            let smear_f = smear * f;
            if !skip_min && let Some(noise) = self.min_limit.octave_noise(i) {
                min += noise.noise_smeared(wx, wy, wz, smear_f, y * f) / f;
            }
            if !skip_max && let Some(noise) = self.max_limit.octave_noise(i) {
                max += noise.noise_smeared(wx, wy, wz, smear_f, y * f) / f;
            }
            f /= 2.0;
        }
        mth::clamped_lerp(t, min / 512.0, max / 512.0) / 128.0
    }

    /// `maxValue()`: `minLimitNoise.maxBrokenValue(yMultiplier)`.
    pub fn max_value(&self) -> f64 {
        self.min_limit.max_broken_value(self.y_multiplier)
    }

    /// `minValue()`: `−maxValue()`.
    pub fn min_value(&self) -> f64 {
        -self.max_value()
    }
}

#[cfg(test)]
mod tests {
    use spyglass_rng::LegacyRandomSource;

    use super::*;
    use crate::v1_21::ImprovedNoise;

    fn overworld(seed: i64) -> BlendedNoise {
        // 26.2's overworld: xz_scale 0.25, y_scale 0.125, xz_factor 80, y_factor 160,
        // smear_scale_multiplier 8.
        BlendedNoise::new(
            &mut LegacyRandomSource::new(seed),
            0.25,
            0.125,
            80.0,
            160.0,
            8.0,
        )
    }

    #[test]
    fn construction_draws_min_max_then_main() {
        let noise = overworld(3);
        let mut r = LegacyRandomSource::new(3);
        // Each set: octave 0 first, then −1 down; none skipped.
        let mut firsts = Vec::new();
        for count in [16, 16, 8] {
            let zero = ImprovedNoise::new(&mut r);
            for _ in 1..count {
                let _ = ImprovedNoise::new(&mut r);
            }
            firsts.push(zero.offset());
        }
        for (set, offset) in noise.octave_sets().into_iter().zip(firsts) {
            assert_eq!(set.octave_noise(0).unwrap().offset(), offset);
        }
        assert_eq!(noise.octave_sets()[0].levels().len(), 16);
        assert_eq!(noise.octave_sets()[2].levels().len(), 8);
    }

    /// `compute` written out from the spec's pseudocode, without the skips.
    fn unskipped(n: &BlendedNoise, bx: i32, by: i32, bz: i32) -> (f64, f64, f64, f64) {
        let x = f64::from(bx) * n.xz_multiplier;
        let y = f64::from(by) * n.y_multiplier;
        let z = f64::from(bz) * n.xz_multiplier;
        let (mx, my, mz) = (x / n.xz_factor, y / n.y_factor, z / n.xz_factor);
        let smear = n.y_multiplier * n.smear_scale_multiplier;
        let mut main = 0.0;
        let mut f = 1.0;
        for i in 0..8 {
            let o = n.main.octave_noise(i).unwrap();
            main += o.noise_smeared(
                wrap(mx * f),
                wrap(my * f),
                wrap(mz * f),
                smear / n.y_factor * f,
                my * f,
            ) / f;
            f /= 2.0;
        }
        let (mut min, mut max) = (0.0, 0.0);
        f = 1.0;
        for i in 0..16 {
            let (wx, wy, wz) = (wrap(x * f), wrap(y * f), wrap(z * f));
            min += n
                .min_limit
                .octave_noise(i)
                .unwrap()
                .noise_smeared(wx, wy, wz, smear * f, y * f)
                / f;
            max += n
                .max_limit
                .octave_noise(i)
                .unwrap()
                .noise_smeared(wx, wy, wz, smear * f, y * f)
                / f;
            f /= 2.0;
        }
        ((main / 10.0 + 1.0) / 2.0, min, max, main)
    }

    #[test]
    fn compute_follows_the_formula() {
        let noise = overworld(11);
        let mut seen = [false; 3];
        for bx in (-64..64).step_by(16) {
            for by in (-64..320).step_by(24) {
                for bz in [-1000, 0, 37] {
                    let (t, min, max, _) = unskipped(&noise, bx, by, bz);
                    let want = (if t < 0.0 {
                        seen[0] = true;
                        min / 512.0
                    } else if t > 1.0 {
                        seen[2] = true;
                        max / 512.0
                    } else {
                        seen[1] = true;
                        min / 512.0 + t * (max / 512.0 - min / 512.0)
                    }) / 128.0;
                    assert_eq!(noise.compute(bx, by, bz), want, "({bx}, {by}, {bz})");
                }
            }
        }
        // The lerp branch at least; the clamps need extreme main noise.
        assert!(seen[1]);
    }

    #[test]
    fn range_is_the_min_limit_broken_value() {
        let noise = overworld(0);
        // Every amplitude 1 over 16 levels: Σ (y + 2) · 2^15/(2^16 − 1) / 2^i.
        let m = 684.412 * 0.125 + 2.0;
        let mut value = 32768.0 / 65535.0;
        let mut want = 0.0;
        for _ in 0..16 {
            want += 1.0 * m * value;
            value /= 2.0;
        }
        assert_eq!(noise.max_value(), want);
        assert_eq!(noise.min_value(), -want);
    }
}
