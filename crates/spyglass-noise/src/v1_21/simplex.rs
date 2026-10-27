//! 26.2's `SimplexNoise`: 2D and 3D simplex noise in `f64`, without its offsets.

use spyglass_rng::RandomSource;

use crate::mth;
use crate::perlin::GRADIENT;

/// `SimplexNoise.F3` and `G3` as the bytecode loads them: `1/3` and `1/6`.
const F3: f64 = 0.333_333_333_333_333_3;
const G3: f64 = 0.166_666_666_666_666_66;

/// `SimplexNoise`: the permutation of [`ImprovedNoise`](super::ImprovedNoise), with offsets it
/// draws but never adds.
#[derive(Debug, Clone)]
pub struct SimplexNoise {
    perms: [u8; 256],
    offset: [f64; 3],
}

impl SimplexNoise {
    /// `SimplexNoise(RandomSource)`: `xo`, `yo`, `zo` (`nextDouble() · 256`), then the
    /// permutation shuffle, as `ImprovedNoise`'s. The same draws as 26.3's
    /// [`SimplexNoise::new(random, false)`](crate::SimplexNoise::new).
    pub fn new(random: &mut impl RandomSource) -> Self {
        let offset = std::array::from_fn(|_| random.next_double() * 256.0);
        let mut perms: [u8; 256] = std::array::from_fn(|i| i as u8);
        for i in 0..256 {
            let j = random.next_int_bounded(256 - i as i32) as usize;
            perms.swap(i, i + j);
        }
        SimplexNoise { perms, offset }
    }

    /// `xo`, `yo` and `zo`, which neither `getValue` adds.
    pub fn offset(&self) -> [f64; 3] {
        self.offset
    }

    /// `p(int)`: `p[i & 255]`.
    fn p(&self, i: i32) -> i32 {
        i32::from(self.perms[(i & 255) as usize])
    }

    /// `getValue(double, double)`: 2D simplex noise, `70 · ((n0 + n1) + n2)` in `f64`. The
    /// coordinates are used as given.
    pub fn get_value_2d(&self, x: f64, y: f64) -> f64 {
        // SQRT_3 = Math.sqrt(3.0); F2 and G2 as the class initializer computes them.
        let sqrt_3 = 3.0_f64.sqrt();
        let f2 = 0.5 * (sqrt_3 - 1.0);
        let g2 = (3.0 - sqrt_3) / 6.0;
        let s = (x + y) * f2;
        let i = mth::floor(x + s);
        let j = mth::floor(y + s);
        let t = f64::from(i.wrapping_add(j)) * g2;
        let x0 = x - (f64::from(i) - t);
        let y0 = y - (f64::from(j) - t);
        // dcmpl/ifle: NaN takes the else branch.
        let (i1, j1) = if x0 > y0 { (1, 0) } else { (0, 1) };
        let x1 = (x0 - f64::from(i1)) + g2;
        let y1 = (y0 - f64::from(j1)) + g2;
        let x2 = (x0 - 1.0) + 2.0 * g2;
        let y2 = (y0 - 1.0) + 2.0 * g2;
        let (ii, jj) = (i & 255, j & 255);
        let g0 = self.p(ii + self.p(jj)) % 12;
        let g1 = self.p(ii + i1 + self.p(jj + j1)) % 12;
        let g2i = self.p(ii + 1 + self.p(jj + 1)) % 12;
        let n0 = corner(g0, x0, y0, 0.0, 0.5);
        let n1 = corner(g1, x1, y1, 0.0, 0.5);
        let n2 = corner(g2i, x2, y2, 0.0, 0.5);
        70.0 * ((n0 + n1) + n2)
    }

    /// `getValue(double, double, double)`: 3D simplex noise, `32 · (((n0 + n1) + n2) + n3)` in
    /// `f64`, with `F3 = 1/3` and `G3 = 1/6` as constants and corners of base 0.6. The
    /// coordinates are used as given. No worldgen in spyglass's scope calls it.
    pub fn get_value(&self, x: f64, y: f64, z: f64) -> f64 {
        let s = ((x + y) + z) * F3;
        let i = mth::floor(x + s);
        let j = mth::floor(y + s);
        let k = mth::floor(z + s);
        let t = f64::from(i.wrapping_add(j).wrapping_add(k)) * G3;
        let x0 = x - (f64::from(i) - t);
        let y0 = y - (f64::from(j) - t);
        let z0 = z - (f64::from(k) - t);
        // The simplex's two middle corners, from the order of x0, y0, z0. The `>=` tests are
        // dcmpl/iflt and the `<` tests dcmpg/ifge, so a NaN fails every one of them.
        let (i1, j1, k1, i2, j2, k2) = if x0 >= y0 {
            if y0 >= z0 {
                (1, 0, 0, 1, 1, 0)
            } else if x0 >= z0 {
                (1, 0, 0, 1, 0, 1)
            } else {
                (0, 0, 1, 1, 0, 1)
            }
        } else if y0 < z0 {
            (0, 0, 1, 0, 1, 1)
        } else if x0 < z0 {
            (0, 1, 0, 0, 1, 1)
        } else {
            (0, 1, 0, 1, 1, 0)
        };
        let x1 = (x0 - f64::from(i1)) + G3;
        let y1 = (y0 - f64::from(j1)) + G3;
        let z1 = (z0 - f64::from(k1)) + G3;
        let x2 = (x0 - f64::from(i2)) + F3;
        let y2 = (y0 - f64::from(j2)) + F3;
        let z2 = (z0 - f64::from(k2)) + F3;
        let x3 = (x0 - 1.0) + 0.5;
        let y3 = (y0 - 1.0) + 0.5;
        let z3 = (z0 - 1.0) + 0.5;
        let (ii, jj, kk) = (i & 255, j & 255, k & 255);
        let g =
            |di: i32, dj: i32, dk: i32| self.p(ii + di + self.p(jj + dj + self.p(kk + dk))) % 12;
        let n0 = corner(g(0, 0, 0), x0, y0, z0, 0.6);
        let n1 = corner(g(i1, j1, k1), x1, y1, z1, 0.6);
        let n2 = corner(g(i2, j2, k2), x2, y2, z2, 0.6);
        let n3 = corner(g(1, 1, 1), x3, y3, z3, 0.6);
        32.0 * (((n0 + n1) + n2) + n3)
    }

    #[cfg(test)]
    fn with_identity_perms() -> Self {
        SimplexNoise {
            perms: std::array::from_fn(|i| i as u8),
            offset: [0.0; 3],
        }
    }
}

/// `SimplexNoise.getCornerNoise3D`.
fn corner(g: i32, x: f64, y: f64, z: f64, base: f64) -> f64 {
    let t = ((base - x * x) - y * y) - z * z;
    // dcmpg/ifge: NaN goes on to the else branch.
    if t < 0.0 {
        0.0
    } else {
        let t = t * t;
        (t * t) * dot(g, x, y, z)
    }
}

/// `SimplexNoise.dot(int[], double, double, double)`.
fn dot(g: i32, x: f64, y: f64, z: f64) -> f64 {
    let [gx, gy, gz] = GRADIENT[g as usize];
    (f64::from(gx) * x + f64::from(gy) * y) + f64::from(gz) * z
}

#[cfg(test)]
mod tests {
    use spyglass_rng::LegacyRandomSource;

    use super::*;

    #[test]
    fn draws_as_26_3_simplex_does() {
        let mut a = LegacyRandomSource::new(7);
        let mut b = LegacyRandomSource::new(7);
        let old = SimplexNoise::new(&mut a);
        let new = crate::SimplexNoise::new(&mut b, false);
        assert_eq!(old.offset(), new.offset());
        assert_eq!(a.next_long(), b.next_long());
        // 26.3's End noise is made with zero offsets because 26.2 never adds them: the same
        // permutation gives the same sample, narrowed.
        let zero = crate::SimplexNoise::new(&mut LegacyRandomSource::new(7), true);
        for (x, y) in [(0.3, 0.7), (-1234.5, 99.0), (12.0, 12.0)] {
            assert_eq!(old.get_value_2d(x, y) as f32, zero.get(x, y), "({x}, {y})");
        }
    }

    #[test]
    fn values_2d_with_identity_permutations() {
        // The same points as 26.3's hand-worked test, which narrows these to f32.
        let noise = SimplexNoise::with_identity_perms();
        for (x, y, bits) in [
            (0.3, 0.7, 0xbf65_0b6f),
            (5.25, -3.5, 0xbd77_1290),
            (-100.0, 42.0, 0xbf5c_dbcd),
        ] {
            let v = noise.get_value_2d(x, y);
            assert_eq!((v as f32).to_bits(), bits, "({x}, {y})");
        }
    }

    /// At the origin, worked by hand with `p[i] = i`: `s = 0`, so `i = j = k = 0`, `t = 0` and
    /// `(x0, y0, z0) = (0, 0, 0)`, taking the first branch (1,0,0 then 1,1,0). Corner 0's dot
    /// is 0. Corner 1 is (−5/6, 1/6, 1/6), r² = 0.75; corner 2 (−2/3, −2/3, 1/3), r² = 1;
    /// corner 3 (−1/2, −1/2, −1/2), r² = 0.75: all beyond the 0.6 base, so 0.
    #[test]
    fn value_3d_at_the_origin() {
        let noise = SimplexNoise::with_identity_perms();
        assert_eq!(noise.get_value(0.0, 0.0, 0.0), 0.0);
    }

    #[test]
    fn value_3d_one_corner_by_hand() {
        // At (0.1, 0, 0): s = 0.1/3, all floors 0, t = 0; x0 = 0.1, y0 = z0 = 0, so
        // (1,0,0) and (1,1,0). Only corner 0 is near: r² = 0.01, t = 0.59, and its gradient
        // is GRADIENT[p(0 + p(0 + p(0))) % 12] = (1,1,0), so dot = 0.1.
        // Corner 1 is (0.1 − 1 + 1/6, 1/6, 1/6), r² ≈ 0.593, t ≈ 0.0067: small but non-zero.
        let noise = SimplexNoise::with_identity_perms();
        let x0: f64 = 0.1;
        let t0: f64 = ((0.6 - x0 * x0) - 0.0) - 0.0;
        let n0 = (t0 * t0) * (t0 * t0) * x0;
        let (x1, y1, z1) = ((x0 - 1.0) + G3, (0.0 - 0.0) + G3, (0.0 - 0.0) + G3);
        let t1: f64 = ((0.6 - x1 * x1) - y1 * y1) - z1 * z1;
        // Corner 1's gradient: p(1 + p(0 + p(0))) % 12 = 1, (−1, 1, 0).
        let n1 = (t1 * t1) * (t1 * t1) * (-x1 + y1);
        assert_eq!(
            noise.get_value(0.1, 0.0, 0.0),
            32.0 * (((n0 + n1) + 0.0) + 0.0)
        );
    }
}
