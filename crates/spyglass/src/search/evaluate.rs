//! Judging one seed against a query: the plain implementation, every constraint in full
//! (`docs/specs/search.md`).

use std::cell::OnceCell;

use spyglass_biome::BiomeId;
use spyglass_search::{Anchor, Count, Importance, Kind, Place, Region, Sampling};

use super::{Resolved, SearchQuery, Target};
use crate::{BiomeAt, Result, Session, SpawnOrigin, StructureQuery, StructureStart};

/// Whether a constraint, or a seed, meets a query (`docs/specs/search.md#outcomes`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Outcome {
    /// It holds in the worst case: whatever terrain and ties decide.
    Met,
    /// It holds only in the best case: it depends on what spyglass can't decide yet.
    PossiblyMet,
    /// It fails in both cases.
    Failed,
}

/// A seed judged against a query by [`Session::evaluate`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evaluation {
    /// Over the required constraints: failed if any failed, possibly met if any is only
    /// possibly met, met otherwise.
    pub verdict: Outcome,
    /// The score in the best case: what a search ranks by unless it's in certain mode.
    pub best_score: u64,
    /// The score in the worst case: what a search in certain mode ranks by.
    pub worst_score: u64,
    /// Each constraint's result, in the query's order.
    pub constraints: Vec<ConstraintResult>,
}

impl Evaluation {
    /// Whether a search reports this seed: met, or possibly met outside certain mode.
    pub fn reported(&self, certain: bool) -> bool {
        match self.verdict {
            Outcome::Met => true,
            Outcome::PossiblyMet => !certain,
            Outcome::Failed => false,
        }
    }

    /// The score a search ranks by.
    pub fn score(&self, certain: bool) -> u64 {
        if certain {
            self.worst_score
        } else {
            self.best_score
        }
    }
}

/// One constraint's result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConstraintResult {
    /// Its outcome.
    pub outcome: Outcome,
    /// Its satisfaction in the best case, 0 to 1,000. Required constraints have one too, though
    /// it doesn't count towards the score.
    pub best: u32,
    /// Its satisfaction in the worst case.
    pub worst: u32,
    /// What it matched, for showing: structure starts and slime chunks nearest first, a
    /// cluster's largest group, the nearest matching biome sample, the spawn. Empty for
    /// coverage.
    pub features: Vec<Feature>,
}

/// Something a constraint matched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Feature {
    /// A structure start, or a chunk where one may be.
    Structure(StructureStart),
    /// A slime chunk.
    SlimeChunk([i32; 2]),
    /// A biome sample.
    Biome {
        /// The block sampled.
        position: [i32; 3],
        /// The biome there.
        biome: BiomeAt,
        /// Whether the sample certainly matches; otherwise it's tied.
        certain: bool,
    },
    /// The spawn.
    Spawn(SpawnOrigin),
}

/// A constraint's answer in both cases, before the spawn's uncertainty is applied.
struct Judged {
    best: bool,
    worst: bool,
    best_sat: u32,
    worst_sat: u32,
    features: Vec<Feature>,
}

impl Judged {
    /// The constraint's result. A spawn that may differ by platform makes a constraint that
    /// uses it possibly met, and worth nothing in the worst case.
    fn result(self, spawn_uncertain: bool) -> ConstraintResult {
        let outcome = if spawn_uncertain {
            Outcome::PossiblyMet
        } else if self.worst {
            Outcome::Met
        } else if self.best {
            Outcome::PossiblyMet
        } else {
            Outcome::Failed
        };
        ConstraintResult {
            outcome,
            best: self.best_sat,
            worst: if spawn_uncertain { 0 } else { self.worst_sat },
            features: self.features,
        }
    }
}

/// One evaluation's view of a session: the spawn origin is found once, when first needed.
struct Judge<'a, 's> {
    session: &'a Session<'s>,
    spawn: OnceCell<SpawnOrigin>,
}

impl Session<'_> {
    /// Judges this seed against `query`: every constraint, in full, with what it matched. The
    /// plain implementation of `docs/specs/search.md`; searches check their answers against
    /// it.
    pub fn evaluate(&self, query: &SearchQuery) -> Result<Evaluation> {
        self.check_world(query.world, query.to_string().trim_end())?;
        let judge = Judge {
            session: self,
            spawn: OnceCell::new(),
        };
        let constraints: Vec<ConstraintResult> = query
            .query
            .constraints()
            .iter()
            .zip(&query.resolved)
            .map(|(c, resolved)| judge.constraint(&c.kind, resolved))
            .collect();
        let mut verdict = Outcome::Met;
        let (mut best_score, mut worst_score) = (0, 0);
        for (c, result) in query.query.constraints().iter().zip(&constraints) {
            match c.importance {
                Importance::Required => {
                    verdict = match (verdict, result.outcome) {
                        (_, Outcome::Failed) | (Outcome::Failed, _) => Outcome::Failed,
                        (_, Outcome::PossiblyMet) | (Outcome::PossiblyMet, _) => {
                            Outcome::PossiblyMet
                        }
                        (Outcome::Met, Outcome::Met) => Outcome::Met,
                    }
                }
                Importance::Preferred { weight } => {
                    best_score += u64::from(weight) * u64::from(result.best);
                    worst_score += u64::from(weight) * u64::from(result.worst);
                }
            }
        }
        Ok(Evaluation {
            verdict,
            best_score,
            worst_score,
            constraints,
        })
    }
}

/// A place in blocks.
#[derive(Debug, Clone, Copy)]
enum Area {
    Disc { centre: [i32; 2], radius: u32 },
    Box { min: [i32; 2], max: [i32; 2] },
}

impl Area {
    /// The bounding box, corners included.
    fn bounds(self) -> ([i32; 2], [i32; 2]) {
        match self {
            Area::Disc { centre, radius } => {
                let r = radius as i32; // at most 32,767
                (centre.map(|c| c - r), centre.map(|c| c + r))
            }
            Area::Box { min, max } => (min, max),
        }
    }

    /// The radius scores are graded over; 0 for a box, where every feature earns 1,000.
    fn radius(self) -> u32 {
        match self {
            Area::Disc { radius, .. } => radius,
            Area::Box { .. } => 0,
        }
    }

    /// `p`'s distance from the centre if it's in the place (0 in a box).
    fn distance(self, p: [i32; 2]) -> Option<u32> {
        match self {
            Area::Disc { centre, radius } => within(centre, p, radius),
            Area::Box { min, max } => {
                (min[0] <= p[0] && p[0] <= max[0] && min[1] <= p[1] && p[1] <= max[1]).then_some(0)
            }
        }
    }

    /// The biome grid's points, z then x, each with its distance from the centre.
    fn grid(self, step: u32) -> Vec<([i32; 2], u32)> {
        let step = i64::from(step);
        let mut out = Vec::new();
        match self {
            Area::Disc { centre, radius } => {
                let k = i64::from(radius) / step;
                for j in -k..=k {
                    for i in -k..=k {
                        let (dx, dz) = (i * step, j * step);
                        let d2 = dx * dx + dz * dz;
                        if d2 <= i64::from(radius).pow(2) {
                            // Inside the disc, so within the world's coordinates.
                            let p = [centre[0] + dx as i32, centre[1] + dz as i32];
                            out.push((p, d2.unsigned_abs().isqrt() as u32));
                        }
                    }
                }
            }
            Area::Box { min, max } => {
                let count = |a: i32, b: i32| i64::from(b - a) / step;
                for j in 0..=count(min[1], max[1]) {
                    for i in 0..=count(min[0], max[0]) {
                        out.push(([min[0] + (i * step) as i32, min[1] + (j * step) as i32], 0));
                    }
                }
            }
        }
        out
    }
}

/// `b`'s distance from `a`, `isqrt(dx² + dz²)`, if it's at most `radius`.
fn within(a: [i32; 2], b: [i32; 2], radius: u32) -> Option<u32> {
    let d2 = distance_sqr(a, b);
    (d2 <= u64::from(radius).pow(2)).then(|| d2.isqrt() as u32)
}

fn distance_sqr(a: [i32; 2], b: [i32; 2]) -> u64 {
    let d = |i: usize| i64::from(a[i]) - i64::from(b[i]);
    (d(0) * d(0) + d(1) * d(1)).unsigned_abs()
}

/// The points one feature earns at distance `d` in a place of radius `radius`.
fn points(d: u32, radius: u32) -> u64 {
    if radius == 0 {
        1000
    } else {
        500 + 500 * u64::from(radius - d) / u64::from(radius)
    }
}

/// The satisfaction of "at least `n`" given the matches' distances: the first `min(count, n)`,
/// nearest first, each earning [`points`], summed and divided by `n`.
fn graded(mut distances: Vec<u32>, n: u32, radius: u32) -> u32 {
    distances.sort_unstable();
    let sum: u64 = distances
        .iter()
        .take(n as usize)
        .map(|&d| points(d, radius))
        .sum();
    (sum / u64::from(n)) as u32 // at most 1,000
}

/// A feature that may be counted.
struct Candidate {
    position: [i32; 2],
    certain: bool,
    /// For structure starts, the set and chunk: a start is never its own anchor.
    start: Option<(String, [i32; 2])>,
}

/// A feature in a region: its distance in each case, `None` where it isn't counted.
struct Matched {
    index: usize,
    best: u32,
    worst: Option<u32>,
}

/// A region with its anchors found.
enum Within {
    Area(Area),
    Anchors {
        radius: u32,
        anchors: Vec<Candidate>,
    },
}

impl Within {
    /// The bounding box of everything that can match, or `None` if nothing can.
    fn bounds(&self) -> Option<([i32; 2], [i32; 2])> {
        match self {
            Within::Area(area) => Some(area.bounds()),
            Within::Anchors { radius, anchors } => {
                let r = *radius as i32; // at most 32,767
                let (first, rest) = anchors.split_first()?;
                let (mut min, mut max) = (first.position, first.position);
                for a in rest {
                    for i in 0..2 {
                        min[i] = min[i].min(a.position[i]);
                        max[i] = max[i].max(a.position[i]);
                    }
                }
                Some((min.map(|c| c - r), max.map(|c| c + r)))
            }
        }
    }

    fn radius(&self) -> u32 {
        match self {
            Within::Area(area) => area.radius(),
            Within::Anchors { radius, .. } => *radius,
        }
    }

    /// Whether `c` is in the region in each case, and how far it is from the centre or its
    /// nearest anchor.
    fn matches(&self, c: &Candidate) -> Option<(u32, Option<u32>)> {
        match self {
            Within::Area(area) => {
                let d = area.distance(c.position)?;
                Some((d, c.certain.then_some(d)))
            }
            Within::Anchors { radius, anchors } => {
                let nearest = |certain_only: bool| {
                    anchors
                        .iter()
                        .filter(|a| !certain_only || a.certain)
                        .filter(|a| c.start.is_none() || a.start != c.start)
                        .filter_map(|a| within(a.position, c.position, *radius))
                        .min()
                };
                let best = nearest(false)?;
                Some((best, c.certain.then(|| nearest(true)).flatten()))
            }
        }
    }
}

impl Judge<'_, '_> {
    fn spawn(&self) -> &SpawnOrigin {
        self.spawn.get_or_init(|| self.session.spawn_origin())
    }

    fn area(&self, place: &Place) -> Area {
        match *place {
            Place::Within { radius, anchor } => Area::Disc {
                centre: match anchor {
                    Anchor::Spawn => self.spawn().map_block(),
                    Anchor::Point(p) => p,
                },
                radius,
            },
            Place::Box { min, max } => Area::Box { min, max },
        }
    }

    fn constraint(&self, kind: &Kind, resolved: &Resolved) -> ConstraintResult {
        let judged = match (kind, &resolved.target) {
            (Kind::Structures { count, region, .. }, Some(Target::Structure(target))) => {
                let within = self.within(region, resolved.anchor.as_ref());
                let starts = within
                    .bounds()
                    .map_or_else(Vec::new, |(min, max)| self.starts(target, min, max));
                let candidates: Vec<Candidate> = starts.iter().map(candidate).collect();
                let (matched, judged) = count_matches(*count, &within, &candidates);
                let mut features = Vec::new();
                for m in matched {
                    features.push(Feature::Structure(starts[m.index].clone()));
                }
                Judged { features, ..judged }
            }
            (Kind::Slime { count, region }, None) => {
                let within = self.within(region, resolved.anchor.as_ref());
                let chunks = within
                    .bounds()
                    .map_or_else(Vec::new, |(min, max)| self.slime_chunks(min, max));
                let candidates: Vec<Candidate> = chunks
                    .iter()
                    .map(|&c| Candidate {
                        position: c.map(|v| v * 16 + 8),
                        certain: true,
                        start: None,
                    })
                    .collect();
                let (matched, judged) = count_matches(*count, &within, &candidates);
                let features = matched
                    .iter()
                    .map(|m| Feature::SlimeChunk(chunks[m.index]))
                    .collect();
                Judged { features, ..judged }
            }
            (
                Kind::Cluster {
                    size, apart, place, ..
                },
                Some(Target::Structure(target)),
            ) => {
                let area = self.area(place);
                let (min, max) = area.bounds();
                let starts: Vec<StructureStart> = self
                    .starts(target, min, max)
                    .into_iter()
                    .filter(|s| area.distance(xz(s)).is_some())
                    .collect();
                let all: Vec<[i32; 2]> = starts.iter().map(xz).collect();
                let certain: Vec<[i32; 2]> = starts.iter().filter(|s| s.certain).map(xz).collect();
                let cap = *size as usize;
                let group = largest_group(&all, *apart, cap);
                let worst = largest_group(&certain, *apart, cap).len();
                let share = |g: usize| (1000 * g / cap) as u32; // g is at most cap
                Judged {
                    best: group.len() >= cap,
                    worst: worst >= cap,
                    best_sat: share(group.len()),
                    worst_sat: share(worst),
                    features: group
                        .iter()
                        .map(|&i| Feature::Structure(starts[i].clone()))
                        .collect(),
                }
            }
            (
                Kind::Biome {
                    present,
                    place,
                    sampling,
                    ..
                },
                Some(Target::Biome(biome)),
            ) => self.biome_presence(*present, &self.area(place), *sampling, biome.biome),
            (
                Kind::Coverage {
                    percent,
                    place,
                    sampling,
                    ..
                },
                Some(Target::Biome(biome)),
            ) => self.coverage(*percent, &self.area(place), *sampling, biome.biome),
            (Kind::Spawn { place }, None) => {
                let area = self.area(place);
                let spawn = *self.spawn();
                let d = area.distance(spawn.map_block());
                let sat = d.map_or(0, |d| points(d, area.radius()) as u32);
                Judged {
                    best: d.is_some(),
                    worst: d.is_some(),
                    best_sat: sat,
                    worst_sat: sat,
                    features: vec![Feature::Spawn(spawn)],
                }
            }
            _ => unreachable!("resolution gives each kind its target"),
        };
        let spawn_uncertain = uses_spawn(kind) && self.spawn().platform_dependent;
        judged.result(spawn_uncertain)
    }

    /// A region with its anchors found: a structure anchor's starts in their place.
    fn within(&self, region: &Region, anchor: Option<&StructureQuery>) -> Within {
        match (region, anchor) {
            (Region::Place(place), _) => Within::Area(self.area(place)),
            (Region::NearStructure { radius, place, .. }, Some(anchor)) => {
                let area = self.area(place);
                let (min, max) = area.bounds();
                let anchors = self
                    .starts(anchor, min, max)
                    .iter()
                    .filter(|s| area.distance(xz(s)).is_some())
                    .map(candidate)
                    .collect();
                Within::Anchors {
                    radius: *radius,
                    anchors,
                }
            }
            (Region::NearStructure { .. }, None) => {
                unreachable!("resolution gives each structure anchor its query")
            }
        }
    }

    /// The starts of `query` whose position is in `min..=max`, by set, then in chunk order.
    fn starts(&self, query: &StructureQuery, min: [i32; 2], max: [i32; 2]) -> Vec<StructureStart> {
        // position = chunk · 16 + offset, so the chunks run from ceil((min − offset) / 16) to
        // floor((max − offset) / 16). Every value is far from overflowing.
        let chunks = |offset: [i32; 3]| {
            let lo = |b: i32, o: i32| (b - o + 15).div_euclid(16);
            let hi = |b: i32, o: i32| (b - o).div_euclid(16);
            (
                [lo(min[0], offset[0]), lo(min[1], offset[2])],
                [hi(max[0], offset[0]), hi(max[1], offset[2])],
            )
        };
        self.session.starts_by_set(&query.targets, chunks)
    }

    /// The slime chunks whose centre is in `min..=max`.
    fn slime_chunks(&self, min: [i32; 2], max: [i32; 2]) -> Vec<[i32; 2]> {
        let lo = min.map(|b| (b - 8 + 15).div_euclid(16));
        let hi = max.map(|b| (b - 8).div_euclid(16));
        self.session.slime_chunks(lo, hi)
    }

    fn biome_presence(
        &self,
        present: bool,
        area: &Area,
        sampling: Sampling,
        target: Option<BiomeId>,
    ) -> Judged {
        // The nearest sample that possibly matches, and the nearest that certainly does.
        let mut possible: Option<([i32; 3], u32)> = None;
        let mut certain: Option<([i32; 3], u32)> = None;
        for (p, d) in area.grid(sampling.step) {
            let position = [p[0], sampling.y, p[1]];
            let (maybe, sure) = self.sample(position, target);
            let nearer = |best: Option<([i32; 3], u32)>| best.is_none_or(|(_, b)| d < b);
            if maybe && nearer(possible) {
                possible = Some((position, d));
            }
            if sure && nearer(certain) {
                certain = Some((position, d));
            }
        }
        let radius = area.radius();
        let sat = |hit: Option<([i32; 3], u32)>| hit.map_or(0, |(_, d)| points(d, radius) as u32);
        let mut features = Vec::new();
        for (position, _) in possible.into_iter().chain(certain) {
            if !features
                .iter()
                .any(|f| matches!(f, Feature::Biome { position: p, .. } if *p == position))
            {
                let lookup = self.lookup(position);
                features.push(Feature::Biome {
                    position,
                    biome: self.session.biome_names(&lookup),
                    certain: !lookup.is_tied(),
                });
            }
        }
        if present {
            Judged {
                best: possible.is_some(),
                worst: certain.is_some(),
                best_sat: sat(possible),
                worst_sat: sat(certain),
                features,
            }
        } else {
            let all_or_nothing = |holds: bool| if holds { 1000 } else { 0 };
            Judged {
                best: certain.is_none(),
                worst: possible.is_none(),
                best_sat: all_or_nothing(certain.is_none()),
                worst_sat: all_or_nothing(possible.is_none()),
                features,
            }
        }
    }

    fn coverage(
        &self,
        percent: u32,
        area: &Area,
        sampling: Sampling,
        target: Option<BiomeId>,
    ) -> Judged {
        let grid = area.grid(sampling.step);
        let (mut possible, mut certain) = (0_u64, 0_u64);
        for &(p, _) in &grid {
            let (maybe, sure) = self.sample([p[0], sampling.y, p[1]], target);
            possible += u64::from(maybe);
            certain += u64::from(sure);
        }
        let total = grid.len() as u64 * u64::from(percent);
        let holds = |m: u64| 100 * m >= total;
        let sat = |m: u64| (100_000 * m / total).min(1000) as u32;
        Judged {
            best: holds(possible),
            worst: holds(certain),
            best_sat: sat(possible),
            worst_sat: sat(certain),
            features: Vec::new(),
        }
    }

    fn lookup(&self, [x, y, z]: [i32; 3]) -> spyglass_biome::Lookup {
        self.session.biomes.biome_at(x >> 2, y >> 2, z >> 2)
    }

    /// Whether the sample at `position` possibly, and certainly, is `target`.
    fn sample(&self, position: [i32; 3], target: Option<BiomeId>) -> (bool, bool) {
        let Some(target) = target else {
            return (false, false);
        };
        let lookup = self.lookup(position);
        (
            lookup.candidates().contains(&target),
            !lookup.is_tied() && lookup.biome() == target,
        )
    }
}

fn xz(s: &StructureStart) -> [i32; 2] {
    [s.position[0], s.position[2]]
}

fn candidate(s: &StructureStart) -> Candidate {
    Candidate {
        position: xz(s),
        certain: s.certain,
        start: Some((s.set.clone(), s.chunk)),
    }
}

/// Judges a count over `candidates` in `within`, and lists the matches in the best case,
/// nearest first.
fn count_matches(
    count: Count,
    within: &Within,
    candidates: &[Candidate],
) -> (Vec<Matched>, Judged) {
    let mut matched: Vec<Matched> = candidates
        .iter()
        .enumerate()
        .filter_map(|(index, c)| {
            let (best, worst) = within.matches(c)?;
            Some(Matched { index, best, worst })
        })
        .collect();
    matched.sort_by_key(|m| m.best);
    let best: Vec<u32> = matched.iter().map(|m| m.best).collect();
    let worst: Vec<u32> = matched.iter().filter_map(|m| m.worst).collect();
    let judged = match count {
        Count::AtLeast(n) => {
            let radius = within.radius();
            Judged {
                best: best.len() >= n as usize,
                worst: worst.len() >= n as usize,
                best_sat: graded(best, n, radius),
                worst_sat: graded(worst, n, radius),
                features: Vec::new(),
            }
        }
        Count::No => {
            let all_or_nothing = |holds: bool| if holds { 1000 } else { 0 };
            Judged {
                best: worst.is_empty(),
                worst: best.is_empty(),
                best_sat: all_or_nothing(worst.is_empty()),
                worst_sat: all_or_nothing(best.is_empty()),
                features: Vec::new(),
            }
        }
    };
    (matched, judged)
}

/// The largest group of `points` (up to `cap` members) every two of which are at most `apart`
/// apart, as indices; the first such group in index order.
fn largest_group(points: &[[i32; 2]], apart: u32, cap: usize) -> Vec<usize> {
    let limit = u64::from(apart).pow(2);
    let near = |a: usize, b: usize| distance_sqr(points[a], points[b]) <= limit;
    let mut best = Vec::new();
    let mut group = Vec::new();
    for i in 0..points.len() {
        if best.len() >= cap {
            break;
        }
        let rest: Vec<usize> = (i + 1..points.len()).filter(|&j| near(i, j)).collect();
        group.push(i);
        extend(&mut group, &rest, &mut best, cap, &near);
        group.pop();
    }
    best
}

/// Grows `group` with members of `rest`, each near every member, keeping the largest in
/// `best`.
fn extend(
    group: &mut Vec<usize>,
    rest: &[usize],
    best: &mut Vec<usize>,
    cap: usize,
    near: &impl Fn(usize, usize) -> bool,
) {
    if group.len() > best.len() {
        *best = group.clone();
    }
    for (k, &next) in rest.iter().enumerate() {
        if best.len() >= cap || group.len() + rest.len() - k <= best.len() {
            return;
        }
        let narrowed: Vec<usize> = rest[k + 1..]
            .iter()
            .copied()
            .filter(|&j| near(next, j))
            .collect();
        group.push(next);
        extend(group, &narrowed, best, cap, near);
        group.pop();
    }
}

/// Whether a constraint depends on where the spawn is.
fn uses_spawn(kind: &Kind) -> bool {
    let place = |p: &Place| {
        matches!(
            p,
            Place::Within {
                anchor: Anchor::Spawn,
                ..
            }
        )
    };
    let region = |r: &Region| match r {
        Region::Place(p) | Region::NearStructure { place: p, .. } => place(p),
    };
    match kind {
        Kind::Structures { region: r, .. } | Kind::Slime { region: r, .. } => region(r),
        Kind::Cluster { place: p, .. }
        | Kind::Biome { place: p, .. }
        | Kind::Coverage { place: p, .. } => place(p),
        Kind::Spawn { .. } => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outcomes_from_both_cases() {
        let judged = |best, worst| Judged {
            best,
            worst,
            best_sat: 900,
            worst_sat: 400,
            features: Vec::new(),
        };
        let outcome = |best, worst, uncertain| judged(best, worst).result(uncertain).outcome;
        assert_eq!(outcome(true, true, false), Outcome::Met);
        assert_eq!(outcome(true, false, false), Outcome::PossiblyMet);
        assert_eq!(outcome(false, false, false), Outcome::Failed);
        // A platform-dependent spawn: possibly met either way, and nothing in the worst case.
        for (best, worst) in [(true, true), (false, false)] {
            let r = judged(best, worst).result(true);
            assert_eq!((r.outcome, r.best, r.worst), (Outcome::PossiblyMet, 900, 0));
        }
    }

    #[test]
    fn points_grade_by_distance() {
        assert_eq!(points(0, 100), 1000);
        assert_eq!(points(100, 100), 500);
        assert_eq!(points(33, 100), 835);
        assert_eq!(points(7, 0), 1000);
        // Two of three wanted, at the centre and at the edge.
        assert_eq!(graded(vec![100, 0], 3, 100), 500);
        assert_eq!(graded(vec![0, 0, 0, 100], 3, 100), 1000);
        assert_eq!(graded(vec![], 1, 100), 0);
    }

    #[test]
    fn distances_round_down() {
        assert_eq!(within([0, 0], [3, 4], 5), Some(5));
        assert_eq!(within([0, 0], [3, 4], 4), None);
        assert_eq!(within([0, 0], [1, 1], 1), None);
        assert_eq!(within([0, 0], [7, 7], 10), Some(9));
        assert_eq!(
            within([-30_000_000, 0], [-29_967_233, 0], 32_767),
            Some(32_767)
        );
    }

    #[test]
    fn grids_follow_the_place() {
        let disc = Area::Disc {
            centre: [100, -100],
            radius: 16,
        };
        let points: Vec<_> = disc.grid(16).into_iter().map(|(p, _)| p).collect();
        assert_eq!(
            points,
            [
                [100, -116],
                [84, -100],
                [100, -100],
                [116, -100],
                [100, -84]
            ]
        );
        let square = Area::Box {
            min: [0, 0],
            max: [20, 10],
        };
        assert_eq!(square.grid(10).len(), 6);
        assert_eq!(
            Area::Disc {
                centre: [5, 5],
                radius: 0
            }
            .grid(16),
            [([5, 5], 0)]
        );
    }

    #[test]
    fn groups_are_the_largest_close_set() {
        let pts = [[0, 0], [1000, 0], [100, 0], [1050, 0], [1100, 0], [50, 80]];
        assert_eq!(largest_group(&pts, 100, 64), [0, 2, 5]);
        assert_eq!(largest_group(&pts, 100, 2), [0, 2]);
        assert_eq!(largest_group(&pts, 10, 64).len(), 1);
        assert!(largest_group(&[], 10, 2).is_empty());
        // Every two within 100: the triangle [0,0], [100,0], [50,80] is (100, 94, 94).
        assert_eq!(largest_group(&pts[..3], 100, 64), [0, 2]);
        assert_eq!(largest_group(&[pts[0], pts[2], pts[5]], 100, 64), [0, 1, 2]);
    }
}
