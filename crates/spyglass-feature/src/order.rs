//! `FeatureSorter.buildFeaturesPerStep`: every possible biome's feature lists sorted into one
//! order per decoration step (`docs/specs/features.md#the-order`).

use std::collections::{BTreeMap, BTreeSet, HashMap};

/// A placed feature, by its place in [`crate::FeatureSettings`]' table: one per registry id,
/// and one per inline entry in a biome's lists.
pub type PlacedRef = usize;

/// `FeatureData`: a feature in a step, ordered by step, then by the number the feature got
/// when first seen. The same feature in two steps is two nodes.
type Node = (usize, u32);

/// Each step's features in decoration order, from each biome's lists in the biome source's
/// order (`possibleBiomes()`). `None` if the lists disagree: some features would have to run
/// both before and after others, which the game rejects ("Feature order cycle found").
pub fn features_per_step(biomes: &[Vec<Vec<PlacedRef>>]) -> Option<Vec<Vec<PlacedRef>>> {
    // Reference2IntOpenHashMap.computeIfAbsent with a counter: numbers by first sight.
    let mut numbers: HashMap<PlacedRef, u32> = HashMap::new();
    let mut features: BTreeMap<Node, PlacedRef> = BTreeMap::new();
    let mut edges: BTreeMap<Node, BTreeSet<Node>> = BTreeMap::new();
    let mut steps = 0;
    for lists in biomes {
        steps = steps.max(lists.len());
        let mut flat: Vec<Node> = Vec::new();
        for (step, list) in lists.iter().enumerate() {
            for &feature in list {
                let next = numbers.len() as u32;
                let number = *numbers.entry(feature).or_insert(next);
                features.insert((step, number), feature);
                flat.push((step, number));
            }
        }
        // Each entry before the next, across steps too.
        for (i, &node) in flat.iter().enumerate() {
            let after = edges.entry(node).or_default();
            if let Some(&next) = flat.get(i + 1) {
                after.insert(next);
            }
        }
    }
    let mut done: BTreeSet<Node> = BTreeSet::new();
    let mut path: BTreeSet<Node> = BTreeSet::new();
    let mut sorted: Vec<Node> = Vec::new();
    for &node in edges.keys() {
        if done.contains(&node) {
            continue;
        }
        if depth_first(&edges, &mut done, &mut path, &mut sorted, node) {
            return None;
        }
    }
    sorted.reverse();
    Some(
        (0..steps)
            .map(|step| {
                sorted
                    .iter()
                    .filter(|n| n.0 == step)
                    .map(|n| features[n])
                    .collect()
            })
            .collect(),
    )
}

/// `Graph.depthFirstSearch`: visits `node`'s successors in order, then emits `node`. True if
/// it finds a cycle.
fn depth_first(
    edges: &BTreeMap<Node, BTreeSet<Node>>,
    done: &mut BTreeSet<Node>,
    path: &mut BTreeSet<Node>,
    sorted: &mut Vec<Node>,
    node: Node,
) -> bool {
    if done.contains(&node) {
        return false;
    }
    if path.contains(&node) {
        return true;
    }
    path.insert(node);
    if let Some(after) = edges.get(&node) {
        for &next in after {
            if depth_first(edges, done, path, sorted, next) {
                return true;
            }
        }
    }
    path.remove(&node);
    done.insert(node);
    sorted.push(node);
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_biome_keeps_its_order() {
        let order = features_per_step(&[vec![vec![3, 1], vec![], vec![2]]]).unwrap();
        assert_eq!(order, [vec![3, 1], vec![], vec![2]]);
    }

    #[test]
    fn biomes_merge_into_one_order() {
        // a b d in one biome, a c d in another: c and b both after a and before d. Depth first
        // from the lowest node, a: its successors in number order (b, then c), each emitted
        // after what follows it, so reversed: a, c, b, d.
        let (a, b, c, d) = (10, 11, 12, 13);
        let order = features_per_step(&[vec![vec![a, b, d]], vec![vec![a, c, d]]]).unwrap();
        assert_eq!(order, [vec![a, c, b, d]]);
    }

    #[test]
    fn steps_split_the_order_and_link_across() {
        // The edge from the last of step 0 to the first of step 1 doesn't move either step.
        let order =
            features_per_step(&[vec![vec![1], vec![2, 3]], vec![vec![4], vec![3]]]).unwrap();
        assert_eq!(order, [vec![4, 1], vec![2, 3]]);
        // A feature in two steps is in both.
        let order = features_per_step(&[vec![vec![5], vec![5]]]).unwrap();
        assert_eq!(order, [vec![5], vec![5]]);
    }

    #[test]
    fn disagreeing_biomes_are_a_cycle() {
        assert_eq!(
            features_per_step(&[vec![vec![1, 2]], vec![vec![2, 1]]]),
            None
        );
    }

    #[test]
    fn no_biomes_no_steps() {
        assert_eq!(features_per_step(&[]), Some(Vec::new()));
    }
}
