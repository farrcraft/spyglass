//! The viewer's own timings, for measuring it: with `SPYGLASS_VIEWER_TIMING=1`, each job's
//! compute time and how long the map takes to fill the window go to stderr. Without it, nothing
//! is measured or written.

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

/// How long the window takes to fill, from when what it shows last changed: the world, seed,
/// view or layers.
#[derive(Default)]
pub struct Fill {
    /// What the window shows, hashed, and when it started showing it.
    shown: Option<(u64, Instant)>,
    coarse: bool,
    full: bool,
}

impl Fill {
    /// One frame of the map showing `what`, and whether every tile in the window was drawn,
    /// coarse or detailed, and whether every one was drawn detailed with its markers.
    pub fn frame(&mut self, what: impl Hash, covered: bool, full: bool) {
        if !enabled() {
            return;
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
                return;
            }
        };
        let seconds = start.elapsed().as_secs_f64();
        if covered && !self.coarse {
            self.coarse = true;
            eprintln!("timing: window covered in {seconds:.2} s");
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
        }
    }
}
