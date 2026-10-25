//! The nearest parameter points to a target, with every tie (see `docs/specs/biome-lookup.md`).
//!
//! The game's `Climate.RTree` returns one point at the minimum fitness and, when several tie,
//! which one depends on the thread's previous search. This search is exact instead: it finds
//! every point at the minimum, and answers with the first of them in list order, which is what
//! `Climate.ParameterList.findValueBruteForce` returns.

use crate::parameters::{BiomeId, ParameterList, ParameterPoint, TargetPoint};

/// The biome for a target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lookup {
    /// One biome is nearest. Several points may tie, as long as they're all this biome.
    Unique(BiomeId),
    /// Points of two or more biomes tie at the minimum, so the game may give any of these. In
    /// list order of each biome's first tied point; the first is spyglass's answer.
    Tied(Box<[BiomeId]>),
}

impl Lookup {
    /// spyglass's answer: the unique biome, or the first tied one in list order.
    pub fn biome(&self) -> BiomeId {
        match self {
            Lookup::Unique(b) => *b,
            Lookup::Tied(set) => set[0],
        }
    }

    /// Whether the game's answer here depends on lookup history.
    pub fn is_tied(&self) -> bool {
        matches!(self, Lookup::Tied(_))
    }

    /// Every biome the game may give: one, or the tied set.
    pub fn candidates(&self) -> &[BiomeId] {
        match self {
            Lookup::Unique(b) => std::slice::from_ref(b),
            Lookup::Tied(set) => set,
        }
    }
}

/// The search's dimensions: the six climate parameters, and the offset against a target of 0,
/// as in `Climate.ParameterPoint.parameterSpace` and `Climate.TargetPoint.toParameterArray`.
const DIMENSIONS: usize = 7;
/// Points per leaf.
const LEAF_SIZE: usize = 8;
/// Above this, a squared distance could overflow an `i64` sum, which the tree's pruning can't
/// allow for, so the search falls back to brute force. Climate values never come near it.
const TREE_LIMIT: i64 = 1 << 30;

type Bounds = ([i64; DIMENSIONS], [i64; DIMENSIONS]);

fn bounds(p: &ParameterPoint) -> Bounds {
    let mut b = ([0; DIMENSIONS], [0; DIMENSIONS]);
    for (d, range) in p.parameters.iter().enumerate() {
        b.0[d] = range.min;
        b.1[d] = range.max;
    }
    b.0[6] = p.offset;
    b.1[6] = p.offset;
    b
}

/// The sum of squared per-dimension distances from `target` to a box. For a single point that
/// is exactly `ParameterPoint.fitness`.
fn distance(target: &[i64; DIMENSIONS], (min, max): &Bounds) -> i64 {
    let mut sum = 0i64;
    for d in 0..DIMENSIONS {
        let v = target[d];
        let above = v.wrapping_sub(max[d]);
        let below = min[d].wrapping_sub(v);
        let dist = if above > 0 { above } else { below.max(0) };
        sum = sum.wrapping_add(dist.wrapping_mul(dist));
    }
    sum
}

#[derive(Debug, Clone)]
enum Children {
    /// A range of [`SearchTree::points`].
    Leaf(usize, usize),
    Inner(usize, usize),
}

#[derive(Debug, Clone)]
struct Node {
    bounds: Bounds,
    children: Children,
}

/// A bounding-box tree over a parameter list's points, for exact nearest search with ties.
#[derive(Debug, Clone)]
pub struct SearchTree {
    /// Point bounds and list index, in tree order.
    points: Vec<(Bounds, u32)>,
    nodes: Vec<Node>,
    /// Each list index's biome.
    biomes: Vec<BiomeId>,
    /// The list's points, for brute force.
    list: Vec<ParameterPoint>,
}

impl SearchTree {
    /// Builds the tree. Each split is at the median, along the dimension that minimises the
    /// children's total box span (the cost `Climate.RTree.build` minimises). Any split gives the
    /// same answers; this one prunes well.
    pub fn new(list: &ParameterList) -> Self {
        let mut points: Vec<(Bounds, u32)> = list
            .points()
            .iter()
            .enumerate()
            .map(|(i, p)| (bounds(p), i as u32))
            .collect();
        let mut nodes = Vec::new();
        let len = points.len();
        build(&mut points, 0, len, &mut nodes);
        SearchTree {
            points,
            nodes,
            biomes: list.biomes().to_vec(),
            list: list.points().to_vec(),
        }
    }

    /// The biome nearest to `target`, with ties.
    pub fn lookup(&self, target: &TargetPoint) -> Lookup {
        self.lookup_with(target, &mut Vec::new())
    }

    /// [`lookup`](Self::lookup), reusing `hits` as scratch space.
    pub fn lookup_with(&self, target: &TargetPoint, hits: &mut Vec<u32>) -> Lookup {
        hits.clear();
        if target
            .0
            .iter()
            .any(|v| !(-TREE_LIMIT..=TREE_LIMIT).contains(v))
        {
            self.brute_force_hits(target, hits);
        } else {
            let mut t = [0; DIMENSIONS];
            t[..6].copy_from_slice(&target.0);
            let mut best = i64::MAX;
            self.visit(0, &t, &mut best, hits);
        }
        self.resolve(hits)
    }

    /// The same answer as [`lookup`](Self::lookup), from `ParameterPoint::fitness` on every point.
    pub fn brute_force(&self, target: &TargetPoint) -> Lookup {
        let mut hits = Vec::new();
        self.brute_force_hits(target, &mut hits);
        self.resolve(&mut hits)
    }

    fn brute_force_hits(&self, target: &TargetPoint, hits: &mut Vec<u32>) {
        let mut best = i64::MAX;
        for (i, p) in self.list.iter().enumerate() {
            offer(i as u32, p.fitness(target), &mut best, hits);
        }
    }

    fn visit(&self, node: usize, target: &[i64; DIMENSIONS], best: &mut i64, hits: &mut Vec<u32>) {
        match self.nodes[node].children {
            Children::Leaf(start, end) => {
                for (b, i) in &self.points[start..end] {
                    offer(*i, distance(target, b), best, hits);
                }
            }
            Children::Inner(l, r) => {
                let dl = distance(target, &self.nodes[l].bounds);
                let dr = distance(target, &self.nodes[r].bounds);
                let (first, second, near, far) = if dl <= dr {
                    (l, r, dl, dr)
                } else {
                    (r, l, dr, dl)
                };
                // Prune only strictly farther boxes: a box at exactly the best can hold a tie.
                if near <= *best {
                    self.visit(first, target, best, hits);
                }
                if far <= *best {
                    self.visit(second, target, best, hits);
                }
            }
        }
    }

    /// The lookup for the list indices in `hits`, every point at the minimum.
    fn resolve(&self, hits: &mut [u32]) -> Lookup {
        let biome = |i: &u32| self.biomes[*i as usize];
        if let [only] = hits {
            return Lookup::Unique(biome(only));
        }
        hits.sort_unstable();
        let mut set: Vec<BiomeId> = Vec::with_capacity(hits.len());
        for b in hits.iter().map(biome) {
            if !set.contains(&b) {
                set.push(b);
            }
        }
        if set.len() == 1 {
            Lookup::Unique(set[0])
        } else {
            Lookup::Tied(set.into_boxed_slice())
        }
    }
}

/// Records point `i` if it's at or below the best fitness so far.
fn offer(i: u32, fitness: i64, best: &mut i64, hits: &mut Vec<u32>) {
    if fitness < *best {
        *best = fitness;
        hits.clear();
    }
    if fitness == *best {
        hits.push(i);
    }
}

/// The box around `points`.
fn enclose(points: &[(Bounds, u32)]) -> Bounds {
    let mut b = ([i64::MAX; DIMENSIONS], [i64::MIN; DIMENSIONS]);
    for ((min, max), _) in points {
        for d in 0..DIMENSIONS {
            b.0[d] = b.0[d].min(min[d]);
            b.1[d] = b.1[d].max(max[d]);
        }
    }
    b
}

/// Total span of a box, over all dimensions.
fn cost((min, max): &Bounds) -> i64 {
    (0..DIMENSIONS).map(|d| max[d] - min[d]).sum()
}

/// Builds the node for `points[from..to]`, reordering that range, and returns its index.
fn build(points: &mut [(Bounds, u32)], from: usize, to: usize, nodes: &mut Vec<Node>) -> usize {
    let node = nodes.len();
    nodes.push(Node {
        bounds: enclose(&points[from..to]),
        children: Children::Leaf(from, to),
    });
    if to - from <= LEAF_SIZE {
        return node;
    }
    let mid = (from + to) / 2;
    let by_center = |d: usize| move |p: &(Bounds, u32)| (p.0.0[d] + p.0.1[d], p.1);
    let mut best = (i64::MAX, 0);
    for d in 0..DIMENSIONS {
        points[from..to].sort_by_key(by_center(d));
        let c = cost(&enclose(&points[from..mid])) + cost(&enclose(&points[mid..to]));
        if c < best.0 {
            best = (c, d);
        }
    }
    points[from..to].sort_by_key(by_center(best.1));
    let l = build(points, from, mid, nodes);
    let r = build(points, mid, to, nodes);
    nodes[node].children = Children::Inner(l, r);
    node
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parameters::Parameter;
    use serde_json::{Value, json};

    /// A small xorshift, so the tests need no dependency.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        fn range(&mut self, lo: i64, hi: i64) -> i64 {
            lo + (self.next() % (hi - lo + 1) as u64) as i64
        }
    }

    /// A random list on a coarse grid, so ties are common.
    fn random_list(rng: &mut Rng, n: usize, biomes: usize) -> ParameterList {
        let entries: Vec<_> = (0..n)
            .map(|_| {
                let mut range = || {
                    let a = rng.range(-4, 4) as f64 * 0.25;
                    let b = a + rng.range(0, 3) as f64 * 0.25;
                    json!([a, b.min(2.0)])
                };
                let ranges: [Value; 6] = std::array::from_fn(|_| range());
                let biome = format!("minecraft:b{}", rng.range(0, biomes as i64 - 1));
                let offset = rng.range(0, 2) as f64 * 0.25;
                let [t, h, c, e, d, w] = ranges;
                json!({
                    "biome": biome,
                    "parameters": {
                        "temperature": t, "humidity": h, "continentalness": c,
                        "erosion": e, "depth": d, "weirdness": w, "offset": offset,
                    }
                })
            })
            .collect();
        ParameterList::from_report(&json!({ "biomes": entries }), "random").unwrap()
    }

    #[test]
    fn tree_equals_brute_force() {
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
        let mut tied = 0;
        for n in [1, 2, 9, 40, 300] {
            let list = random_list(&mut rng, n, 5);
            let tree = SearchTree::new(&list);
            for _ in 0..2000 {
                // Targets on the grid tie often; others rarely.
                let step = if rng.next().is_multiple_of(2) {
                    2500
                } else {
                    1
                };
                let target = TargetPoint(std::array::from_fn(|_| {
                    rng.range(-12_000, 12_000) / step * step
                }));
                let (fast, slow) = (tree.lookup(&target), tree.brute_force(&target));
                assert_eq!(fast, slow, "{target:?} with {n} points");
                tied += usize::from(fast.is_tied());
            }
        }
        assert!(
            tied > 100,
            "only {tied} ties; the test isn't exercising them"
        );
    }

    #[test]
    fn ties_answer_with_the_first_in_list_order() {
        let point = |t: [i64; 2], offset| ParameterPoint {
            parameters: std::array::from_fn(|i| {
                let v = if i < 2 { t[i] } else { 0 };
                Parameter { min: v, max: v }
            }),
            offset,
        };
        let report = json!({"biomes": [
            {"biome": "minecraft:a", "parameters": {"temperature": 0.1, "humidity": 0.0,
                "continentalness": 0, "erosion": 0, "depth": 0, "weirdness": 0, "offset": 0}},
            {"biome": "minecraft:b", "parameters": {"temperature": -0.1, "humidity": 0.0,
                "continentalness": 0, "erosion": 0, "depth": 0, "weirdness": 0, "offset": 0}},
            {"biome": "minecraft:a", "parameters": {"temperature": 0.0, "humidity": 0.1,
                "continentalness": 0, "erosion": 0, "depth": 0, "weirdness": 0, "offset": 0}},
            {"biome": "minecraft:c", "parameters": {"temperature": 0.5, "humidity": 0.0,
                "continentalness": 0, "erosion": 0, "depth": 0, "weirdness": 0, "offset": 0.0001}}
        ]});
        let list = ParameterList::from_report(&report, "t").unwrap();
        assert_eq!(list.points()[0], point([1000, 0], 0));
        let tree = SearchTree::new(&list);
        let (a, b, c) = (BiomeId(0), BiomeId(1), BiomeId(2));
        // Equidistant from a and b (fitness 1000²): tied, a first.
        let origin = TargetPoint([0; 6]);
        assert_eq!(tree.lookup(&origin), Lookup::Tied(Box::new([a, b])));
        assert_eq!(tree.lookup(&origin).biome(), a);
        // Two points of a tie with each other only: unique.
        let target = TargetPoint([1000, 1000, 0, 0, 0, 0]);
        assert_eq!(tree.lookup(&target), Lookup::Unique(a));
        // The offset counts as a distance: at c's own point, its fitness is 1.
        let near_c = TargetPoint([5000, 0, 0, 0, 0, 0]);
        assert_eq!(list.points()[3].fitness(&near_c), 1);
        assert_eq!(tree.lookup(&near_c), Lookup::Unique(c));
        // Out of the tree's range, brute force answers.
        let far = TargetPoint([i64::MAX, 0, 0, 0, 0, 0]);
        assert_eq!(tree.lookup(&far), tree.brute_force(&far));
    }
}
