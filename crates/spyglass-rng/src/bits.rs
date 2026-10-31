//! `BitRandomSource`'s methods, from `next(bits)`: shared by `LegacyRandomSource` and
//! `WorldgenRandom`, which differ only in where the bits come from.

/// `BitRandomSource.FLOAT_MULTIPLIER`: 2^-24.
const FLOAT_MULTIPLIER: f32 = 5.960_464_5e-8;
/// `BitRandomSource.DOUBLE_MULTIPLIER`: 2^-53.
const DOUBLE_MULTIPLIER: f64 = 1.110_223_024_625_156_5e-16;

/// A source of `next(bits)`: the top `bits` bits of its next value, as an `int`.
pub(crate) trait Bits {
    fn next(&mut self, bits: u32) -> i32;
}

/// `nextInt(int)`: uniform in `0..bound`.
///
/// # Panics
///
/// If `bound` isn't positive, where the game throws `IllegalArgumentException`.
pub(crate) fn next_int_bounded(source: &mut impl Bits, bound: i32) -> i32 {
    assert!(bound > 0, "Bound must be positive");
    // A power of two takes the top bits, which are better mixed than the low bits a
    // remainder would keep.
    if bound & (bound - 1) == 0 {
        return ((i64::from(bound) * i64::from(source.next(31))) >> 31) as i32;
    }
    loop {
        let bits = source.next(31);
        let value = bits % bound;
        // Rejects the top, partial range; the sum overflows exactly when it's hit.
        if bits.wrapping_sub(value).wrapping_add(bound - 1) >= 0 {
            return value;
        }
    }
}

/// `nextLong()`: two 32-bit halves, the low one added as signed.
pub(crate) fn next_long(source: &mut impl Bits) -> i64 {
    let hi = source.next(32);
    let lo = source.next(32);
    (i64::from(hi) << 32).wrapping_add(i64::from(lo))
}

/// `nextBoolean()`.
pub(crate) fn next_boolean(source: &mut impl Bits) -> bool {
    source.next(1) != 0
}

/// `nextFloat()`: 24 bits.
pub(crate) fn next_float(source: &mut impl Bits) -> f32 {
    source.next(24) as f32 * FLOAT_MULTIPLIER
}

/// `nextDouble()`: 26 bits and then 27, joined into the 53 a `double`'s mantissa holds.
pub(crate) fn next_double(source: &mut impl Bits) -> f64 {
    let hi = source.next(26);
    let lo = source.next(27);
    ((i64::from(hi) << 27) + i64::from(lo)) as f64 * DOUBLE_MULTIPLIER
}
