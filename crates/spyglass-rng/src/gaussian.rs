//! `MarsagliaPolarGaussian`.

/// The state of `MarsagliaPolarGaussian`: each round produces two values and caches the second.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Gaussian {
    next: f64,
    have_next: bool,
}

impl Gaussian {
    /// `reset()`.
    pub(crate) fn reset(&mut self) {
        self.have_next = false;
    }

    /// `nextGaussian()`, drawing uniform doubles from `next_double`.
    ///
    /// Java's `Math.log` may differ from Rust's `ln` in the last bit; see `docs/specs/rng.md`.
    pub(crate) fn next(&mut self, mut next_double: impl FnMut() -> f64) -> f64 {
        if self.have_next {
            self.have_next = false;
            return self.next;
        }
        loop {
            let v1 = 2.0 * next_double() - 1.0;
            let v2 = 2.0 * next_double() - 1.0;
            // Mth.square(v1) + Mth.square(v2).
            let s = v1 * v1 + v2 * v2;
            if s < 1.0 && s != 0.0 {
                let multiplier = (-2.0 * s.ln() / s).sqrt();
                self.next = v2 * multiplier;
                self.have_next = true;
                return v1 * multiplier;
            }
        }
    }
}
