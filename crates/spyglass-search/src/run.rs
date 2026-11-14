//! Running a search: seed sources, budgets, and a ranked top list that's the same for any
//! thread count (`docs/specs/search.md#seed-sources`, ADR 0021).

use std::ops::ControlFlow;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use rayon::prelude::*;

/// The most indices per batch of checks: small, since one check can take milliseconds.
const BATCH: u128 = 16;
/// How long a batch aims to take on its thread: batches of slow checks, which can take seconds
/// each, hold fewer, so every thread has one and a cut round drops little.
const BATCH_TIME: f64 = 0.01;
/// Seeds that share their lower 48 bits in [`SeedSource::All`]: one prefilter answer each.
const GROUP: u128 = 1 << 16;
/// How long a round of batches aims to take: progress is reported between rounds, and each
/// round's results raise the score later checks must reach.
const ROUND: Duration = Duration::from_millis(500);
/// How long a round may run before it's cut short, wherever its size estimate put its end.
const MAX_ROUND_TIME: Duration = Duration::from_secs(1);
/// The most indices in one round, however cheap they are.
const MAX_ROUND: u128 = 1 << 30;
/// The most indices one round checks after the prefilter, which bounds its list of batches.
const MAX_ROUND_CHECKS: u128 = 1 << 22;

/// Asks a search to stop, from any thread. Clones share one request. The runner checks it
/// before every seed, so a search stops within one seed's check of [`stop`](Self::stop).
#[derive(Debug, Clone, Default)]
pub struct Stop(Arc<AtomicU8>);

impl Stop {
    /// A handle nothing has asked to stop yet.
    pub fn new() -> Self {
        Stop::default()
    }

    /// Stops searching. What happens to the seeds found so far is the caller's: the public
    /// API still evaluates them, unless [`stop_now`](Self::stop_now) is asked too.
    pub fn stop(&self) {
        self.0.fetch_max(1, Ordering::Relaxed);
    }

    /// Stops searching and everything after it.
    pub fn stop_now(&self) {
        self.0.store(2, Ordering::Relaxed);
    }

    /// Whether [`stop`](Self::stop) or [`stop_now`](Self::stop_now) has been asked.
    pub fn is_stopped(&self) -> bool {
        self.0.load(Ordering::Relaxed) >= 1
    }

    /// Whether [`stop_now`](Self::stop_now) has been asked.
    pub fn is_stopped_now(&self) -> bool {
        self.0.load(Ordering::Relaxed) >= 2
    }
}

/// Two handles are equal when they share one request.
impl PartialEq for Stop {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for Stop {}

/// Limits how many of a search's threads work at once, from any thread, while it runs: so a
/// program can lend the search's cores to other work for a while and give them back. Clones share
/// one limit. The runner takes it up before each batch of seeds, so a new limit holds within a
/// batch's checks. Which seeds are reported doesn't depend on it (ADR 0021); only how fast.
#[derive(Debug, Clone, Default)]
pub struct Throttle(Arc<ThrottleState>);

#[derive(Debug, Default)]
struct ThrottleState {
    /// The most threads at work; 0 for no limit.
    limit: AtomicUsize,
    /// Threads at work now.
    active: AtomicUsize,
}

/// How long a thread held back by a [`Throttle`] waits before looking again.
const THROTTLED_WAIT: Duration = Duration::from_millis(2);

impl Throttle {
    /// A handle with no limit yet.
    pub fn new() -> Self {
        Throttle::default()
    }

    /// At most `threads` at work at once (at least one), from each thread's next batch;
    /// `None` lifts the limit.
    pub fn set(&self, threads: Option<usize>) {
        let limit = threads.map_or(0, |n| n.max(1));
        self.0.limit.store(limit, Ordering::Relaxed);
    }

    /// The limit, if there is one.
    pub fn limit(&self) -> Option<usize> {
        match self.0.limit.load(Ordering::Relaxed) {
            0 => None,
            n => Some(n),
        }
    }

    /// Waits until this thread may work, or until `stop` is asked; held until dropped.
    fn enter(&self, stop: &Stop) -> Working<'_> {
        let state = &*self.0;
        loop {
            let active = state.active.load(Ordering::Relaxed);
            let limit = state.limit.load(Ordering::Relaxed);
            if (limit == 0 || active < limit || stop.is_stopped())
                && state
                    .active
                    .compare_exchange_weak(active, active + 1, Ordering::Relaxed, Ordering::Relaxed)
                    .is_ok()
            {
                return Working(state);
            }
            if limit != 0 && active >= limit {
                std::thread::sleep(THROTTLED_WAIT);
            }
        }
    }
}

/// A thread a [`Throttle`] counts as at work.
struct Working<'a>(&'a ThrottleState);

impl Drop for Working<'_> {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Two handles are equal when they share one limit.
impl PartialEq for Throttle {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for Throttle {}

/// The thread pool a search runs on: `threads` workers, or one per core.
pub fn pool(threads: Option<usize>) -> Result<rayon::ThreadPool, rayon::ThreadPoolBuildError> {
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads.unwrap_or(0))
        .build()
}

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
                let count = (i128::from(*last) - i128::from(*first) + 1).max(0);
                u64::try_from(count).ok()
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
    /// Stops the search when asked, part way through a round.
    pub stop: Stop,
    /// How many threads may work at once, changed while it runs.
    pub throttle: Throttle,
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

/// `f` on each of `items` on `pool`, in parallel, until `stop` says to stop: the results in
/// order, up to the first item not done. Items after it are dropped even if they're done, so
/// what's returned doesn't depend on which threads got furthest.
pub fn map_prefix<T: Sync, R: Send, E: Send>(
    pool: &rayon::ThreadPool,
    items: &[T],
    stop: impl Fn() -> bool + Sync,
    f: impl Fn(&T) -> Result<R, E> + Sync,
) -> Result<Vec<R>, E> {
    let results: Vec<Option<Result<R, E>>> = pool.install(|| {
        items
            .par_iter()
            .map(|item| (!stop()).then(|| f(item)))
            .collect()
    });
    results.into_iter().map_while(|r| r).collect()
}

/// [`map_prefix`] that stops when `stop` is asked, with each item's work held to `throttle`'s
/// limit on threads at work.
pub fn map_prefix_throttled<T: Sync, R: Send, E: Send>(
    pool: &rayon::ThreadPool,
    items: &[T],
    stop: &Stop,
    throttle: &Throttle,
    f: impl Fn(&T) -> Result<R, E> + Sync,
) -> Result<Vec<R>, E> {
    map_prefix(
        pool,
        items,
        || stop.is_stopped(),
        |item| {
            let _working = throttle.enter(stop);
            f(item)
        },
    )
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
    /// Seeds the prefilter rejected.
    prefiltered: u64,
    /// Seeds checked.
    checked: u64,
    /// Seeds reported, scored or below the threshold.
    reported: u64,
    /// The batch's best hits, ranked and cut to the top list's length.
    top: Vec<Hit>,
}

/// Runs a search on `pool`. `accepts` is the prefilter; `check(seed, threshold)` says whether a
/// seed is reported and, if its score is at least `threshold`, what it is. Work goes in rounds
/// of about half a second; `progress` is called after each with the state so far, and stops the
/// search by returning `Break`. The threshold is the top list's last score when the round began.
///
/// A round is cut short, before any seed, when [`RunOptions::stop`] is asked, when the time
/// limit passes, or when it has run for a second. What's kept is every index below the first
/// one not finished, so `next` still means every index below it is done; the rest is left for
/// the next round (`docs/specs/search.md#seed-sources`).
///
/// The top list is the same for any thread count, throttle and timing, because nothing in it
/// depends on which thread finished first. Every seed in a round is checked against the same
/// threshold, fixed when the round begins. A seed scored below it can't be in the final list
/// either, since the list's last score only rises, so a higher or lower threshold (rounds sized
/// differently) changes how much work is skipped, not what's found. The batches are merged in
/// index order, ties in score go to the lower index, and a cut keeps only a prefix of indices,
/// so `next` always means the same set of seeds was done.
pub fn run<E: Send>(
    pool: &rayon::ThreadPool,
    options: &RunOptions,
    accepts: impl Fn(i64) -> bool + Sync,
    check: impl Fn(i64, u64) -> Result<Checked, E> + Sync,
    mut progress: impl FnMut(&Progress) -> ControlFlow<()>,
) -> Result<Progress, E> {
    let start = Instant::now();
    let deadline = options.time.map(|t| start + t);
    let top_n = options.top.max(1);
    // Indices as u128, so `All`'s 2⁶⁴ seeds end at a number.
    let len = options.source.len().map_or(1 << 64, u128::from);
    let from = u128::from(options.from).min(len);
    let end = options
        .seeds
        .map_or(len, |count| (from + u128::from(count)).min(len));
    let threads = pool.current_num_threads() as u128;
    // The first round has a batch of one index for each thread; later rounds' batches aim at
    // BATCH_TIME, and their rounds at ROUND, with a batch for every thread at least.
    let mut batch_len = 1;
    let mut round = threads;
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
        // The round covers `cursor..round_end`, unless it's cut short.
        let mut round_end = end.min(cursor + round);
        let started = Instant::now();
        let cut_off = started + MAX_ROUND_TIME;
        let threshold = if state.top.len() == top_n {
            state.top[top_n - 1].score
        } else {
            0
        };
        // The indices to check. In `All`, one prefilter answer covers each group of seeds
        // sharing their lower 48 bits, and rejected groups are skipped whole. Once
        // MAX_ROUND_CHECKS indices are accepted, the round ends at the next group.
        let mut ranges = vec![(cursor, round_end)];
        let mut rejected = Vec::new();
        if grouped {
            // Each group as `(start, end)`, cut at the round's ends.
            let mut groups = Vec::new();
            let mut group_start = cursor;
            while group_start < round_end {
                let group_end = ((group_start / GROUP + 1) * GROUP).min(round_end);
                groups.push((group_start, group_end));
                group_start = group_end;
            }
            let accepted: Vec<bool> = pool.install(|| {
                groups
                    .par_iter()
                    .map(|&(group_start, _)| accepts(options.source.seed(group_start as u64)))
                    .collect()
            });
            ranges.clear();
            let mut checks = 0;
            for (range, accepted) in groups.into_iter().zip(accepted) {
                if checks >= MAX_ROUND_CHECKS {
                    round_end = range.0;
                    break;
                }
                if accepted {
                    checks += range.1 - range.0;
                    ranges.push(range);
                } else {
                    rejected.push(range);
                }
            }
        }
        // Each range in pieces of `batch_len`, as `(start, end)`, in index order.
        let batches: Vec<(u128, u128)> = ranges
            .into_iter()
            .flat_map(|(range_start, range_end)| {
                (0..(range_end - range_start).div_ceil(batch_len)).map(move |k| {
                    (
                        range_start + k * batch_len,
                        (range_start + (k + 1) * batch_len).min(range_end),
                    )
                })
            })
            .collect();
        // Whether to leave the seed at `index` for later. The round's first index is never
        // left for running long, so every round gets somewhere unless the search is stopped.
        let cut = |index: u128| {
            options.stop.is_stopped()
                || deadline.is_some_and(|d| Instant::now() >= d)
                || (index != cursor && Instant::now() >= cut_off)
        };
        // Each batch, and how far it got.
        let results: Vec<Result<(Batch, u128), E>> = pool.install(|| {
            batches
                .par_iter()
                .map(|&(batch_start, batch_end)| {
                    let _working = options.throttle.enter(&options.stop);
                    let mut batch = Batch::default();
                    for index in batch_start..batch_end {
                        if cut(index) {
                            return Ok((batch, index));
                        }
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
                    Ok((batch, batch_end))
                })
                .collect()
        });
        // Keep the batches in index order up to the first that stopped short, with that one's
        // part, and drop the rest: the next round checks their indices again.
        let mut reached = round_end;
        for (&(_, batch_end), result) in batches.iter().zip(results) {
            let (batch, got_to) = result?;
            state.prefiltered += batch.prefiltered;
            state.checked += batch.checked;
            state.reported += batch.reported;
            merge(&mut state.top, batch.top, top_n);
            if got_to < batch_end {
                reached = got_to;
                break;
            }
        }
        // A cut falls inside an accepted group, so each rejected group is wholly before or
        // after it.
        state.prefiltered += rejected
            .iter()
            .filter(|&&(_, group_end)| group_end <= reached)
            .map(|&(group_start, group_end)| (group_end - group_start) as u64)
            .sum::<u64>();
        // Size the next round's batches and the round itself from this one's speed.
        let took = started.elapsed().as_secs_f64().max(1e-6);
        let per_second = (reached - cursor) as f64 / took;
        // Per thread, so checks that keep every thread busy fill a batch as fast as one alone.
        let per_thread = per_second / threads as f64;
        batch_len = ((per_thread * BATCH_TIME) as u128).clamp(1, BATCH);
        round = ((per_second * ROUND.as_secs_f64()) as u128).clamp(threads * batch_len, MAX_ROUND);
        cursor = reached;
        state.evaluated = u64::try_from(cursor - from).unwrap_or(u64::MAX);
        state.next = next(cursor);
        state.elapsed = start.elapsed();
        let out_of_time = deadline.is_some_and(|d| Instant::now() >= d);
        if progress(&state).is_break() || out_of_time || options.stop.is_stopped() {
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
            stop: Stop::new(),
            throttle: Throttle::new(),
        }
    }

    fn four() -> rayon::ThreadPool {
        pool(Some(4)).unwrap()
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
        let hash = (seed as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15) >> 54; // 0 to 1023
        Ok(match (seed & 1 == 1, hash >= threshold) {
            (false, _) => Checked::Failed,
            (true, false) => Checked::Below,
            (true, true) => Checked::Scored(hash),
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
    fn a_throttle_limits_the_threads_at_work_but_not_the_results() {
        let (at_work, most) = (AtomicUsize::new(0), AtomicUsize::new(0));
        let check = |seed, threshold| {
            let now = at_work.fetch_add(1, Ordering::SeqCst) + 1;
            most.fetch_max(now, Ordering::SeqCst);
            std::thread::sleep(Duration::from_micros(50));
            at_work.fetch_sub(1, Ordering::SeqCst);
            score(seed, threshold)
        };
        let source = SeedSource::Range {
            first: 0,
            last: 5000,
        };
        let o = options(source.clone(), 0, None, 10);
        o.throttle.set(Some(1));
        let out = run(&four(), &o, |_| true, check, |_| ControlFlow::Continue(())).unwrap();
        assert_eq!(most.load(Ordering::SeqCst), 1);
        assert_eq!(out.top, brute_force(&source, 0, 5001, 10));
        // Lifted, the pool's threads work together again.
        o.throttle.set(None);
        assert_eq!(o.throttle.limit(), None);
        run(&four(), &o, |_| true, check, |_| ControlFlow::Continue(())).unwrap();
        assert!(most.load(Ordering::SeqCst) > 1);
    }

    #[test]
    fn results_match_brute_force_for_any_thread_count() {
        let source = SeedSource::Range {
            first: -50_000,
            last: 50_000,
        };
        let expected = brute_force(&source, 123, 70_000, 25);
        for threads in [1, 2, 8] {
            let o = options(source.clone(), 123, Some(70_000), 25);
            let pool = pool(Some(threads)).unwrap();
            let out = run(&pool, &o, |_| true, score, |_| ControlFlow::Continue(())).unwrap();
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
            &four(),
            &options(source.clone(), 0, None, 10),
            |_| true,
            score,
            |_| ControlFlow::Continue(()),
        )
        .unwrap();
        assert_eq!(whole.next, None);
        assert_eq!(whole.evaluated, 20_000);
        let first = run(
            &four(),
            &options(source.clone(), 0, Some(8_000), 10),
            |_| true,
            score,
            |_| ControlFlow::Continue(()),
        )
        .unwrap();
        let second = run(
            &four(),
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
            &four(),
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
            &four(),
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
            &four(),
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
            &four(),
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
            &four(),
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

    #[test]
    fn slow_checks_spread_over_the_threads_and_run_once() {
        // Checks of 300 ms each, as a feature search's are. Each thread takes one seed at a
        // time, so no round is cut with checks done but dropped, and none runs twice.
        let source = SeedSource::Range {
            first: 0,
            last: 999,
        };
        let o = options(source.clone(), 0, Some(24), 5);
        let calls = AtomicUsize::new(0);
        let started = Instant::now();
        let out = run(
            &four(),
            &o,
            |_| true,
            |seed, threshold| {
                calls.fetch_add(1, Ordering::Relaxed);
                std::thread::sleep(Duration::from_millis(300));
                score(seed, threshold)
            },
            |_| ControlFlow::Continue(()),
        )
        .unwrap();
        assert_eq!(calls.into_inner(), 24);
        assert_eq!(out.top, brute_force(&source, 0, 24, 5));
        // Six rounds of four: 1.8 s, where batches of 16 took over 5 s.
        assert!(
            started.elapsed() < Duration::from_secs(4),
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn a_stop_cuts_a_round_part_way() {
        // Checks take a while, and the search is asked to stop from inside one. It stops within
        // a check, and keeps exactly the indices below `next`, as if it had been asked to
        // evaluate that many. The time is measured from the stop: before it, how long the sleeps
        // take depends on the OS timer (on Windows, up to 15.6 ms each, not 200 µs).
        let source = SeedSource::Range {
            first: 0,
            last: 999_999,
        };
        let o = options(source.clone(), 0, None, 10);
        let stop = o.stop.clone();
        let stopped_at = std::sync::OnceLock::new();
        let out = run(
            &four(),
            &o,
            |_| true,
            |seed, threshold| {
                if seed >= 3_000 {
                    stopped_at.get_or_init(Instant::now);
                    stop.stop();
                }
                std::thread::sleep(Duration::from_micros(200));
                score(seed, threshold)
            },
            |_| ControlFlow::Continue(()),
        )
        .unwrap();
        assert!(stopped_at.get().unwrap().elapsed() < Duration::from_secs(2));
        let next = out.next.unwrap();
        assert!(next < 100_000, "{next}");
        assert_eq!(out.evaluated, next);
        assert_eq!(out.top, brute_force(&source, 0, next, 10));
        assert!(o.stop.is_stopped() && !o.stop.is_stopped_now());
    }

    #[test]
    fn a_time_limit_cuts_a_long_round() {
        // The first groups are all rejected, so they go fast and the next round is sized for
        // millions of seeds; then every seed takes a millisecond. The time limit still holds.
        let accepts = |seed: i64| seed & 0xffff_ffff_ffff >= 64;
        let o = RunOptions {
            time: Some(Duration::from_millis(300)),
            ..options(SeedSource::All, 0, None, 5)
        };
        let started = Instant::now();
        let out = run(
            &four(),
            &o,
            accepts,
            |seed, threshold| {
                std::thread::sleep(Duration::from_millis(1));
                score(seed, threshold)
            },
            |_| ControlFlow::Continue(()),
        )
        .unwrap();
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "{:?}",
            started.elapsed()
        );
        let next = out.next.unwrap();
        assert!(next > 64 << 16, "{next}");
        // Every rejected group below `next` is counted, and nothing past it.
        assert_eq!(out.prefiltered, 64 << 16);
        assert_eq!(out.checked, next - (64 << 16));
    }
}
