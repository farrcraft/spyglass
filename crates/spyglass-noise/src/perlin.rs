//! `GradientNoise` and `PerlinNoise`: one octave of 3D gradient noise.

use spyglass_rng::RandomSource;

use crate::mth;
use crate::volume::Volume;

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

    /// `addToVolume(DensityBuffer, DensityVolume, double, double, float)`: adds
    /// `factor · noise` at every position of `volume` to `buffer`.
    ///
    /// Not the point path in a loop: each coordinate is `wrap(block · scale) + offset`, with the
    /// caller's scale already multiplied by any octave frequency, and each corner's gradient dot
    /// is `(gx·xf + gz·zf) + gy·yf`.
    pub fn add_to_volume(
        &self,
        buffer: &mut [f32],
        volume: &Volume,
        xz_scale: f64,
        y_scale: f64,
        factor: f32,
    ) {
        self.add_to_volume_with(buffer, volume, xz_scale, y_scale, factor, |_, yw, yi| {
            let yf = (yw - f64::from(yi)) as f32;
            (yf, yf)
        });
    }

    /// The loop both Perlin noises' `addToVolume` share. `fractions(y, yw, yi)` gives, from the
    /// unwrapped coordinate `y`, the wrapped and offset `yw` and its floor, the y fraction the
    /// gradients see and the one the smoothstep sees.
    fn add_to_volume_with(
        &self,
        buffer: &mut [f32],
        volume: &Volume,
        xz_scale: f64,
        y_scale: f64,
        factor: f32,
        fractions: impl Fn(f64, f64, i32) -> (f32, f32),
    ) {
        let [ox, oy, oz] = self.offset;
        let grad = |i: i32| GRADIENT[(self.permute(i) & 15) as usize];
        let mut index = 0;
        for iz in 0..volume.size[2] {
            let zw = wrap(f64::from(volume.block_z(iz)) * xz_scale) + oz;
            let zi = mth::floor(zw);
            let zf = (zw - f64::from(zi)) as f32;
            let tz = mth::smoothstep(zf);
            for ix in 0..volume.size[0] {
                let xw = wrap(f64::from(volume.block_x(ix)) * xz_scale) + ox;
                let xi = mth::floor(xw);
                let xf = (xw - f64::from(xi)) as f32;
                let h0 = self.permute(xi);
                let h1 = self.permute(xi.wrapping_add(1));
                let tx = mth::smoothstep(xf);
                // Each corner's dotXz and gradient y, recomputed only when the y cell changes.
                let mut last_yi = i32::MIN;
                let mut corners = [(0.0_f32, 0.0_f32); 8];
                for iy in 0..volume.size[1] {
                    let y = f64::from(volume.block_y(iy)) * y_scale;
                    let yw = wrap(y) + oy;
                    let yi = mth::floor(yw);
                    let (yf, y_smooth) = fractions(y, yw, yi);
                    let ty = mth::smoothstep(y_smooth);
                    if last_yi != yi {
                        let add = |p: i32, q: i32| p.wrapping_add(q);
                        let h00 = self.permute(add(h0, yi));
                        let h01 = self.permute(add(add(h0, yi), 1));
                        let h10 = self.permute(add(h1, yi));
                        let h11 = self.permute(add(add(h1, yi), 1));
                        // Corners in lerp3's order: x fastest, then y, then z.
                        let hashes = [
                            (add(h00, zi), xf, zf),
                            (add(h10, zi), xf - 1.0, zf),
                            (add(h01, zi), xf, zf),
                            (add(h11, zi), xf - 1.0, zf),
                            (add(add(h00, zi), 1), xf, zf - 1.0),
                            (add(add(h10, zi), 1), xf - 1.0, zf - 1.0),
                            (add(add(h01, zi), 1), xf, zf - 1.0),
                            (add(add(h11, zi), 1), xf - 1.0, zf - 1.0),
                        ];
                        corners = hashes.map(|(h, cx, cz)| {
                            let [gx, gy, gz] = grad(h);
                            (gx as f32 * cx + gz as f32 * cz, gy as f32)
                        });
                        last_yi = yi;
                    }
                    let c = |i: usize, fy: f32| corners[i].0 + corners[i].1 * fy;
                    let y1 = yf - 1.0;
                    let value = mth::lerp3(
                        tx,
                        ty,
                        tz,
                        c(0, yf),
                        c(1, yf),
                        c(2, y1),
                        c(3, y1),
                        c(4, yf),
                        c(5, yf),
                        c(6, y1),
                        c(7, y1),
                    );
                    buffer[index] += factor * value;
                    index += 1;
                }
            }
        }
    }

    /// `sampleAndLerp(int, int, int, float, float, float, float)`. `y_smooth` is the fraction
    /// that drives the y smoothstep; plain Perlin passes `yf` again, smeared Perlin doesn't.
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

/// `SmearedPerlinNoise.SHIFT_UP_EPSILON` (`1.0E-7f`), widened as `computeFudgeY` adds it.
const SHIFT_UP_EPSILON: f64 = 1.000_000_011_686_097_4e-7;

/// `SmearedPerlinNoise`: Perlin noise whose y gradient input is shifted down to a multiple of a
/// scale, so the noise smears along y. Old blended noise is built from it.
#[derive(Debug, Clone)]
pub struct SmearedPerlinNoise {
    noise: PerlinNoise,
    fudge_y_scale: f64,
}

impl SmearedPerlinNoise {
    /// `SmearedPerlinNoise(RandomSource, double)`: a [`PerlinNoise`] from `random`, consuming it
    /// the same way.
    pub fn new(random: &mut impl RandomSource, fudge_y_scale: f64) -> Self {
        SmearedPerlinNoise {
            noise: PerlinNoise::new(random),
            fudge_y_scale,
        }
    }

    /// The underlying Perlin noise.
    pub fn perlin(&self) -> &PerlinNoise {
        &self.noise
    }

    /// `fudgeYScale`.
    pub fn fudge_y_scale(&self) -> f64 {
        self.fudge_y_scale
    }

    /// `get(double, double, double)`: as [`PerlinNoise::get`], but the gradients see
    /// `(float)(yf − computeFudgeY(y, yf))` while the y smoothstep still sees `(float) yf`.
    pub fn get(&self, x: f64, y: f64, z: f64) -> f32 {
        let [ox, oy, oz] = self.noise.offset;
        let xw = wrap(x) + ox;
        let yw = wrap(y) + oy;
        let zw = wrap(z) + oz;
        let (xi, yi, zi) = (mth::floor(xw), mth::floor(yw), mth::floor(zw));
        let xf = (xw - f64::from(xi)) as f32;
        let yd = yw - f64::from(yi);
        let zf = (zw - f64::from(zi)) as f32;
        let yf = (yd - self.compute_fudge_y(y, yd)) as f32;
        self.noise
            .sample_and_lerp(xi, yi, zi, xf, yf, zf, yd as f32)
    }

    /// `addToVolume`: as [`PerlinNoise::add_to_volume`], with the gradients' y fraction smeared
    /// as [`get`](Self::get) smears it, from the unwrapped `block · y_scale`.
    pub fn add_to_volume(
        &self,
        buffer: &mut [f32],
        volume: &Volume,
        xz_scale: f64,
        y_scale: f64,
        factor: f32,
    ) {
        self.noise
            .add_to_volume_with(buffer, volume, xz_scale, y_scale, factor, |y, yw, yi| {
                let yd = yw - f64::from(yi);
                ((yd - self.compute_fudge_y(y, yd)) as f32, yd as f32)
            });
    }

    /// `computeFudgeY(double, double)`: `y` itself when `0 ≤ y < yd` (the input before
    /// wrapping), else `yd`, rounded down to a multiple of the scale after a small upward shift.
    fn compute_fudge_y(&self, y: f64, yd: f64) -> f64 {
        let r = if y >= 0.0 && y < yd { y } else { yd };
        f64::from(mth::floor(r / self.fudge_y_scale + SHIFT_UP_EPSILON)) * self.fudge_y_scale
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
    fn shift_up_epsilon_is_the_float_widened() {
        assert_eq!(SHIFT_UP_EPSILON, f64::from(1.0e-7_f32));
    }

    #[test]
    fn smeared_noise_consumes_the_random_as_perlin_does() {
        let plain = PerlinNoise::new(&mut XoroshiroRandomSource::new(3));
        let smeared = SmearedPerlinNoise::new(&mut XoroshiroRandomSource::new(3), 8.0);
        assert_eq!(plain.offset, smeared.perlin().offset);
        assert_eq!(plain.perms, smeared.perlin().perms);
    }

    #[test]
    fn fudge_rounds_down_to_the_scale() {
        let smeared = SmearedPerlinNoise::new(&mut XoroshiroRandomSource::new(3), 0.5);
        // 0 ≤ y < yd: y itself is rounded, 1.3 → 1.0.
        assert_eq!(smeared.compute_fudge_y(1.3, 1.9), 1.0);
        // Otherwise yd is: 0.7 → 0.5.
        assert_eq!(smeared.compute_fudge_y(-4.0, 0.7), 0.5);
        assert_eq!(smeared.compute_fudge_y(f64::NAN, 0.7), 0.5);
        // The epsilon lifts values just under a multiple onto it.
        assert_eq!(smeared.compute_fudge_y(-1.0, 0.499_999_99), 0.5);
    }

    /// At a scale of 1 the volume path's coordinates are the point path's, and its dots differ
    /// only in the sign of an exact zero, so the values agree.
    #[test]
    fn volume_matches_points_at_unit_scale() {
        let plain = PerlinNoise::new(&mut XoroshiroRandomSource::new(11));
        let smeared = SmearedPerlinNoise::new(&mut XoroshiroRandomSource::new(12), 0.75);
        let volume = Volume::new([3, 7, 2], [-5, -3, 1000], [2, 3, 5]);
        for factor in [1.0, 0.5] {
            let mut a = vec![10.0; volume.len()];
            let mut b = vec![10.0; volume.len()];
            plain.add_to_volume(&mut a, &volume, 1.0, 1.0, factor);
            smeared.add_to_volume(&mut b, &volume, 1.0, 1.0, factor);
            for iz in 0..2 {
                for ix in 0..3 {
                    for iy in 0..7 {
                        let i = volume.index(ix, iy, iz);
                        let (x, y, z) = (
                            f64::from(volume.block_x(ix)),
                            f64::from(volume.block_y(iy)),
                            f64::from(volume.block_z(iz)),
                        );
                        assert_eq!(a[i], 10.0 + factor * plain.get(x, y, z));
                        assert_eq!(b[i], 10.0 + factor * smeared.get(x, y, z));
                    }
                }
            }
        }
    }

    /// The volume path multiplies the scale into the coordinate before wrapping, so a scale that
    /// isn't a power of two can round differently from scaling the point's coordinate.
    #[test]
    fn volume_scales_blocks_directly() {
        let noise = PerlinNoise::new(&mut XoroshiroRandomSource::new(5));
        let volume = Volume::blocks([1, 1, 1], [12345, 67, -891]);
        let mut buffer = [0.0];
        let scale = 0.666_666_666_666_666_6;
        noise.add_to_volume(&mut buffer, &volume, scale, scale, 1.0);
        let want = noise.get(12345.0 * scale, 67.0 * scale, -891.0 * scale);
        assert_eq!(buffer[0], want);
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
