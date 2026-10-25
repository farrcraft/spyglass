//! `Xoroshiro128PlusPlus`, `XoroshiroRandomSource` and its positional factory: the random source
//! modern worldgen uses (`legacy_random_source: false`).

use crate::gaussian::Gaussian;
use crate::source::{PositionalRandomFactory, RandomSource};
use crate::support::{self, GOLDEN_RATIO_64, SILVER_RATIO_64, Seed128};

/// `XoroshiroRandomSource.FLOAT_UNIT`: 2^-24.
const FLOAT_UNIT: f32 = 5.960_464_5e-8;
/// `XoroshiroRandomSource.DOUBLE_UNIT`: 2^-53.
const DOUBLE_UNIT: f64 = 1.110_223_024_625_156_5e-16;

/// `Xoroshiro128PlusPlus`: the generator itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Xoroshiro128PlusPlus {
    lo: i64,
    hi: i64,
}

impl Xoroshiro128PlusPlus {
    /// `Xoroshiro128PlusPlus(long, long)`. An all-zero state, which would only ever produce
    /// zeros, is replaced by the golden and silver ratios, as in the game.
    pub fn new(lo: i64, hi: i64) -> Self {
        if lo | hi == 0 {
            Xoroshiro128PlusPlus {
                lo: GOLDEN_RATIO_64,
                hi: SILVER_RATIO_64,
            }
        } else {
            Xoroshiro128PlusPlus { lo, hi }
        }
    }

    /// `Xoroshiro128PlusPlus(Seed128bit)`.
    pub fn from_seed(seed: Seed128) -> Self {
        Self::new(seed.lo, seed.hi)
    }

    /// `nextLong()`.
    pub fn next_long(&mut self) -> i64 {
        let lo = self.lo;
        let mut hi = self.hi;
        let result = lo.wrapping_add(hi).rotate_left(17).wrapping_add(lo);
        hi ^= lo;
        self.lo = lo.rotate_left(49) ^ hi ^ (hi << 21);
        self.hi = hi.rotate_left(28);
        result
    }
}

/// `XoroshiroRandomSource`.
#[derive(Debug, Clone)]
pub struct XoroshiroRandomSource {
    rng: Xoroshiro128PlusPlus,
    gaussian: Gaussian,
}

impl XoroshiroRandomSource {
    /// `XoroshiroRandomSource(long)`: the seed through `upgradeSeedTo128bit`.
    pub fn new(seed: i64) -> Self {
        Self::from_seed(support::upgrade_seed_to_128bit(seed))
    }

    /// `XoroshiroRandomSource(Seed128bit)`.
    pub fn from_seed(seed: Seed128) -> Self {
        Self::from_generator(Xoroshiro128PlusPlus::from_seed(seed))
    }

    /// `XoroshiroRandomSource(long, long)`: a raw 128-bit state, used unmixed.
    pub fn from_state(lo: i64, hi: i64) -> Self {
        Self::from_generator(Xoroshiro128PlusPlus::new(lo, hi))
    }

    fn from_generator(rng: Xoroshiro128PlusPlus) -> Self {
        XoroshiroRandomSource {
            rng,
            gaussian: Gaussian::default(),
        }
    }

    /// `setSeed(long)`.
    pub fn set_seed(&mut self, seed: i64) {
        self.rng = Xoroshiro128PlusPlus::from_seed(support::upgrade_seed_to_128bit(seed));
        self.gaussian.reset();
    }

    /// `nextBits(int)`: the top `bits` bits of the next long.
    fn next_bits(&mut self, bits: u32) -> u64 {
        (self.rng.next_long() as u64) >> (64 - bits)
    }
}

impl RandomSource for XoroshiroRandomSource {
    type Positional = XoroshiroPositionalRandomFactory;

    fn fork(&mut self) -> Self {
        let lo = self.rng.next_long();
        let hi = self.rng.next_long();
        Self::from_state(lo, hi)
    }

    fn fork_positional(&mut self) -> XoroshiroPositionalRandomFactory {
        let lo = self.rng.next_long();
        let hi = self.rng.next_long();
        XoroshiroPositionalRandomFactory { lo, hi }
    }

    fn next_int(&mut self) -> i32 {
        self.rng.next_long() as i32
    }

    /// Lemire's method: multiply, then reject the biased low range.
    fn next_int_bounded(&mut self, bound: i32) -> i32 {
        assert!(bound > 0, "Bound must be positive");
        let bound64 = i64::from(bound);
        let mut product = i64::from(self.next_int() as u32) * bound64;
        let mut low = product & 0xffff_ffff;
        if low < bound64 {
            // Integer.remainderUnsigned(-bound, bound).
            let threshold = i64::from(bound.wrapping_neg() as u32 % bound as u32);
            while low < threshold {
                product = i64::from(self.next_int() as u32) * bound64;
                low = product & 0xffff_ffff;
            }
        }
        (product >> 32) as i32
    }

    fn next_long(&mut self) -> i64 {
        self.rng.next_long()
    }

    fn next_boolean(&mut self) -> bool {
        self.rng.next_long() & 1 != 0
    }

    fn next_float(&mut self) -> f32 {
        self.next_bits(24) as f32 * FLOAT_UNIT
    }

    fn next_double(&mut self) -> f64 {
        self.next_bits(53) as f64 * DOUBLE_UNIT
    }

    fn next_gaussian(&mut self) -> f64 {
        let mut gaussian = self.gaussian;
        let value = gaussian.next(|| self.next_double());
        self.gaussian = gaussian;
        value
    }

    fn consume_count(&mut self, count: i32) {
        for _ in 0..count {
            self.rng.next_long();
        }
    }
}

/// `XoroshiroRandomSource.XoroshiroPositionalRandomFactory`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct XoroshiroPositionalRandomFactory {
    lo: i64,
    hi: i64,
}

impl XoroshiroPositionalRandomFactory {
    /// A factory from its two seed halves, as `forkPositional` makes it.
    pub fn new(lo: i64, hi: i64) -> Self {
        XoroshiroPositionalRandomFactory { lo, hi }
    }

    /// `seedLo` and `seedHi`, as `parityConfigString` prints them.
    pub fn seed(&self) -> Seed128 {
        Seed128 {
            lo: self.lo,
            hi: self.hi,
        }
    }
}

impl PositionalRandomFactory for XoroshiroPositionalRandomFactory {
    type Source = XoroshiroRandomSource;

    fn at(&self, x: i32, y: i32, z: i32) -> XoroshiroRandomSource {
        XoroshiroRandomSource::from_state(crate::block_seed(x, y, z) ^ self.lo, self.hi)
    }

    fn from_hash_of(&self, name: &str) -> XoroshiroRandomSource {
        XoroshiroRandomSource::from_seed(support::seed_from_hash_of(name).xor(self.lo, self.hi))
    }

    fn from_seed(&self, seed: i64) -> XoroshiroRandomSource {
        XoroshiroRandomSource::from_state(seed ^ self.lo, seed ^ self.hi)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn longs(mut rng: Xoroshiro128PlusPlus) -> [i64; 4] {
        std::array::from_fn(|_| rng.next_long())
    }

    #[test]
    fn matches_the_jdks_xoroshiro128plusplus() {
        // jdk.internal.random.Xoroshiro128PlusPlus(x0, x1).nextLong() on JDK 25, an implementation
        // independent of the game's.
        assert_eq!(
            longs(Xoroshiro128PlusPlus::new(1, 2)),
            [
                393_217,
                669_327_710_093_319,
                1_732_421_326_133_921_491,
                -7_051_953_992_050_424_633
            ]
        );
        assert_eq!(
            longs(Xoroshiro128PlusPlus::new(0x0123_4567_89ab_cdef, -2)),
            [
                -8_362_523_980_082_065_355,
                -6_845_419_023_851_938_775,
                -6_221_830_669_876_034_202,
                -1_054_186_753_984_944_070
            ]
        );
    }

    #[test]
    fn zero_state_becomes_the_ratios() {
        // The JDK has no zero-state rule, so compare with its output for the ratios.
        assert_eq!(
            longs(Xoroshiro128PlusPlus::new(0, 0)),
            [
                6_807_859_099_481_836_695,
                5_275_285_228_792_843_439,
                -1_883_134_111_310_439_721,
                -7_481_282_880_567_689_833
            ]
        );
    }

    #[test]
    fn bounded_ints_stay_in_range() {
        let mut r = XoroshiroRandomSource::new(42);
        for bound in [1, 2, 3, 7, 1 << 30, i32::MAX] {
            for _ in 0..100 {
                let v = r.next_int_bounded(bound);
                assert!((0..bound).contains(&v), "{v} not in 0..{bound}");
            }
        }
    }

    #[test]
    fn floats_and_doubles_stay_in_unit_range() {
        let mut r = XoroshiroRandomSource::new(-1);
        for _ in 0..1000 {
            assert!((0.0..1.0).contains(&r.next_float()));
            assert!((0.0..1.0).contains(&r.next_double()));
        }
    }

    #[test]
    #[should_panic(expected = "Bound must be positive")]
    fn rejects_non_positive_bounds() {
        XoroshiroRandomSource::new(0).next_int_bounded(0);
    }
}
