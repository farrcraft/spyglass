//! `RandomSupport`: seed mixing and the 128-bit seeds Xoroshiro uses.

use md5::{Digest, Md5};

/// `RandomSupport.GOLDEN_RATIO_64`.
pub const GOLDEN_RATIO_64: i64 = -7_046_029_254_386_353_131;
/// `RandomSupport.SILVER_RATIO_64`.
pub const SILVER_RATIO_64: i64 = 7_640_891_576_956_012_809;

/// `RandomSupport.mixStafford13(long)`: Stafford's "Mix13" 64-bit finalizer.
pub fn mix_stafford13(mut z: i64) -> i64 {
    z = (z ^ ((z as u64) >> 30) as i64).wrapping_mul(-4_658_895_280_553_007_687);
    z = (z ^ ((z as u64) >> 27) as i64).wrapping_mul(-7_723_592_293_110_705_685);
    z ^ ((z as u64) >> 31) as i64
}

/// `RandomSupport.Seed128bit`: a Xoroshiro seed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Seed128 {
    /// `seedLo`.
    pub lo: i64,
    /// `seedHi`.
    pub hi: i64,
}

impl Seed128 {
    /// `Seed128bit.xor(long, long)`.
    pub fn xor(self, lo: i64, hi: i64) -> Seed128 {
        Seed128 {
            lo: self.lo ^ lo,
            hi: self.hi ^ hi,
        }
    }

    /// `Seed128bit.mixed()`: both halves through [`mix_stafford13`].
    pub fn mixed(self) -> Seed128 {
        Seed128 {
            lo: mix_stafford13(self.lo),
            hi: mix_stafford13(self.hi),
        }
    }
}

/// `RandomSupport.upgradeSeedTo128bitUnmixed(long)`.
pub fn upgrade_seed_to_128bit_unmixed(seed: i64) -> Seed128 {
    let lo = seed ^ SILVER_RATIO_64;
    Seed128 {
        lo,
        hi: lo.wrapping_add(GOLDEN_RATIO_64),
    }
}

/// `RandomSupport.upgradeSeedTo128bit(long)`: the unmixed seed, then [`Seed128::mixed`].
pub fn upgrade_seed_to_128bit(seed: i64) -> Seed128 {
    upgrade_seed_to_128bit_unmixed(seed).mixed()
}

/// `RandomSupport.seedFromHashOf(String)`: the MD5 of the UTF-8 bytes, read as two big-endian
/// longs (`Longs.fromBytes`).
pub fn seed_from_hash_of(name: &str) -> Seed128 {
    let digest: [u8; 16] = Md5::digest(name.as_bytes()).into();
    let (lo, hi) = digest.split_at(8);
    Seed128 {
        lo: i64::from_be_bytes(lo.try_into().expect("8 bytes")),
        hi: i64::from_be_bytes(hi.try_into().expect("8 bytes")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mix_stafford13_is_splitmix64s_finalizer() {
        // SplitMix64's first output for seed 0 is mix13(0 + golden gamma): 0xe220a8397b1dcdaf.
        assert_eq!(
            mix_stafford13(GOLDEN_RATIO_64) as u64,
            0xe220_a839_7b1d_cdaf
        );
        assert_eq!(mix_stafford13(0), 0);
    }

    #[test]
    fn seed_from_hash_of_reads_md5_big_endian() {
        // RFC 1321 test suite: MD5("") = d41d8cd98f00b204 e9800998ecf8427e.
        assert_eq!(
            seed_from_hash_of(""),
            Seed128 {
                lo: 0xd41d_8cd9_8f00_b204_u64 as i64,
                hi: 0xe980_0998_ecf8_427e_u64 as i64,
            }
        );
        // MD5("abc") = 90015098 3cd24fb0 d6963f7d 28e17f72.
        assert_eq!(
            seed_from_hash_of("abc"),
            Seed128 {
                lo: 0x9001_5098_3cd2_4fb0_u64 as i64,
                hi: 0xd696_3f7d_28e1_7f72_u64 as i64,
            }
        );
    }

    #[test]
    fn upgrade_wraps() {
        let s = upgrade_seed_to_128bit_unmixed(0);
        assert_eq!(s.lo, SILVER_RATIO_64);
        assert_eq!(s.hi, SILVER_RATIO_64.wrapping_add(GOLDEN_RATIO_64));
        assert_eq!(upgrade_seed_to_128bit(0), s.mixed());
    }
}
