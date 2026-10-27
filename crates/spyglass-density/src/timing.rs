//! Coarse timing of evaluation, behind the `timing` feature, for finding where the time goes.
//! Not a stable interface, and compiled away without the feature.
//!
//! A `span` times the code until it's dropped. Spans nest: each records its inclusive time, and
//! its exclusive time without the spans inside it. Totals are per thread, by name. A
//! `detail` span, one per density node, is recorded only while `set_detail` is on: with
//! thousands per operation, they slow the code around them too.

#[cfg(feature = "timing")]
mod enabled {
    use std::cell::{Cell, RefCell};
    use std::collections::BTreeMap;
    use std::time::{Duration, Instant};

    #[derive(Default)]
    struct State {
        /// Time spent in the spans inside each open span.
        inside: Vec<Duration>,
        totals: BTreeMap<&'static str, Total>,
    }

    /// One name's totals.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    pub struct Total {
        /// Time in its spans, including the spans inside them.
        pub inclusive: Duration,
        /// Time in its spans, without the spans inside them.
        pub exclusive: Duration,
        /// How many spans.
        pub calls: u64,
    }

    thread_local! {
        static STATE: RefCell<State> = RefCell::default();
        static DETAIL: Cell<bool> = const { Cell::new(true) };
    }

    /// An open span; dropping it records it. `None` for a detail span while details are off.
    pub struct Span(Option<(&'static str, Instant)>);

    /// Opens a span named `name`.
    pub fn span(name: &'static str) -> Span {
        STATE.with_borrow_mut(|s| s.inside.push(Duration::ZERO));
        Span(Some((name, Instant::now())))
    }

    /// Opens a span named `name` if details are on ([`set_detail`]).
    pub fn detail(name: &'static str) -> Span {
        if DETAIL.get() { span(name) } else { Span(None) }
    }

    /// Whether [`detail`] spans are recorded on this thread; on by default.
    pub fn set_detail(on: bool) {
        DETAIL.set(on);
    }

    impl Drop for Span {
        fn drop(&mut self) {
            let Some((name, start)) = self.0 else {
                return;
            };
            let elapsed = start.elapsed();
            STATE.with_borrow_mut(|s| {
                let inside = s.inside.pop().unwrap_or_default();
                let total = s.totals.entry(name).or_default();
                total.inclusive += elapsed;
                total.exclusive += elapsed.saturating_sub(inside);
                total.calls += 1;
                if let Some(parent) = s.inside.last_mut() {
                    *parent += elapsed;
                }
            });
        }
    }

    /// This thread's totals by name, which are then reset.
    pub fn take() -> BTreeMap<&'static str, Total> {
        STATE.with_borrow_mut(|s| std::mem::take(&mut s.totals))
    }
}

#[cfg(feature = "timing")]
pub use enabled::{Span, Total, detail, set_detail, span, take};
