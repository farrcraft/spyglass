//! `LegacyRandomSource` and its positional factory: Java's 48-bit LCG, as in `java.util.Random`,
//! used where `legacy_random_source` is true and by older parts of worldgen.

use crate::gaussian::Gaussian;
use crate::source::{PositionalRandomFactory, RandomSource};

const MULTIPLIER: i64 = 0x5_DEEC_E66D;
const INCREMENT: i64 = 11;
const MODULUS_MASK: i64 = (1 << 48) - 1;
/// `BitRandomSource.FLOAT_MULTIPLIER`: 2^-24.
const FLOAT_MULTIPLIER: f32 = 5.960_464_5e-8;
/// `BitRandomSource.DOUBLE_MULTIPLIER`: 2^-53.
const DOUBLE_MULTIPLIER: f64 = 1.110_223_024_625_156_5e-16;

/// `LegacyRandomSource`, with the defaults it inherits from `BitRandomSource`.
#[derive(Debug, Clone)]
pub struct LegacyRandomSource {
    seed: i64,
    gaussian: Gaussian,
}

impl LegacyRandomSource {
    /// `LegacyRandomSource(long)`: the seed scrambled as `java.util.Random` does.
    pub fn new(seed: i64) -> Self {
        let mut source = LegacyRandomSource {
            seed: 0,
            gaussian: Gaussian::default(),
        };
        source.set_seed(seed);
        source
    }

    /// `setSeed(long)`.
    pub fn set_seed(&mut self, seed: i64) {
        self.seed = (seed ^ MULTIPLIER) & MODULUS_MASK;
        self.gaussian.reset();
    }

    /// `next(int)`: advance the LCG and return its top `bits` bits.
    pub fn next(&mut self, bits: u32) -> i32 {
        self.seed = self.seed.wrapping_mul(MULTIPLIER).wrapping_add(INCREMENT) & MODULUS_MASK;
        (self.seed >> (48 - bits)) as i32
    }
}

impl RandomSource for LegacyRandomSource {
    type Positional = LegacyPositionalRandomFactory;

    fn fork(&mut self) -> Self {
        LegacyRandomSource::new(self.next_long())
    }

    fn fork_positional(&mut self) -> LegacyPositionalRandomFactory {
        LegacyPositionalRandomFactory {
            seed: self.next_long(),
        }
    }

    fn next_int(&mut self) -> i32 {
        self.next(32)
    }

    fn next_int_bounded(&mut self, bound: i32) -> i32 {
        assert!(bound > 0, "Bound must be positive");
        if bound & (bound - 1) == 0 {
            return ((i64::from(bound) * i64::from(self.next(31))) >> 31) as i32;
        }
        loop {
            let bits = self.next(31);
            let value = bits % bound;
            // Rejects the top, partial range; the sum overflows exactly when it's hit.
            if bits.wrapping_sub(value).wrapping_add(bound - 1) >= 0 {
                return value;
            }
        }
    }

    fn next_long(&mut self) -> i64 {
        let hi = self.next(32);
        let lo = self.next(32);
        (i64::from(hi) << 32).wrapping_add(i64::from(lo))
    }

    fn next_boolean(&mut self) -> bool {
        self.next(1) != 0
    }

    fn next_float(&mut self) -> f32 {
        self.next(24) as f32 * FLOAT_MULTIPLIER
    }

    fn next_double(&mut self) -> f64 {
        let hi = self.next(26);
        let lo = self.next(27);
        ((i64::from(hi) << 27) + i64::from(lo)) as f64 * DOUBLE_MULTIPLIER
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

/// `LegacyRandomSource.LegacyPositionalRandomFactory`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LegacyPositionalRandomFactory {
    seed: i64,
}

impl LegacyPositionalRandomFactory {
    /// A factory from its seed, as `forkPositional` makes it.
    pub fn new(seed: i64) -> Self {
        LegacyPositionalRandomFactory { seed }
    }

    /// The seed, as `parityConfigString` prints it.
    pub fn seed(&self) -> i64 {
        self.seed
    }
}

impl PositionalRandomFactory for LegacyPositionalRandomFactory {
    type Source = LegacyRandomSource;

    fn at(&self, x: i32, y: i32, z: i32) -> LegacyRandomSource {
        LegacyRandomSource::new(crate::block_seed(x, y, z) ^ self.seed)
    }

    /// Uses `String.hashCode()`, not MD5.
    fn from_hash_of(&self, name: &str) -> LegacyRandomSource {
        LegacyRandomSource::new(i64::from(crate::java_string_hash(name)) ^ self.seed)
    }

    /// Ignores the factory's own seed, as the game does.
    fn from_seed(&self, seed: i64) -> LegacyRandomSource {
        LegacyRandomSource::new(seed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Expected values are from java.util.Random on JDK 25, which LegacyRandomSource reproduces
    // (same seed scrambling, LCG and derived methods).

    #[test]
    fn matches_java_util_random_ints_and_longs() {
        let mut r = LegacyRandomSource::new(0);
        assert_eq!(
            [r.next_int(), r.next_int(), r.next_int()],
            [-1_155_484_576, -723_955_400, 1_033_096_058]
        );
        let mut r = LegacyRandomSource::new(42);
        assert_eq!(
            [r.next_long(), r.next_long()],
            [-5_025_562_857_975_149_833, -5_843_495_416_241_995_736]
        );
        let mut r = LegacyRandomSource::new(-1);
        assert_eq!(
            [r.next_int(), r.next_int(), r.next_int()],
            [1_155_099_827, 1_887_904_451, 52_699_159]
        );
    }

    #[test]
    fn matches_java_util_random_bounded() {
        let mut r = LegacyRandomSource::new(0);
        let tens: Vec<_> = (0..4).map(|_| r.next_int_bounded(10)).collect();
        assert_eq!(tens, [0, 8, 9, 7]);
        // A power of two takes the multiply-and-shift path.
        let mut r = LegacyRandomSource::new(42);
        assert_eq!([r.next_int_bounded(16), r.next_int_bounded(16)], [11, 0]);
        let mut r = LegacyRandomSource::new(-1);
        let tens: Vec<_> = (0..4).map(|_| r.next_int_bounded(10)).collect();
        assert_eq!(tens, [3, 5, 9, 9]);
    }

    #[test]
    fn matches_java_util_random_floats_and_booleans() {
        assert_eq!(
            LegacyRandomSource::new(0).next_float().to_bits(),
            0x3f3b_20b4
        );
        assert_eq!(
            LegacyRandomSource::new(42).next_double().to_bits(),
            0x3fe7_4833_a06f_f457
        );
        assert_eq!(
            LegacyRandomSource::new(-1).next_double().to_bits(),
            0x3fd1_365b_2708_722c
        );
        let mut r = LegacyRandomSource::new(0);
        let bools: Vec<_> = (0..4).map(|_| r.next_boolean()).collect();
        assert_eq!(bools, [true, true, false, true]);
    }
}
