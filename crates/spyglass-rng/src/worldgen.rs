//! `WorldgenRandom`: the seeding methods for structures and slime chunks, and the random
//! decoration draws from.
//!
//! `WorldgenRandom` wraps a random source and delegates `next(bits)` to it when it's a
//! `LegacyRandomSource`. Structures and slime chunks only use it that way, so their methods
//! here return that `LegacyRandomSource`, seeded as the game seeds it; drawing from it gives
//! the same values as drawing from the `WorldgenRandom`. Decoration wraps a
//! `XoroshiroRandomSource`, which [`WorldgenRandom`] is.

use crate::bits::{self, Bits};
use crate::gaussian::Gaussian;
use crate::legacy::LegacyRandomSource;
use crate::source::RandomSource;
use crate::xoroshiro::{XoroshiroPositionalRandomFactory, XoroshiroRandomSource};

/// `WorldgenRandom` over a `XoroshiroRandomSource`, as `applyBiomeDecoration` makes it
/// (`docs/specs/rng.md`). It's a `LegacyRandomSource` whose `next(bits)` is the top `bits` of
/// the inner source's `nextLong()`, so every other draw is the legacy one built on `next`.
#[derive(Debug, Clone)]
pub struct WorldgenRandom {
    /// The source every bit comes from, and what seeding reaches.
    inner: XoroshiroRandomSource,
    /// The legacy source's own Gaussian state. `setSeed` reaches only the inner source, so it
    /// never resets this.
    gaussian: Gaussian,
}

impl WorldgenRandom {
    /// `new WorldgenRandom(new XoroshiroRandomSource(seed))`.
    pub fn new(seed: i64) -> Self {
        WorldgenRandom {
            inner: XoroshiroRandomSource::new(seed),
            gaussian: Gaussian::default(),
        }
    }

    /// `setSeed(long)`: the inner source's.
    pub fn set_seed(&mut self, seed: i64) {
        self.inner.set_seed(seed);
    }

    /// `setDecorationSeed(seed, x, z)`, with the chunk's minimum block x and z. Returns the
    /// decoration seed, which the random is then seeded with.
    pub fn set_decoration_seed(&mut self, seed: i64, x: i32, z: i32) -> i64 {
        self.set_seed(seed);
        // As in `large_feature`, two longs from the world seed weight x and z (the game's
        // `a` and `b`, made odd).
        let x_weight = self.next_long() | 1;
        let z_weight = self.next_long() | 1;
        let decoration = i64::from(x)
            .wrapping_mul(x_weight)
            .wrapping_add(i64::from(z).wrapping_mul(z_weight))
            ^ seed;
        self.set_seed(decoration);
        decoration
    }

    /// `setFeatureSeed(decorationSeed, index, step)`: `10000 · step` is taken as an `int`.
    pub fn set_feature_seed(&mut self, decoration: i64, index: i32, step: i32) {
        self.set_seed(
            decoration
                .wrapping_add(i64::from(index))
                .wrapping_add(i64::from(10_000_i32.wrapping_mul(step))),
        );
    }
}

impl Bits for WorldgenRandom {
    fn next(&mut self, bits: u32) -> i32 {
        ((self.inner.next_long() as u64) >> (64 - bits)) as i32
    }
}

impl RandomSource for WorldgenRandom {
    type Positional = XoroshiroPositionalRandomFactory;
    type Fork = XoroshiroRandomSource;

    /// The inner source's fork, not a `WorldgenRandom`.
    fn fork(&mut self) -> XoroshiroRandomSource {
        self.inner.fork()
    }

    fn fork_positional(&mut self) -> XoroshiroPositionalRandomFactory {
        self.inner.fork_positional()
    }

    fn next_int(&mut self) -> i32 {
        self.next(32)
    }

    fn next_int_bounded(&mut self, bound: i32) -> i32 {
        bits::next_int_bounded(self, bound)
    }

    fn next_long(&mut self) -> i64 {
        bits::next_long(self)
    }

    fn next_boolean(&mut self) -> bool {
        bits::next_boolean(self)
    }

    fn next_float(&mut self) -> f32 {
        bits::next_float(self)
    }

    fn next_double(&mut self) -> f64 {
        bits::next_double(self)
    }

    fn next_gaussian(&mut self) -> f64 {
        let mut gaussian = self.gaussian;
        let value = gaussian.next(|| self.next_double());
        self.gaussian = gaussian;
        value
    }

    /// `RandomSource.consumeCount`: one `nextInt()` per count.
    fn consume_count(&mut self, count: i32) {
        for _ in 0..count {
            self.next_int();
        }
    }
}

/// `WorldgenRandom.setLargeFeatureSeed(seed, chunkX, chunkZ)`: a chunk's own seed, for the
/// structures that start in it.
///
/// Two longs drawn from the world seed weight the chunk's x and z, so neighbouring chunks get
/// unrelated seeds, and the world seed is mixed back in at the end.
pub fn large_feature(seed: i64, chunk_x: i32, chunk_z: i32) -> LegacyRandomSource {
    let mut random = LegacyRandomSource::new(seed);
    let x_weight = random.next_long();
    let z_weight = random.next_long();
    LegacyRandomSource::new(
        (i64::from(chunk_x).wrapping_mul(x_weight) ^ i64::from(chunk_z).wrapping_mul(z_weight))
            ^ seed,
    )
}

/// `WorldgenRandom.setLargeFeatureWithSalt(seed, x, z, salt)`: the seed for a structure set's
/// placement in region `(x, z)`, the salt keeping each set's draws apart. The constants are
/// the game's, and the sum wraps as Java's `long` does.
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
    fn worldgen_random_takes_the_top_bits_of_xoroshiro() {
        let mut ours = WorldgenRandom::new(-123);
        let mut inner = XoroshiroRandomSource::new(-123);
        // nextInt(): the top 32 bits of one long; nextLong(): two such halves.
        assert_eq!(ours.next_int(), (inner.next_long() as u64 >> 32) as i32);
        let hi = (inner.next_long() as u64 >> 32) as i32;
        let lo = (inner.next_long() as u64 >> 32) as i32;
        assert_eq!(
            ours.next_long(),
            (i64::from(hi) << 32).wrapping_add(i64::from(lo))
        );
        // nextFloat(): the top 24 bits of one long.
        assert_eq!(
            ours.next_float(),
            (inner.next_long() as u64 >> 40) as f32 / (1 << 24) as f32
        );
        // A power-of-two bound takes the top bits of next(31).
        assert_eq!(
            ours.next_int_bounded(16),
            ((16 * (inner.next_long() as u64 >> 33)) >> 31) as i32
        );
    }

    #[test]
    fn decoration_and_feature_seeds_follow_the_spec() {
        let (seed, x, z) = (987_654_321_i64, -160, 48);
        let mut r = WorldgenRandom::new(0);
        let decoration = r.set_decoration_seed(seed, x, z);
        let mut check = WorldgenRandom::new(seed);
        let a = check.next_long() | 1;
        let b = check.next_long() | 1;
        let expected = i64::from(x)
            .wrapping_mul(a)
            .wrapping_add(i64::from(z).wrapping_mul(b))
            ^ seed;
        assert_eq!(decoration, expected);
        assert_eq!(r.next_long(), WorldgenRandom::new(expected).next_long());
        // 10000 · step is an int product: it wraps for a step past 214748.
        r.set_feature_seed(decoration, 5, 300_000);
        let wrapped = i64::from(10_000_i32.wrapping_mul(300_000));
        assert_ne!(wrapped, 3_000_000_000);
        let feature = decoration.wrapping_add(5).wrapping_add(wrapped);
        assert_eq!(r.next_long(), WorldgenRandom::new(feature).next_long());
    }

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
