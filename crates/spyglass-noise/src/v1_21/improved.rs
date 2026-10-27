//! 26.2's `ImprovedNoise`: one octave of Perlin noise, plain or smeared, in `f64`.

use spyglass_rng::RandomSource;

use crate::mth;
use crate::perlin::GRADIENT;

/// `1.0E-7f` widened, as `noise` adds it to the smear's quotient.
const SHIFT_UP_EPSILON: f64 = 1.000_000_011_686_097_4e-7;

/// `ImprovedNoise.gradDot(int, double, double, double)`, which is
/// `SimplexNoise.dot(GRADIENT[h & 15], x, y, z)`: in `f64`, the z term always added.
fn grad_dot(hash: i32, x: f64, y: f64, z: f64) -> f64 {
    let [gx, gy, gz] = GRADIENT[(hash & 15) as usize];
    (f64::from(gx) * x + f64::from(gy) * y) + f64::from(gz) * z
}

/// `ImprovedNoise`: one octave of 3D gradient noise in `f64`. Plain Perlin with a zero y scale,
/// smeared Perlin with a non-zero one. It doesn't wrap its coordinates; the octave set does.
#[derive(Debug, Clone)]
pub struct ImprovedNoise {
    perms: [u8; 256],
    offset: [f64; 3],
}

impl ImprovedNoise {
    /// `ImprovedNoise(RandomSource)`: `xo`, `yo`, `zo` (`nextDouble() · 256`), then the
    /// permutation shuffle (`j = nextInt(256 − i)`, swap `p[i]` and `p[i + j]`). The same draws
    /// as 26.3's [`PerlinNoise::new`](crate::PerlinNoise::new).
    pub fn new(random: &mut impl RandomSource) -> Self {
        let offset = std::array::from_fn(|_| random.next_double() * 256.0);
        let mut perms: [u8; 256] = std::array::from_fn(|i| i as u8);
        for i in 0..256 {
            let j = random.next_int_bounded(256 - i as i32) as usize;
            perms.swap(i, i + j);
        }
        ImprovedNoise { perms, offset }
    }

    /// `xo`, `yo` and `zo`.
    pub fn offset(&self) -> [f64; 3] {
        self.offset
    }

    /// The permutation table, as signed bytes like the game's `byte[] p`.
    pub fn perm(&self, i: usize) -> i8 {
        self.perms[i] as i8
    }

    /// `p(int)`.
    fn p(&self, i: i32) -> i32 {
        i32::from(self.perms[(i & 255) as usize])
    }

    /// `noise(double, double, double)`: plain Perlin noise, `noise(x, y, z, 0, 0)`.
    pub fn noise(&self, x: f64, y: f64, z: f64) -> f64 {
        self.noise_smeared(x, y, z, 0.0, 0.0)
    }

    /// `noise(double, double, double, double, double)`. With a non-zero `y_scale` (NaN counts
    /// as non-zero), the gradients see `yf` less a fudge: `y_max` if `0 ≤ y_max < yf`, else
    /// `yf`, rounded down to a multiple of `y_scale` after a small upward shift. The y
    /// smoothstep sees `yf` itself.
    pub fn noise_smeared(&self, x: f64, y: f64, z: f64, y_scale: f64, y_max: f64) -> f64 {
        let xx = x + self.offset[0];
        let yy = y + self.offset[1];
        let zz = z + self.offset[2];
        let (xi, yi, zi) = (mth::floor(xx), mth::floor(yy), mth::floor(zz));
        let xf = xx - f64::from(xi);
        let yf = yy - f64::from(yi);
        let zf = zz - f64::from(zi);
        // dcmpl/ifeq: NaN takes the fudge branch.
        let fudge = if y_scale != 0.0 || y_scale.is_nan() {
            // dcmpl/iflt then dcmpg/ifge: a NaN y_max fails both tests and takes yf.
            let r = if y_max >= 0.0 && y_max < yf {
                y_max
            } else {
                yf
            };
            f64::from(mth::floor(r / y_scale + SHIFT_UP_EPSILON)) * y_scale
        } else {
            0.0
        };
        self.sample_and_lerp(xi, yi, zi, xf, yf - fudge, zf, yf)
    }

    /// `sampleAndLerp(int, int, int, double, double, double, double)`: `yf` is the gradients'
    /// y fraction, `y_smooth` the smoothstep's.
    #[allow(
        clippy::too_many_arguments,
        reason = "mirrors the game's sampleAndLerp"
    )]
    fn sample_and_lerp(
        &self,
        xi: i32,
        yi: i32,
        zi: i32,
        xf: f64,
        yf: f64,
        zf: f64,
        y_smooth: f64,
    ) -> f64 {
        // Java int additions, which wrap; p masks the sum.
        let add = |p: i32, q: i32| p.wrapping_add(q);
        let h0 = self.p(xi);
        let h1 = self.p(add(xi, 1));
        let h00 = self.p(add(h0, yi));
        let h01 = self.p(add(add(h0, yi), 1));
        let h10 = self.p(add(h1, yi));
        let h11 = self.p(add(add(h1, yi), 1));
        let v000 = grad_dot(self.p(add(h00, zi)), xf, yf, zf);
        let v100 = grad_dot(self.p(add(h10, zi)), xf - 1.0, yf, zf);
        let v010 = grad_dot(self.p(add(h01, zi)), xf, yf - 1.0, zf);
        let v110 = grad_dot(self.p(add(h11, zi)), xf - 1.0, yf - 1.0, zf);
        let v001 = grad_dot(self.p(add(add(h00, zi), 1)), xf, yf, zf - 1.0);
        let v101 = grad_dot(self.p(add(add(h10, zi), 1)), xf - 1.0, yf, zf - 1.0);
        let v011 = grad_dot(self.p(add(add(h01, zi), 1)), xf, yf - 1.0, zf - 1.0);
        let v111 = grad_dot(self.p(add(add(h11, zi), 1)), xf - 1.0, yf - 1.0, zf - 1.0);
        let tx = mth::smoothstep_f64(xf);
        let ty = mth::smoothstep_f64(y_smooth);
        let tz = mth::smoothstep_f64(zf);
        mth::lerp3_f64(tx, ty, tz, v000, v100, v010, v110, v001, v101, v011, v111)
    }

    #[cfg(test)]
    pub(crate) fn with_identity_perms() -> Self {
        ImprovedNoise {
            perms: std::array::from_fn(|i| i as u8),
            offset: [0.0; 3],
        }
    }
}

#[cfg(test)]
mod tests {
    use spyglass_rng::{LegacyRandomSource, XoroshiroRandomSource};

    use super::*;
    use crate::PerlinNoise;

    #[test]
    fn draws_as_26_3_perlin_does() {
        for seed in [0, 3, -77] {
            let mut a = XoroshiroRandomSource::new(seed);
            let mut b = XoroshiroRandomSource::new(seed);
            let improved = ImprovedNoise::new(&mut a);
            let perlin = PerlinNoise::new(&mut b);
            assert_eq!(improved.offset(), perlin.offset());
            for i in 0..256 {
                assert_eq!(improved.perm(i), perlin.perm(i));
            }
            assert_eq!(a.next_long(), b.next_long());
        }
    }

    #[test]
    fn zero_at_lattice_points() {
        let noise = ImprovedNoise::new(&mut LegacyRandomSource::new(1));
        let [ox, oy, oz] = noise.offset();
        // Every gradient dot is 0 at a lattice corner, whatever the offsets.
        let (x, y, z) = (5.0 - ox.fract(), -3.0 - oy.fract(), 12.0 - oz.fract());
        let v = noise.noise(x, y, z);
        assert!(v.abs() < 1e-12, "{v}");
    }

    /// With `p[i] = i` and no offsets, worked by hand: at (0.25, 0.5, 0.75) each corner's hash
    /// is `p(p(p(dx) + dy) + dz) = dx + dy + dz`, so its gradient is `GRADIENT[dx + dy + dz]`:
    /// (1,1,0) for the sum 0, (−1,1,0) for 1, (1,−1,0) for 2 and (−1,−1,0) for 3.
    #[test]
    fn value_with_identity_permutation() {
        let noise = ImprovedNoise::with_identity_perms();
        // Corners in lerp3's order: 0.75, 1.25, −0.75, −0.25, 0.25, −1.25, 0.75, 1.25.
        // smoothstep(0.25) = 0.103515625, smoothstep(0.5) = 0.5, smoothstep(0.75) = 0.896484375.
        // z0 face: x lerps 0.8017578125 and −0.6982421875, then y 0.0517578125.
        // z1 face: x lerps 0.0947265625 and 0.8017578125, then y 0.4482421875.
        // Every step is exact in f64.
        let by_hand = 0.051_757_812_5 + 0.896_484_375 * 0.396_484_375;
        assert_eq!(noise.noise(0.25, 0.5, 0.75), by_hand);
    }

    #[test]
    fn smear_shifts_only_the_gradients() {
        let noise = ImprovedNoise::with_identity_perms();
        // y_scale 0.25 and y_max 0.4 (0 ≤ 0.4 < yf = 0.5): fudge = floor(1.6 + ε) · 0.25 =
        // 0.25, so the gradients see yf 0.25 while the smoothstep still sees 0.5.
        let smeared = noise.noise_smeared(0.25, 0.5, 0.75, 0.25, 0.4);
        let direct = noise.sample_and_lerp(0, 0, 0, 0.25, 0.25, 0.75, 0.5);
        assert_eq!(smeared, direct);
        // y_max outside [0, yf): yf itself is rounded, floor(2 + ε) · 0.25 = 0.5, so yf − fudge
        // is 0.
        let at_yf = noise.noise_smeared(0.25, 0.5, 0.75, 0.25, 0.9);
        assert_eq!(at_yf, noise.sample_and_lerp(0, 0, 0, 0.25, 0.0, 0.75, 0.5));
        let nan_max = noise.noise_smeared(0.25, 0.5, 0.75, 0.25, f64::NAN);
        assert_eq!(nan_max, at_yf);
        // A zero scale is plain noise.
        assert_eq!(
            noise.noise_smeared(0.25, 0.5, 0.75, 0.0, 0.4),
            noise.noise(0.25, 0.5, 0.75)
        );
        // A NaN scale takes the fudge branch, and the NaN fudge poisons the gradients.
        assert!(noise.noise_smeared(0.25, 0.5, 0.75, f64::NAN, 0.4).is_nan());
    }

    #[test]
    fn epsilon_is_the_float_widened() {
        assert_eq!(SHIFT_UP_EPSILON, f64::from(1.0e-7_f32));
    }
}
