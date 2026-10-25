//! Bit-exact ports of Minecraft's RNGs: Java LCG, Xoroshiro128++, positional random factories.
//!
//! Names follow the game's (`net.minecraft.world.level.levelgen`), and each item cites the class
//! and method it ports. Integer arithmetic wraps as Java's does. See `docs/specs/rng.md`.

mod gaussian;
mod legacy;
mod source;
pub mod support;
pub mod worldgen;
mod xoroshiro;

pub use legacy::{LegacyPositionalRandomFactory, LegacyRandomSource};
pub use source::{PositionalRandomFactory, RandomSource};
pub use support::Seed128;
pub use xoroshiro::{
    Xoroshiro128PlusPlus, XoroshiroPositionalRandomFactory, XoroshiroRandomSource,
};

/// `Mth.getSeed(int, int, int)`: the per-block seed that positional factories mix in.
pub fn block_seed(x: i32, y: i32, z: i32) -> i64 {
    let mut seed = i64::from(x.wrapping_mul(3_129_871))
        ^ i64::from(z).wrapping_mul(116_129_781)
        ^ i64::from(y);
    seed = seed
        .wrapping_mul(seed)
        .wrapping_mul(42_317_861)
        .wrapping_add(seed.wrapping_mul(11));
    seed >> 16
}

/// `String.hashCode()`: `s[0]*31^(n-1) + ... + s[n-1]` over UTF-16 code units, wrapping.
pub fn java_string_hash(s: &str) -> i32 {
    s.encode_utf16().fold(0i32, |h, unit| {
        h.wrapping_mul(31).wrapping_add(i32::from(unit))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn string_hash_matches_the_jdk() {
        // Values from String.hashCode() on JDK 25.
        assert_eq!(java_string_hash(""), 0);
        assert_eq!(java_string_hash("spyglass"), -1_556_512_960);
        // From the formula: ((233 * 31) + 116) * 31 + 233 over the three code units.
        assert_eq!(java_string_hash("\u{e9}t\u{e9}"), 227_742);
        // A surrogate pair hashes as two code units.
        assert_eq!(java_string_hash("\u{1F600}"), 1_772_899);
    }

    #[test]
    fn block_seed_multiplies_x_in_32_bits() {
        assert_eq!(block_seed(0, 0, 0), 0);
        // Java multiplies x as an int, so x and x + 2^32 / gcd agree; widening first wouldn't.
        // 3129871 * 1_000_000 overflows i32: the result must differ from the widened product's.
        let widened = {
            let s = 1_000_000_i64 * 3_129_871;
            s.wrapping_mul(s)
                .wrapping_mul(42_317_861)
                .wrapping_add(s * 11)
                >> 16
        };
        assert_ne!(block_seed(1_000_000, 0, 0), widened);
        // y enters unmultiplied, so y = 1 differs from y = 0 only through the final mix.
        assert_eq!(block_seed(0, 1, 0), (42_317_861 + 11) >> 16);
    }
}
