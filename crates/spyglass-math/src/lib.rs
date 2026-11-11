//! Java's `Math.sin`, `Math.cos`, `Math.log` and `Math.pow` as HotSpot computes them on x86-64,
//! where most of the game's players are: correctly rounded, in all but about 0.1% of cases for
//! `sin` and `cos`, 0.002% for `log` and 0.05% for `pow`, never more than 1 ulp off. Every
//! platform gets the same bits, since nothing here calls the platform's libm or a fused
//! multiply-add. See `docs/specs/java-math.md` for the measurements, and
//! `docs/adr/0070-java-math-correctly-rounded.md` for why.
//!
//! Each result is computed in double-double arithmetic, to about 2^-100 of its value, and
//! rounded once when a rounding test shows the bound leaves no doubt. Where it doesn't, which
//! no measured argument hit, the double-double's own rounding is returned. `sin` and `cos` of
//! |x| ≥ 2^20, and `pow` results below 2^-1021, aren't correctly rounded; no worldgen call
//! comes near either.

mod dd;
mod explog;
mod trig;

/// `Math.sin`.
pub fn sin(x: f64) -> f64 {
    trig::sin(x).0
}

/// `Math.cos`.
pub fn cos(x: f64) -> f64 {
    trig::cos(x).0
}

/// `Math.log`: the natural logarithm.
pub fn log(x: f64) -> f64 {
    explog::log(x).0
}

/// `Math.pow`, with Java's special cases (`pow(x, 0)` is 1 even for NaN, `pow(x, 1)` is `x`).
pub fn pow(x: f64, y: f64) -> f64 {
    explog::pow(x, y).0
}

/// Each function with whether its rounding test settled the result: `false` where the result
/// may be an ulp from the correctly rounded one. For measuring, not for worldgen.
pub mod settled {
    /// `Math.sin`, and whether it's surely correctly rounded.
    pub fn sin(x: f64) -> (f64, bool) {
        crate::trig::sin(x)
    }

    /// `Math.cos`, and whether it's surely correctly rounded.
    pub fn cos(x: f64) -> (f64, bool) {
        crate::trig::cos(x)
    }

    /// `Math.log`, and whether it's surely correctly rounded.
    pub fn log(x: f64) -> (f64, bool) {
        crate::explog::log(x)
    }

    /// `Math.pow`, and whether it's surely correctly rounded.
    pub fn pow(x: f64, y: f64) -> (f64, bool) {
        crate::explog::pow(x, y)
    }
}
