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
fn lcg(seed: i64, increment: i64) -> i64 {
    seed.wrapping_mul(
        seed.wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407),
    )
    .wrapping_add(increment)
}

/// `getFiddle`: a displacement in `[-0.45, 0.45)`, from ten bits of `hash`.
fn fiddle(hash: i64) -> f64 {
    let fraction = f64::from((hash >> 24).rem_euclid(1024) as i32) / 1024.0;
    (fraction - 0.5) * 0.9
}

/// `getFiddledDistance`: the squared distance from a block's place in its quart to the
/// displaced corner quart `(qx, qy, qz)`.
fn fiddled_distance(seed: i64, [qx, qy, qz]: [i32; 3], [x, y, z]: [f64; 3]) -> f64 {
    // A hash of the quart, then one displacement on each axis from it.
    let mut hash = seed;
    for q in [qx, qy, qz, qx, qy, qz] {
        hash = lcg(hash, i64::from(q));
    }
    let fx = fiddle(hash);
    hash = lcg(hash, seed);
    let fy = fiddle(hash);
    hash = lcg(hash, seed);
    let fz = fiddle(hash);
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
        // Shifted by half a quart, so the quarts' centres are the corners around the block.
        let shifted = [x, y, z].map(|c| c.wrapping_sub(2));
        let base = shifted.map(|c| c >> 2);
        // The block's place between `base` and the quarts above, in quarts.
        let within = shifted.map(|c| f64::from(c & 3) / 4.0);
        let mut best = 0;
        let mut best_distance = f64::INFINITY;
        for corner in 0..8 {
            // The corner's bits pick, x then y then z, the quart above `base` on that axis.
            let up = [corner & 4 != 0, corner & 2 != 0, corner & 1 != 0];
            let quart = std::array::from_fn(|d| if up[d] { base[d] + 1 } else { base[d] });
            let offset = std::array::from_fn(|d| if up[d] { within[d] - 1.0 } else { within[d] });
            let distance = fiddled_distance(self.seed, quart, offset);
            // dcmpl/ifle: only a strictly smaller distance replaces the best.
            if best_distance > distance {
                best = corner;
                best_distance = distance;
            }
        }
        std::array::from_fn(|d| base[d] + i32::from(best >> (2 - d) & 1 != 0))
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
