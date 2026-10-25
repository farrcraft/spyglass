//! Slime chunks (`Slime.checkSlimeSpawnRules`).

use spyglass_rng::{RandomSource, worldgen};

/// The salt the slime spawn rule passes to `WorldgenRandom.seedSlimeChunk`.
const SLIME_SALT: i64 = 987_234_911;

/// Whether chunk `(x, z)` is a slime chunk for `seed`: `seedSlimeChunk(x, z, seed, 987234911)`
/// then `nextInt(10) == 0`.
pub fn is_slime_chunk(seed: i64, x: i32, z: i32) -> bool {
    worldgen::slime_chunk(x, z, seed, SLIME_SALT).next_int_bounded(10) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn about_one_chunk_in_ten() {
        let count = (0..100)
            .flat_map(|x| (0..100).map(move |z| (x, z)))
            .filter(|&(x, z)| is_slime_chunk(12345, x, z))
            .count();
        assert!((900..1100).contains(&count), "{count}");
    }
}
