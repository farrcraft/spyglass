//! `WorldgenRandom`'s seeding methods, for structures and slime chunks.
//!
//! `WorldgenRandom` wraps a random source and delegates `next(bits)` to it when it's a
//! `LegacyRandomSource`, which is the only kind these methods are used with. So each method here
//! returns that `LegacyRandomSource`, seeded as the game seeds it; drawing from it gives the
//! same values as drawing from the `WorldgenRandom`.

use crate::legacy::LegacyRandomSource;
use crate::source::RandomSource;

/// `WorldgenRandom.setLargeFeatureSeed(seed, chunkX, chunkZ)`.
pub fn large_feature(seed: i64, chunk_x: i32, chunk_z: i32) -> LegacyRandomSource {
    let mut random = LegacyRandomSource::new(seed);
    let a = random.next_long();
    let b = random.next_long();
    LegacyRandomSource::new(
        (i64::from(chunk_x).wrapping_mul(a) ^ i64::from(chunk_z).wrapping_mul(b)) ^ seed,
    )
}

/// `WorldgenRandom.setLargeFeatureWithSalt(seed, x, z, salt)`.
pub fn large_feature_with_salt(seed: i64, x: i32, z: i32, salt: i32) -> LegacyRandomSource {
    LegacyRandomSource::new(
        i64::from(x)
            .wrapping_mul(341_873_128_712)
            .wrapping_add(i64::from(z).wrapping_mul(132_897_987_541))
            .wrapping_add(seed)
            .wrapping_add(i64::from(salt)),
    )
}

/// `WorldgenRandom.seedSlimeChunk(chunkX, chunkZ, seed, salt)`. The products of `x` and `z`
/// with the `int` constants are taken in 32 bits, as the game does; only `z * z` is widened
/// before its multiply.
pub fn slime_chunk(chunk_x: i32, chunk_z: i32, seed: i64, salt: i64) -> LegacyRandomSource {
    let (x, z) = (chunk_x, chunk_z);
    let mixed = seed
        .wrapping_add(i64::from(x.wrapping_mul(x).wrapping_mul(4_987_142)))
        .wrapping_add(i64::from(x.wrapping_mul(5_947_611)))
        .wrapping_add(i64::from(z.wrapping_mul(z)).wrapping_mul(4_392_871))
        .wrapping_add(i64::from(z.wrapping_mul(389_711)));
    LegacyRandomSource::new(mixed ^ salt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn large_feature_mixes_both_longs() {
        // At chunk (0, 0) only the seed is left: a new source with the same seed.
        let mut a = large_feature(42, 0, 0);
        let mut b = LegacyRandomSource::new(42);
        assert_eq!(a.next_long(), b.next_long());
        // From java.util.Random(42): nextLong() twice, as the formula takes them.
        let (l1, l2) = (
            -5_025_562_857_975_149_833_i64,
            -5_843_495_416_241_995_736_i64,
        );
        let mut c = large_feature(42, 3, -7);
        let mut d =
            LegacyRandomSource::new((3_i64.wrapping_mul(l1) ^ (-7_i64).wrapping_mul(l2)) ^ 42);
        assert_eq!(c.next_long(), d.next_long());
    }

    #[test]
    fn salted_seed_wraps() {
        let mut a = large_feature_with_salt(i64::MAX, i32::MAX, i32::MIN, 1);
        let expected = i64::from(i32::MAX)
            .wrapping_mul(341_873_128_712)
            .wrapping_add(i64::from(i32::MIN).wrapping_mul(132_897_987_541))
            .wrapping_add(i64::MAX)
            .wrapping_add(1);
        assert_eq!(a.next_long(), LegacyRandomSource::new(expected).next_long());
    }

    #[test]
    fn slime_products_overflow_in_32_bits() {
        // x * x * 4987142 overflows an int for x = 1000; the game keeps the wrapped int.
        let x = 1000_i32;
        let wrapped = i64::from(x.wrapping_mul(x).wrapping_mul(4_987_142));
        assert_ne!(wrapped, 1000_i64 * 1000 * 4_987_142);
        let mut a = slime_chunk(x, 0, 0, 0);
        let mut b =
            LegacyRandomSource::new(wrapped.wrapping_add(i64::from(x.wrapping_mul(5_947_611))));
        assert_eq!(a.next_long(), b.next_long());
    }
}
