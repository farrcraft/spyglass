//! `GradientNoise` and `PerlinNoise`: one octave of 3D gradient noise.

use spyglass_rng::RandomSource;

use crate::mth;

/// `GradientNoise.GRADIENT`: the 16 gradient vectors (12 cube edges, 4 repeated).
const GRADIENT: [[i32; 3]; 16] = [
    [1, 1, 0],
    [-1, 1, 0],
    [1, -1, 0],
    [-1, -1, 0],
    [1, 0, 1],
    [-1, 0, 1],
    [1, 0, -1],
    [-1, 0, -1],
    [0, 1, 1],
    [0, -1, 1],
    [0, 1, -1],
    [0, -1, -1],
    [1, 1, 0],
    [0, -1, 1],
    [-1, 1, 0],
    [0, -1, -1],
];

/// `GradientNoise.ROUND_OFF`: 2^25.
const ROUND_OFF: f64 = 33_554_432.0;
/// `GradientNoise.HALF_ROUND_OFF`: `Math.nextDown(2^24)`.
const HALF_ROUND_OFF: f64 = f64::from_bits(0x416f_ffff_ffff_ffff);

/// `GradientNoise.wrap(double)`: brings far coordinates back near the origin, so that `f64`
/// precision holds (the "far lands" fix).
pub fn wrap(d: f64) -> f64 {
    // NaN is outside the range, so it takes the second branch, as in the bytecode.
    if (-HALF_ROUND_OFF..HALF_ROUND_OFF).contains(&d) {
        d
    } else {
        d - (d / ROUND_OFF + 0.5).floor() * ROUND_OFF
    }
}

/// `GradientNoise.gradDot(int, float, float, float)`.
fn grad_dot(hash: i32, x: f32, y: f32, z: f32) -> f32 {
    let [gx, gy, gz] = GRADIENT[(hash & 15) as usize];
    gx as f32 * x + gy as f32 * y + gz as f32 * z
}

/// `PerlinNoise` (with the state of `GradientNoise`): one octave.
#[derive(Debug, Clone)]
pub struct PerlinNoise {
    perms: [u8; 256],
    offset: [f64; 3],
}

impl PerlinNoise {
    /// `PerlinNoise(RandomSource)`: three offsets (`nextDouble() * 256`), then a Fisher–Yates
    /// shuffle of the permutation table.
    pub fn new(random: &mut impl RandomSource) -> Self {
        let offset = std::array::from_fn(|_| random.next_double() * 256.0);
        let mut perms: [u8; 256] = std::array::from_fn(|i| i as u8);
        for i in 0..256 {
            let j = random.next_int_bounded(256 - i as i32) as usize + i;
            perms.swap(i, j);
        }
        PerlinNoise { perms, offset }
    }

    /// `offsetX`, `offsetY` and `offsetZ`.
    pub fn offset(&self) -> [f64; 3] {
        self.offset
    }

    /// The permutation table, as signed bytes like the game's `byte[] perms`.
    pub fn perm(&self, i: usize) -> i8 {
        self.perms[i] as i8
    }

    /// `GradientNoise.permute(int)`.
    fn permute(&self, i: i32) -> i32 {
        i32::from(self.perms[(i & 255) as usize])
    }

    /// `get(double, double, double)`.
    pub fn get(&self, x: f64, y: f64, z: f64) -> f32 {
        let x = wrap(x) + self.offset[0];
        let y = wrap(y) + self.offset[1];
        let z = wrap(z) + self.offset[2];
        let (xi, yi, zi) = (mth::floor(x), mth::floor(y), mth::floor(z));
        let xf = (x - f64::from(xi)) as f32;
        let yf = (y - f64::from(yi)) as f32;
        let zf = (z - f64::from(zi)) as f32;
        self.sample_and_lerp(xi, yi, zi, xf, yf, zf, yf)
    }

    /// `get(double, double)`: the 3D noise at `y = 0`, with both coordinates wrapped first, as
    /// the game does (they're wrapped again inside, which changes nothing).
    pub fn get_2d(&self, x: f64, z: f64) -> f32 {
        self.get(wrap(x), 0.0, wrap(z))
    }

    /// `sampleAndLerp(int, int, int, float, float, float, float)`. `y_smooth` is the fraction
    /// that drives the y smoothstep; the game passes `yf` again.
    #[allow(
        clippy::too_many_arguments,
        reason = "mirrors the game's sampleAndLerp"
    )]
    fn sample_and_lerp(
        &self,
        xi: i32,
        yi: i32,
        zi: i32,
        xf: f32,
        yf: f32,
        zf: f32,
        y_smooth: f32,
    ) -> f32 {
        let h0 = self.permute(xi);
        let h1 = self.permute(xi.wrapping_add(1));
        // Hashes of the cell's corners: h<x><y>, x and y offsets 0 or 1. Java int additions,
        // which wrap; permute masks the sum.
        let add = |p: i32, q: i32| p.wrapping_add(q);
        let h00 = self.permute(add(h0, yi));
        let h01 = self.permute(add(add(h0, yi), 1));
        let h10 = self.permute(add(h1, yi));
        let h11 = self.permute(add(add(h1, yi), 1));
        let v000 = grad_dot(self.permute(add(h00, zi)), xf, yf, zf);
        let v100 = grad_dot(self.permute(add(h10, zi)), xf - 1.0, yf, zf);
        let v010 = grad_dot(self.permute(add(h01, zi)), xf, yf - 1.0, zf);
        let v110 = grad_dot(self.permute(add(h11, zi)), xf - 1.0, yf - 1.0, zf);
        let v001 = grad_dot(self.permute(add(add(h00, zi), 1)), xf, yf, zf - 1.0);
        let v101 = grad_dot(self.permute(add(add(h10, zi), 1)), xf - 1.0, yf, zf - 1.0);
        let v011 = grad_dot(self.permute(add(add(h01, zi), 1)), xf, yf - 1.0, zf - 1.0);
        let v111 = grad_dot(
            self.permute(add(add(h11, zi), 1)),
            xf - 1.0,
            yf - 1.0,
            zf - 1.0,
        );
        let tx = mth::smoothstep(xf);
        let ty = mth::smoothstep(y_smooth);
        let tz = mth::smoothstep(zf);
        mth::lerp3(tx, ty, tz, v000, v100, v010, v110, v001, v101, v011, v111)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spyglass_rng::XoroshiroRandomSource;

    #[test]
    fn half_round_off_is_next_down_of_2_24() {
        assert_eq!(
            HALF_ROUND_OFF,
            f64::from_bits((16_777_216.0_f64).to_bits() - 1)
        );
    }

    #[test]
    fn wrap_keeps_near_values_and_folds_far_ones() {
        assert_eq!(wrap(123.5), 123.5);
        // -2^24 is just outside the range, and folds back onto itself.
        assert_eq!(wrap(-16_777_216.0), -16_777_216.0);
        assert_eq!(wrap(3e7), 3e7 - ROUND_OFF);
        assert_eq!(wrap(ROUND_OFF + 1.5), 1.5);
        assert!(wrap(f64::NAN).is_nan());
    }

    #[test]
    fn permutation_is_a_permutation() {
        let noise = PerlinNoise::new(&mut XoroshiroRandomSource::new(7));
        let mut seen = [false; 256];
        for &p in &noise.perms {
            seen[p as usize] = true;
        }
        assert!(seen.iter().all(|&s| s));
        assert!(noise.offset.iter().all(|o| (0.0..256.0).contains(o)));
    }
}
