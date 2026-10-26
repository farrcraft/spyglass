//! Running a search: seed sources, budgets, and a ranked top list that's the same for any
//! thread count (`docs/specs/search.md#seed-sources`, ADR 0021).

use std::ops::ControlFlow;
use std::time::{Duration, Instant};

use rayon::prelude::*;

/// Indices per batch of checks: small, since one check can take milliseconds.
const BATCH: u128 = 16;
/// Seeds that share their lower 48 bits in [`SeedSource::All`]: one prefilter answer each.
const GROUP: u128 = 1 << 16;
/// How long a round of batches aims to take: progress, stopping and time limits act between
/// rounds, and each round's results raise the score later checks must reach.
const ROUND: Duration = Duration::from_millis(500);
/// The most indices in one round, however cheap they are.
const MAX_ROUND: u128 = 1 << 30;

/// Where a search's seeds come from, each with an index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SeedSource {
    /// Every 64-bit seed: index `i` has lower 48 bits `i >> 16` and upper 16 bits `i & 0xFFFF`.
    All,
    /// `first..=last` in order.
    Range {
        /// The first seed.
        first: i64,
        /// The last seed, included.
        last: i64,
    },
    /// These seeds, in this order.
    List(Vec<i64>),
}

impl SeedSource {
    /// How many seeds there are; `None` for 2⁶⁴.
    pub fn len(&self) -> Option<u64> {
        match self {
            SeedSource::All => None,
            SeedSource::Range { first, last } => {
                let n = (i128::from(*last) - i128::from(*first) + 1).max(0);
                u64::try_from(n).ok()
            }
            SeedSource::List(seeds) => Some(seeds.len() as u64),
        }
    }

    /// Whether there are no seeds.
    pub fn is_empty(&self) -> bool {
        self.len() == Some(0)
    }

    /// The seed at `index`, which must be below [`len`](Self::len).
    pub fn seed(&self, index: u64) -> i64 {
        match self {
            // Upper 16 bits from the index's lowest 16, lower 48 from the rest.
            SeedSource::All => index.rotate_right(16) as i64,
            SeedSource::Range { first, .. } => first.wrapping_add(index as i64),
            SeedSource::List(seeds) => seeds[index as usize],
        }
    }

    /// Whether consecutive runs of [`GROUP`] indices share their lower 48 bits, so one
    /// prefilter answer covers each.
    fn groups_share_lower_bits(&self) -> bool {
        matches!(self, SeedSource::All)
    }
}

/// How a search runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOptions {
    /// The seeds.
    pub source: SeedSource,
    /// The index to start at.
    pub from: u64,
    /// Evaluate exactly this many indices, or to the end of the source.
    pub seeds: Option<u64>,
    /// Stop once this much time has passed, where every index below `next` is done.
    pub time: Option<Duration>,
    /// How many hits to keep; at least 1.
    pub top: usize,
    /// Worker threads; all cores if `None`.
    pub threads: Option<usize>,
}

/// A reported seed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hit {
    /// Its index in the source.
    pub index: u64,
    /// The seed.
    pub seed: i64,
    /// Its score.
    pub score: u64,
}

/// How a search is going, or how it ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Progress {
    /// How many indices have been evaluated.
    pub evaluated: u64,
    /// The first index not yet evaluated; `None` once the source is finished.
    pub next: Option<u64>,
    /// How many seeds the prefilter rejected.
    pub prefiltered: u64,
    /// How many seeds were checked in full (or until a required constraint failed).
    pub checked: u64,
    /// How many seeds were reported, whether or not they made the top list.
    pub reported: u64,
    /// Time since the search started.
    pub elapsed: Duration,
    /// The best seeds so far, best first: by score, then by index.
    pub top: Vec<Hit>,
}

/// Merges `hits` into `top` in rank order, keeping the first `n`.
fn merge(top: &mut Vec<Hit>, hits: Vec<Hit>, n: usize) {
    top.extend(hits);
    top.sort_unstable_by(|a, b| b.score.cmp(&a.score).then(a.index.cmp(&b.index)));
    top.truncate(n);
}

/// A seed's fate in a search's per-seed check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Checked {
    /// A required constraint failed: not reported.
    Failed,
    /// Reported, but its score is below the threshold it was checked with, so it can't make
    /// the top list.
    Below,
    /// Reported, with this score, at least the threshold.
    Scored(u64),
}

/// What one batch found.
#[derive(Default)]
struct Batch {
    prefiltered: u64,
    checked: u64,
    reported: u64,
    top: Vec<Hit>,
}

/// Runs a search. `accepts` is the prefilter; `check(seed, threshold)` says whether a seed is
/// reported and, if its score is at least `threshold`, what it is. Work goes in rounds of about
/// half a second; `progress` is called after each with the state so far, and stops the search
/// by returning `Break`. The threshold is the top list's last score when the round began.
pub fn run<E: Send>(
    options: &RunOptions,
    accepts: impl Fn(i64) -> bool + Sync,
    check: impl Fn(i64, u64) -> Result<Checked, E> + Sync,
    mut progress: impl FnMut(&Progress) -> ControlFlow<()>,
) -> Result<Progress, E> {
    let start = Instant::now();
    let top_n = options.top.max(1);
    // Indices as u128, so `All`'s 2⁶⁴ seeds end at a number.
    let len = options.source.len().map_or(1 << 64, u128::from);
    let from = u128::from(options.from).min(len);
    let end = options
        .seeds
        .map_or(len, |n| (from + u128::from(n)).min(len));
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(options.threads.unwrap_or(0))
        .build()
        .expect("a thread pool");
    let threads = pool.current_num_threads() as u128;
    // Every thread gets a batch in the first round; later rounds aim at ROUND.
    let min_round = threads * BATCH;
    let mut round = min_round;
    let grouped = options.source.groups_share_lower_bits();
    let next = |cursor: u128| (cursor < len).then_some(cursor as u64);
    let mut state = Progress {
        evaluated: 0,
        next: next(from),
        prefiltered: 0,
        checked: 0,
        reported: 0,
        elapsed: Duration::ZERO,
        top: Vec::new(),
    };
    let mut cursor = from;
    while cursor < end {
        let stop = end.min(cursor + round);
        let started = Instant::now();
        let threshold = if state.top.len() == top_n {
            state.top[top_n - 1].score
        } else {
            0
        };
        // The indices to check. In `All`, one prefilter answer covers each group of seeds
        // sharing their lower 48 bits, and rejected groups are skipped whole.
        let mut ranges = vec![(cursor, stop)];
        if grouped {
            let mut groups = Vec::new();
            let mut a = cursor;
            while a < stop {
                let b = ((a / GROUP + 1) * GROUP).min(stop);
                groups.push((a, b));
                a = b;
            }
            let accepted: Vec<bool> = pool.install(|| {
                groups
                    .par_iter()
                    .map(|&(a, _)| accepts(options.source.seed(a as u64)))
                    .collect()
            });
            ranges = groups
                .into_iter()
                .zip(accepted)
                .filter_map(|(range, accepted)| {
                    if !accepted {
                        state.prefiltered += (range.1 - range.0) as u64;
                    }
                    accepted.then_some(range)
                })
                .collect();
        }
        let batches: Vec<(u128, u128)> = ranges
            .into_iter()
            .flat_map(|(a, b)| {
                (0..(b - a).div_ceil(BATCH))
                    .map(move |k| (a + k * BATCH, (a + (k + 1) * BATCH).min(b)))
            })
            .collect();
        let results: Vec<Result<Batch, E>> = pool.install(|| {
            batches
                .par_iter()
                .map(|&(a, b)| {
                    let mut batch = Batch::default();
                    for index in a..b {
                        let index = index as u64; // below len, at most 2⁶⁴
                        let seed = options.source.seed(index);
                        if !grouped && !accepts(seed) {
                            batch.prefiltered += 1;
                            continue;
                        }
                        batch.checked += 1;
                        match check(seed, threshold)? {
                            Checked::Failed => {}
                            Checked::Below => batch.reported += 1,
                            Checked::Scored(score) => {
                                batch.reported += 1;
                                merge(&mut batch.top, vec![Hit { index, seed, score }], top_n);
                            }
                        }
                    }
                    Ok(batch)
                })
                .collect()
        });
        for batch in results {
            let batch = batch?;
            state.prefiltered += batch.prefiltered;
            state.checked += batch.checked;
            state.reported += batch.reported;
            merge(&mut state.top, batch.top, top_n);
        }
        let took = started.elapsed().as_secs_f64().max(1e-6);
        let per_second = (stop - cursor) as f64 / took;
        round = ((per_second * ROUND.as_secs_f64()) as u128).clamp(min_round, MAX_ROUND);
        cursor = stop;
        state.evaluated = u64::try_from(cursor - from).unwrap_or(u64::MAX);
        state.next = next(cursor);
        state.elapsed = start.elapsed();
        let out_of_time = options.time.is_some_and(|t| state.elapsed >= t);
        if progress(&state).is_break() || out_of_time {
            break;
        }
    }
    state.elapsed = start.elapsed();
    Ok(state)
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;

    use super::*;

    fn options(source: SeedSource, from: u64, seeds: Option<u64>, top: usize) -> RunOptions {
        RunOptions {
            source,
            from,
            seeds,
            time: None,
            top,
            threads: Some(4),
        }
    }

    #[test]
    fn sources_map_indices_to_seeds() {
        assert_eq!(SeedSource::All.seed(0), 0);
        assert_eq!(SeedSource::All.seed(1), 1 << 48);
        assert_eq!(SeedSource::All.seed(0xffff), -(1 << 48));
        assert_eq!(SeedSource::All.seed(0x1_0000), 1);
        assert_eq!(SeedSource::All.seed(u64::MAX), -1);
        let range = SeedSource::Range { first: -3, last: 3 };
        assert_eq!(
            (range.len(), range.seed(0), range.seed(6)),
            (Some(7), -3, 3)
        );
        let empty = SeedSource::Range { first: 1, last: 0 };
        assert!(empty.is_empty());
        let whole = SeedSource::Range {
            first: i64::MIN,
            last: i64::MAX,
        };
        assert_eq!(whole.len(), None);
        assert_eq!(whole.seed(u64::MAX), i64::MAX);
    }

    /// A made-up score: reported when the seed is odd, scored by a hash of it.
    fn score(seed: i64, threshold: u64) -> Result<Checked, Infallible> {
        let s = (seed as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15) >> 54; // 0 to 1023
        Ok(match (seed & 1 == 1, s >= threshold) {
            (false, _) => Checked::Failed,
            (true, false) => Checked::Below,
            (true, true) => Checked::Scored(s),
        })
    }

    fn brute_force(source: &SeedSource, from: u64, count: u64, n: usize) -> Vec<Hit> {
        let mut top = Vec::new();
        for index in from..from + count {
            let seed = source.seed(index);
            if let Checked::Scored(score) = score(seed, 0).unwrap() {
                merge(&mut top, vec![Hit { index, seed, score }], n);
            }
        }
        top
    }

    #[test]
    fn results_match_brute_force_for_any_thread_count() {
        let source = SeedSource::Range {
            first: -50_000,
            last: 50_000,
        };
        let expected = brute_force(&source, 123, 70_000, 25);
        for threads in [1, 2, 8] {
            let o = RunOptions {
                threads: Some(threads),
                ..options(source.clone(), 123, Some(70_000), 25)
            };
            let out = run(&o, |_| true, score, |_| ControlFlow::Continue(())).unwrap();
            assert_eq!(out.top, expected, "{threads} threads");
            assert_eq!((out.evaluated, out.next), (70_000, Some(70_123)));
            assert_eq!(out.checked, 70_000);
            // Every odd seed is reported, pruned or not.
            let odd = (123..70_123).filter(|i| source.seed(*i) & 1 == 1).count();
            assert_eq!(out.reported, odd as u64);
        }
    }

    #[test]
    fn a_range_in_two_parts_merges_to_the_whole() {
        let source = SeedSource::List((0..20_000).map(|i| i * 7 - 3).collect());
        let whole = run(
            &options(source.clone(), 0, None, 10),
            |_| true,
            score,
            |_| ControlFlow::Continue(()),
        )
        .unwrap();
        assert_eq!(whole.next, None);
        assert_eq!(whole.evaluated, 20_000);
        let first = run(
            &options(source.clone(), 0, Some(8_000), 10),
            |_| true,
            score,
            |_| ControlFlow::Continue(()),
        )
        .unwrap();
        let second = run(
            &options(source, first.next.unwrap(), None, 10),
            |_| true,
            score,
            |_| ControlFlow::Continue(()),
        )
        .unwrap();
        let mut merged = first.top;
        merge(&mut merged, second.top, 10);
        assert_eq!(merged, whole.top);
    }

    #[test]
    fn the_prefilter_skips_whole_batches_in_all() {
        // Reject every lower-48 value but 1, 3 and 5.
        let accepts = |seed: i64| matches!(seed & 0xffff_ffff_ffff, 1 | 3 | 5);
        let out = run(
            &options(SeedSource::All, 0, Some(6 << 16), 5),
            accepts,
            score,
            |_| ControlFlow::Continue(()),
        )
        .unwrap();
        assert_eq!(out.checked, 3 << 16);
        assert_eq!(out.prefiltered, 3 << 16);
        assert!(out.top.iter().all(|h| accepts(h.seed)));
        let expected: Vec<Hit> = brute_force(&SeedSource::All, 0, 6 << 16, 1000)
            .into_iter()
            .filter(|h| accepts(h.seed))
            .take(5)
            .collect();
        assert_eq!(out.top, expected);
    }

    #[test]
    fn progress_can_stop_a_search() {
        let mut calls = 0;
        let out = run(
            &options(SeedSource::All, 0, None, 3),
            |_| true,
            score,
            |p| {
                calls += 1;
                assert!(p.top.len() <= 3);
                if calls == 2 {
                    ControlFlow::Break(())
                } else {
                    ControlFlow::Continue(())
                }
            },
        )
        .unwrap();
        assert_eq!(calls, 2);
        let next = out.next.unwrap();
        assert_eq!(out.evaluated, next);
        // Repeating the same count gives the same list.
        let again = run(
            &options(SeedSource::All, 0, Some(next), 3),
            |_| true,
            score,
            |_| ControlFlow::Continue(()),
        )
        .unwrap();
        assert_eq!(again.top, out.top);
    }

    #[test]
    fn the_end_of_all_seeds() {
        let out = run(
            &options(SeedSource::All, u64::MAX - 5, None, 10),
            |_| true,
            score,
            |_| ControlFlow::Continue(()),
        )
        .unwrap();
        assert_eq!((out.evaluated, out.next), (6, None));
        assert_eq!(out.top, brute_force_tail());
    }

    fn brute_force_tail() -> Vec<Hit> {
        let mut top = Vec::new();
        for index in u64::MAX - 5..=u64::MAX {
            let seed = SeedSource::All.seed(index);
            if let Checked::Scored(score) = score(seed, 0).unwrap() {
                merge(&mut top, vec![Hit { index, seed, score }], 10);
            }
        }
        top
    }

    #[test]
    fn errors_stop_the_search() {
        let out = run(
            &options(
                SeedSource::Range {
                    first: 0,
                    last: 100,
                },
                0,
                None,
                3,
            ),
            |_| true,
            |seed, _| {
                if seed == 50 {
                    Err("bad seed")
                } else {
                    Ok(Checked::Failed)
                }
            },
            |_| ControlFlow::Continue(()),
        );
        assert_eq!(out.unwrap_err(), "bad seed");
    }
}
