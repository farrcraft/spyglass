//! `SimplexNoise`: 2D simplex noise, as the End's outer islands use it.

use spyglass_rng::RandomSource;

use crate::mth;
use crate::perlin::GRADIENT;

/// `SimplexNoise` (with the state of `GradientNoise`).
#[derive(Debug, Clone)]
pub struct SimplexNoise {
    perms: [u8; 256],
    offset: [f64; 3],
}

impl SimplexNoise {
    /// `SimplexNoise(RandomSource, boolean)`: three offsets (`nextDouble() · scale`, with scale
    /// 256, or 0 for `zero_offsets`, the three draws taken either way), then the permutation
    /// shuffle, as `PerlinNoise`'s.
    pub fn new(random: &mut impl RandomSource, zero_offsets: bool) -> Self {
        let scale = if zero_offsets { 0.0 } else { 256.0 };
        let offset = std::array::from_fn(|_| random.next_double() * scale);
        SimplexNoise {
            perms: crate::perlin::permutation(random),
            offset,
        }
    }

    /// `offsetX`, `offsetY` and `offsetZ`.
    pub fn offset(&self) -> [f64; 3] {
        self.offset
    }

    /// `GradientNoise.permute(int)`.
    fn permute(&self, i: i32) -> i32 {
        i32::from(self.perms[(i & 255) as usize])
    }

    /// `get(double, double)`: 2D simplex noise, summed in `f64` and narrowed once.
    pub fn get(&self, x: f64, y: f64) -> f32 {
        // SQRT_3 = Math.sqrt(3.0); F2 and G2 as the class initializer computes them.
        let sqrt_3 = 3.0_f64.sqrt();
        let f2 = 0.5 * (sqrt_3 - 1.0);
        let g2 = (3.0 - sqrt_3) / 6.0;
        let xx = x + self.offset[0];
        let yy = y + self.offset[1];
        let s = (xx + yy) * f2;
        let i = mth::floor(xx + s);
        let j = mth::floor(yy + s);
        let t = f64::from(i.wrapping_add(j)) * g2;
        let x0 = xx - (f64::from(i) - t);
        let y0 = yy - (f64::from(j) - t);
        // dcmpl/ifle: NaN takes the else branch.
        let (i1, j1) = if x0 > y0 { (1, 0) } else { (0, 1) };
        let x1 = (x0 - f64::from(i1)) + g2;
        let y1 = (y0 - f64::from(j1)) + g2;
        let x2 = (x0 - 1.0) + 2.0 * g2;
        let y2 = (y0 - 1.0) + 2.0 * g2;
        let (ii, jj) = (i & 255, j & 255);
        let g0 = self.permute(ii + self.permute(jj)) % 12;
        let g1 = self.permute(ii + i1 + self.permute(jj + j1)) % 12;
        let g2i = self.permute(ii + 1 + self.permute(jj + 1)) % 12;
        let n0 = corner(g0, x0, y0, 0.0, 0.5);
        let n1 = corner(g1, x1, y1, 0.0, 0.5);
        let n2 = corner(g2i, x2, y2, 0.0, 0.5);
        (70.0 * ((n0 + n1) + n2)) as f32
    }
}

/// `SimplexNoise.getCornerNoise3D`, the same in 26.2 and 26.3.
pub(crate) fn corner(g: i32, x: f64, y: f64, z: f64, base: f64) -> f64 {
    let t = ((base - x * x) - y * y) - z * z;
    // dcmpg/ifge: NaN goes on to the else branch.
    if t < 0.0 {
        0.0
    } else {
        let t = t * t;
        (t * t) * dot(g, x, y, z)
    }
}

/// `GradientNoise.Gradient.dot(double, double, double)` (26.2's `SimplexNoise.dot`): in `f64`,
/// the z term always added.
pub(crate) fn dot(g: i32, x: f64, y: f64, z: f64) -> f64 {
    let [gx, gy, gz] = GRADIENT[g as usize];
    (f64::from(gx) * x + f64::from(gy) * y) + f64::from(gz) * z
}

#[cfg(test)]
mod tests {
    use spyglass_rng::LegacyRandomSource;

    use super::*;

    #[test]
    fn constants_are_the_games() {
        let sqrt_3 = 3.0_f64.sqrt();
        assert_eq!((0.5 * (sqrt_3 - 1.0)).to_bits(), 0x3FD7_6CF5_D0B0_9954);
        assert_eq!(((3.0 - sqrt_3) / 6.0).to_bits(), 0x3FCB_0CB1_74DF_99C8);
    }

    #[test]
    fn zero_offsets_still_draw() {
        let mut a = LegacyRandomSource::new(7);
        let mut b = LegacyRandomSource::new(7);
        let zero = SimplexNoise::new(&mut a, true);
        let offset = SimplexNoise::new(&mut b, false);
        assert_eq!(zero.offset(), [0.0; 3]);
        assert!(offset.offset().iter().all(|&o| o > 0.0));
        assert_eq!(zero.perms, offset.perms);
        assert_eq!(a.next_long(), b.next_long());
    }

    #[test]
    fn corners_fall_off() {
        assert_eq!(corner(0, 0.8, 0.0, 0.0, 0.5), 0.0);
        let t: f64 = 0.5 - 0.1 * 0.1;
        assert_eq!(corner(0, 0.1, 0.0, 0.0, 0.5), (t * t) * (t * t) * 0.1);
    }

    #[test]
    fn values_with_identity_permutations() {
        // Worked by hand from the spec's steps (in Python), with perms[i] = i and no offsets.
        let mut noise = SimplexNoise::new(&mut LegacyRandomSource::new(0), true);
        noise.perms = std::array::from_fn(|i| i as u8);
        for (x, y, bits) in [
            (0.3, 0.7, 0xbf65_0b6f),
            (5.25, -3.5, 0xbd77_1290),
            (-100.0, 42.0, 0xbf5c_dbcd),
        ] {
            assert_eq!(noise.get(x, y).to_bits(), bits, "({x}, {y})");
        }
    }
}
