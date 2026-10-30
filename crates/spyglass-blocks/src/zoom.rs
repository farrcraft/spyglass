//! `BiomeManager.getBiome`: which quart a block takes its biome from. Each block looks at the
//! eight quarts around it, displaced by a "fiddle" seeded from the world seed, and takes the
//! nearest (`docs/specs/material.md#biomes`).

use sha2::{Digest, Sha256};

/// `BiomeManager.obfuscateSeed`: Guava's `Hashing.sha256().hashLong(seed).asLong()`, the first
/// eight bytes of the SHA-256 of the seed's eight bytes, both little-endian.
pub fn obfuscate_seed(seed: i64) -> i64 {
    let digest = Sha256::digest(seed.to_le_bytes());
    let mut first = [0; 8];
    first.copy_from_slice(&digest[..8]);
    i64::from_le_bytes(first)
}

/// `LinearCongruentialGenerator.next`.
fn lcg(a: i64, c: i64) -> i64 {
    a.wrapping_mul(
        a.wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407),
    )
    .wrapping_add(c)
}

/// `getFiddle`: a displacement in `[-0.45, 0.45)`.
fn fiddle(l: i64) -> f64 {
    let d = f64::from((l >> 24).rem_euclid(1024) as i32) / 1024.0;
    (d - 0.5) * 0.9
}

/// `getFiddledDistance`: the squared distance from a block's place in its quart to the
/// displaced corner quart `(qx, qy, qz)`.
fn fiddled_distance(seed: i64, [qx, qy, qz]: [i32; 3], [x, y, z]: [f64; 3]) -> f64 {
    let mut l = seed;
    for q in [qx, qy, qz, qx, qy, qz] {
        l = lcg(l, i64::from(q));
    }
    let fx = fiddle(l);
    l = lcg(l, seed);
    let fy = fiddle(l);
    l = lcg(l, seed);
    let fz = fiddle(l);
    let square = |d: f64| d * d;
    (square(z + fz) + square(y + fy)) + square(x + fx)
}

/// A world's biome zoom: its obfuscated seed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BiomeZoom {
    seed: i64,
}

impl BiomeZoom {
    /// The zoom of the world with `seed`.
    pub fn new(seed: i64) -> Self {
        BiomeZoom {
            seed: obfuscate_seed(seed),
        }
    }

    /// The quart block `(x, y, z)` takes its biome from: of the eight quarts at and above
    /// `((x − 2) >> 2, (y − 2) >> 2, (z − 2) >> 2)`, the one at the smallest fiddled distance,
    /// the first in order on ties.
    pub fn quart(&self, [x, y, z]: [i32; 3]) -> [i32; 3] {
        let [i, j, k] = [x, y, z].map(|c| c.wrapping_sub(2));
        let q = [i >> 2, j >> 2, k >> 2];
        let frac = [i, j, k].map(|c| f64::from(c & 3) / 4.0);
        let mut best = 0;
        let mut best_distance = f64::INFINITY;
        for corner in 0..8 {
            let up = [corner & 4 != 0, corner & 2 != 0, corner & 1 != 0];
            let quart = std::array::from_fn(|d| if up[d] { q[d] + 1 } else { q[d] });
            let offset = std::array::from_fn(|d| if up[d] { frac[d] - 1.0 } else { frac[d] });
            let distance = fiddled_distance(self.seed, quart, offset);
            // dcmpl/ifle: only a strictly smaller distance replaces the best.
            if best_distance > distance {
                best = corner;
                best_distance = distance;
            }
        }
        std::array::from_fn(|d| q[d] + i32::from(best >> (2 - d) & 1 != 0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn obfuscated_seeds_are_guavas() {
        // Hashing.sha256().hashLong(seed).asLong(), from the game's own Guava 33.6.
        for (seed, hashed) in [
            (1, -6_467_378_160_175_308_932),
            (-1, 6_759_447_113_877_070_610),
            (12_345, 293_737_985_876_514_017),
            (i64::MIN, 6_374_347_445_474_471_398),
        ] {
            assert_eq!(obfuscate_seed(seed), hashed, "seed {seed}");
        }
    }

    #[test]
    fn a_block_takes_one_of_the_eight_quarts_around_it() {
        let zoom = BiomeZoom::new(12_345);
        for x in -40..40 {
            for z in [-77, -1, 0, 5, 1000] {
                for y in [-64, 0, 63, 319] {
                    let [qx, qy, qz] = zoom.quart([x, y, z]);
                    let near = |b: i32, q: i32| ((b - 2) >> 2..=((b - 2) >> 2) + 1).contains(&q);
                    assert!(near(x, qx) && near(y, qy) && near(z, qz), "({x}, {y}, {z})");
                }
            }
        }
    }
}
