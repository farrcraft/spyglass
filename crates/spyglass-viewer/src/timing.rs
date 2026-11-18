//! The viewer's own timings, for measuring it: with `SPYGLASS_VIEWER_TIMING=1`, each job's
//! compute time, each texture's on the UI thread, and how long the map takes to fill the window
//! go to stderr. Without it, nothing is measured or written. `SPYGLASS_VIEWER_TIMING_ZOOM`
//! then zooms the map once, to time the zoom too ([`zoom`]).

use std::collections::BTreeMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::{LazyLock, Mutex, PoisonError};
use std::time::{Duration, Instant};

/// Whether timings are wanted.
pub fn enabled() -> bool {
    static ON: LazyLock<bool> =
        LazyLock::new(|| std::env::var("SPYGLASS_VIEWER_TIMING").is_ok_and(|v| v == "1"));
    *ON
}

/// A zoom made once while timing, from `SPYGLASS_VIEWER_TIMING_ZOOM=<scale>[,covered]`: to
/// `scale` blocks per pixel around the view's centre, when the window is first full, or with
/// `,covered` when it's first covered, while jobs are still running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Zoom {
    /// Blocks per pixel to zoom to.
    pub scale: u32,
    /// Zoom when the window is first covered rather than full.
    pub at_covered: bool,
}

/// The zoom asked for, if timings are wanted and the variable names a scale the map has.
pub fn zoom() -> Option<Zoom> {
    if !enabled() {
        return None;
    }
    parse_zoom(&std::env::var("SPYGLASS_VIEWER_TIMING_ZOOM").ok()?)
}

/// `<scale>` or `<scale>,covered`, the scale a power of two up to the library's largest.
fn parse_zoom(text: &str) -> Option<Zoom> {
    let (scale, at_covered) = match text.split_once(',') {
        Some((scale, "covered")) => (scale, true),
        Some(_) => return None,
        None => (text, false),
    };
    let scale: u32 = scale.trim().parse().ok()?;
    (scale.is_power_of_two() && scale <= spyglass::MAX_SCALE).then_some(Zoom { scale, at_covered })
}

/// What the window reached this frame, for the first time since what it shows changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Filled {
    /// Every tile drawn, coarse or detailed.
    Covered,
    /// Every tile drawn detailed, with its markers.
    Full,
}

/// Jobs' times by kind since the window last started filling: how many, and their total.
static JOBS: Mutex<BTreeMap<String, (u32, Duration)>> = Mutex::new(BTreeMap::new());

/// Times `f` as a job of `kind` if timings are wanted.
pub fn job<T>(kind: impl FnOnce() -> String, f: impl FnOnce() -> T) -> T {
    if !enabled() {
        return f();
    }
    let start = Instant::now();
    let out = f();
    let took = start.elapsed();
    let kind = kind();
    eprintln!("timing: job {kind} {:.1} ms", took.as_secs_f64() * 1000.0);
    let mut jobs = JOBS.lock().unwrap_or_else(PoisonError::into_inner);
    let (count, total) = jobs.entry(kind).or_default();
    *count += 1;
    *total += took;
    out
}

/// A job of `kind` that stopped `after` its stop was raised, if timings are wanted: how long a
/// worker stayed busy with a tile nothing wanted any more.
pub fn stopped(kind: String, after: Duration) {
    if enabled() {
        eprintln!(
            "timing: stopped {kind} after {:.1} ms",
            after.as_secs_f64() * 1000.0
        );
    }
}

/// How long the window takes to fill, from when what it shows last changed: the world, seed,
/// view or layers.
#[derive(Default)]
pub struct Fill {
    /// What the window shows, hashed, and when it started showing it.
    shown: Option<(u64, Instant)>,
    /// Whether "covered" was reported for what's shown.
    coarse: bool,
    /// Whether "full" was reported for what's shown.
    full: bool,
}

impl Fill {
    /// One frame of the map showing `what`, and whether every tile in the window was drawn,
    /// coarse or detailed, and whether every one was drawn detailed with its markers.
    /// Returns what the window reached this frame for the first time, if anything.
    pub fn frame(&mut self, what: impl Hash, covered: bool, full: bool) -> Option<Filled> {
        if !enabled() {
            return None;
        }
        let mut hasher = DefaultHasher::new();
        what.hash(&mut hasher);
        let what = hasher.finish();
        let start = match self.shown {
            Some((shown, start)) if shown == what => start,
            _ => {
                *self = Fill {
                    shown: Some((what, Instant::now())),
                    ..Fill::default()
                };
                JOBS.lock().unwrap_or_else(PoisonError::into_inner).clear();
                return None;
            }
        };
        let mut reached = None;
        let seconds = start.elapsed().as_secs_f64();
        if covered && !self.coarse {
            self.coarse = true;
            eprintln!("timing: window covered in {seconds:.2} s");
            reached = Some(Filled::Covered);
        }
        if full && !self.full {
            self.full = true;
            eprintln!("timing: window full in {seconds:.2} s");
            let jobs = JOBS.lock().unwrap_or_else(PoisonError::into_inner);
            for (kind, (count, total)) in jobs.iter() {
                let total = total.as_secs_f64();
                eprintln!(
                    "timing:   {kind}: {count} jobs, {total:.2} s in all, {:.1} ms each",
                    total * 1000.0 / f64::from(*count)
                );
            }
            reached = Some(Filled::Full);
        }
        reached
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_zoom_is_a_scale_and_maybe_when_covered() {
        let zoom = |scale, at_covered| Some(Zoom { scale, at_covered });
        assert_eq!(parse_zoom("1"), zoom(1, false));
        assert_eq!(parse_zoom("8,covered"), zoom(8, true));
        assert_eq!(parse_zoom("3"), None);
        assert_eq!(parse_zoom("512"), None);
        assert_eq!(parse_zoom("4,full"), None);
        assert_eq!(parse_zoom("x"), None);
    }
}
