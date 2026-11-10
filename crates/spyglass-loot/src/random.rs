//! The game's helpers over a random source and its float arithmetic that filling uses
//! ([loot.md](../../../docs/specs/loot.md#numbers)).

use spyglass_rng::RandomSource;

/// `Mth.nextInt(random, min, max)`: `min` without a draw if `min >= max`, else uniform in
/// `min..=max`.
pub(crate) fn next_int(random: &mut impl RandomSource, min: i32, max: i32) -> i32 {
    if min >= max {
        min
    } else {
        random
            .next_int_bounded(max.wrapping_sub(min).wrapping_add(1))
            .wrapping_add(min)
    }
}

/// `Mth.nextFloat(random, min, max)`: `min` without a draw if `min >= max`, else
/// `nextFloat() * (max - min) + min`, in `f32`.
pub(crate) fn next_float(random: &mut impl RandomSource, min: f32, max: f32) -> f32 {
    if min >= max {
        min
    } else {
        random.next_float() * (max - min) + min
    }
}

/// `Util.shuffle`: for `i` from the length down to 2, swap `i - 1` with `nextInt(i)`.
pub(crate) fn shuffle<T>(list: &mut [T], random: &mut impl RandomSource) {
    for i in (2..=list.len()).rev() {
        let j = random.next_int_bounded(i as i32) as usize;
        list.swap(i - 1, j);
    }
}

/// `Math.round(float)`: the nearest int, halves up, as `floor(x + 0.5)` computed exactly; NaN
/// is 0 and the rest saturate.
pub(crate) fn round(x: f32) -> i32 {
    // An f32 plus 0.5 is exact in f64, where the f32 sum could round up (0.49999997 + 0.5).
    (f64::from(x) + 0.5).floor() as i32
}

/// `Mth.floor(float)`: `(int) Math.floor((double) x)`, saturating, NaN 0.
pub(crate) fn floor(x: f32) -> i32 {
    f64::from(x).floor() as i32
}

/// `Mth.clamp(float, min, max)`: `x < min ? min : Math.min(x, max)`, NaN staying NaN.
pub(crate) fn clamp(x: f32, min: f32, max: f32) -> f32 {
    if x < min {
        min
    } else if x.is_nan() || x <= max {
        x
    } else {
        max
    }
}

#[cfg(test)]
mod tests {
    use spyglass_rng::LegacyRandomSource;

    use super::*;

    #[test]
    fn no_draw_when_the_range_is_one_value() {
        let mut a = LegacyRandomSource::new(7);
        let mut b = LegacyRandomSource::new(7);
        assert_eq!(next_int(&mut a, 3, 3), 3);
        assert_eq!(next_int(&mut a, 5, 2), 5);
        assert_eq!(next_float(&mut a, 0.5, 0.5), 0.5);
        // Nothing was drawn: both randoms agree.
        assert_eq!(a.next_long(), b.next_long());
    }

    #[test]
    fn ranges_draw_once_each() {
        let mut a = LegacyRandomSource::new(42);
        let mut b = LegacyRandomSource::new(42);
        assert_eq!(next_int(&mut a, 2, 5), b.next_int_bounded(4) + 2);
        assert_eq!(
            next_float(&mut a, 0.15, 0.8),
            b.next_float() * (0.8f32 - 0.15) + 0.15
        );
    }

    #[test]
    fn shuffle_swaps_from_the_end() {
        // By hand: for i = 3, 2: j = nextInt(i), swap(i - 1, j).
        let mut random = LegacyRandomSource::new(1);
        let mut copy = LegacyRandomSource::new(1);
        let mut list = [0, 1, 2];
        shuffle(&mut list, &mut random);
        let mut want = [0, 1, 2];
        for i in [3, 2] {
            let j = copy.next_int_bounded(i) as usize;
            want.swap(i as usize - 1, j);
        }
        assert_eq!(list, want);
    }

    #[test]
    fn round_is_javas() {
        assert_eq!(round(0.5), 1);
        assert_eq!(round(-0.5), 0);
        assert_eq!(round(-1.5), -1);
        assert_eq!(round(0.49999997), 0);
        assert_eq!(round(f32::NAN), 0);
        assert_eq!(round(3.0e10), i32::MAX);
        assert_eq!(floor(-0.25), -1);
        assert_eq!(clamp(1.5, 0.0, 1.0), 1.0);
        assert_eq!(clamp(-0.5, 0.0, 1.0), 0.0);
        assert!(clamp(f32::NAN, 0.0, 1.0).is_nan());
    }
}
