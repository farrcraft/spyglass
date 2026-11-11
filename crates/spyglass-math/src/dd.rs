//! Double-double arithmetic: a value as the unevaluated sum of two `f64`s, `hi + lo` with
//! `|lo| ≤ ulp(hi) / 2`, carrying about 106 bits. Built from error-free transformations (Knuth's
//! two-sum, Dekker's split product), never a fused multiply-add, so every platform computes the
//! same bits.

/// `hi + lo`, normalized.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Dd {
    pub hi: f64,
    pub lo: f64,
}

/// `a + b` exactly, as the rounded sum and its error (Knuth).
pub(crate) fn two_sum(a: f64, b: f64) -> Dd {
    let s = a + b;
    let v = s - a;
    Dd {
        hi: s,
        lo: (a - (s - v)) + (b - v),
    }
}

/// `a + b` exactly when `|a| ≥ |b|` (Dekker).
fn fast_two_sum(a: f64, b: f64) -> Dd {
    let s = a + b;
    Dd {
        hi: s,
        lo: b - (s - a),
    }
}

/// `a` as two halves of 26 bits each, whose products are exact (Dekker's split). Overflows for
/// `|a|` above about 2^996, which nothing here reaches.
fn split(a: f64) -> (f64, f64) {
    let t = 134_217_729.0 * a; // 2^27 + 1
    let hi = t - (t - a);
    (hi, a - hi)
}

/// `a · b` exactly, as the rounded product and its error.
pub(crate) fn two_prod(a: f64, b: f64) -> Dd {
    let p = a * b;
    let (ah, al) = split(a);
    let (bh, bl) = split(b);
    Dd {
        hi: p,
        lo: ((ah * bh - p) + ah * bl + al * bh) + al * bl,
    }
}

impl Dd {
    pub(crate) const fn new(hi: f64) -> Dd {
        Dd { hi, lo: 0.0 }
    }

    /// A double-double from its two parts, already normalized.
    #[cfg(test)]
    pub(crate) const fn pair(hi: f64, lo: f64) -> Dd {
        Dd { hi, lo }
    }

    pub(crate) fn add(self, b: Dd) -> Dd {
        let s = two_sum(self.hi, b.hi);
        let t = two_sum(self.lo, b.lo);
        let s = fast_two_sum(s.hi, s.lo + t.hi);
        fast_two_sum(s.hi, s.lo + t.lo)
    }

    pub(crate) fn neg(self) -> Dd {
        Dd {
            hi: -self.hi,
            lo: -self.lo,
        }
    }

    pub(crate) fn sub(self, b: Dd) -> Dd {
        self.add(b.neg())
    }

    pub(crate) fn mul(self, b: Dd) -> Dd {
        let p = two_prod(self.hi, b.hi);
        fast_two_sum(p.hi, p.lo + (self.hi * b.lo + self.lo * b.hi))
    }

    pub(crate) fn mul_f(self, b: f64) -> Dd {
        let p = two_prod(self.hi, b);
        fast_two_sum(p.hi, p.lo + self.lo * b)
    }

    /// `self / b`: three quotient digits, each from the remainder so far.
    pub(crate) fn div(self, b: Dd) -> Dd {
        let q1 = self.hi / b.hi;
        let r = self.sub(b.mul_f(q1));
        let q2 = r.hi / b.hi;
        let r = r.sub(b.mul_f(q2));
        let q3 = r.hi / b.hi;
        fast_two_sum(q1, q2).add(Dd::new(q3))
    }

    /// The double nearest `hi + lo`, if `relative` (an error bound, as a fraction of the value)
    /// leaves no doubt: both ends of the interval round to it (Ziv's test). `None` when the
    /// true value may lie on the other side of a rounding boundary.
    pub(crate) fn round(self, relative: f64) -> Option<f64> {
        let err = self.hi.abs() * relative;
        let low = self.hi + (self.lo - err);
        let high = self.hi + (self.lo + err);
        (low == high).then_some(low)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn products_and_sums_are_exact() {
        let a = 1.0 + f64::EPSILON;
        let p = two_prod(a, a);
        // (1 + ε)² = 1 + 2ε + ε², which needs more than 53 bits.
        assert_eq!(p.hi, 1.0 + 2.0 * f64::EPSILON);
        assert_eq!(p.lo, f64::EPSILON * f64::EPSILON);
        let s = two_sum(1.0, 1e-30);
        assert_eq!((s.hi, s.lo), (1.0, 1e-30));
    }

    #[test]
    fn divides_to_double_double_precision() {
        let third = Dd::new(1.0).div(Dd::new(3.0));
        // Three thirds is one, to within the last bits of a double-double.
        let back = third.mul_f(3.0).sub(Dd::new(1.0));
        assert!(back.hi.abs() < 1e-31, "{back:?}");
    }

    #[test]
    fn rounds_only_without_doubt() {
        assert_eq!(Dd::pair(1.0, 1e-20).round(1e-25), Some(1.0));
        // Exactly halfway between 1 and the next double: no way to tell.
        let half = f64::EPSILON / 2.0;
        assert_eq!(Dd::pair(1.0, half).round(1e-25), None);
    }
}
