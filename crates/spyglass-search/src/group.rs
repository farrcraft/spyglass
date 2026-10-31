//! Clusters: the largest group of points that are every two close together
//! (`docs/specs/search.md#outcomes`).

/// `dx² + dz²` (or `dx² + dy² + dz²`, for points with a height) in 64 bits.
pub fn distance_sqr<const N: usize>(a: [i32; N], b: [i32; N]) -> u64 {
    (0..N)
        .map(|i| (i64::from(a[i]) - i64::from(b[i])).pow(2))
        .sum::<i64>()
        .unsigned_abs()
}

/// The largest group of `points` (up to `cap` members) every two of which are at most `apart`
/// blocks apart, as indices: the first such group in index order.
///
/// A branch-and-bound search: each point in turn starts a group, which grows only with later
/// points near every member so far. A branch stops once even all its remaining candidates
/// couldn't beat the best group found, or once a group reaches `cap`. The point counts are
/// small (starts or features in one place), so the worst case doesn't arise in practice.
pub fn largest_group<const N: usize>(points: &[[i32; N]], apart: u32, cap: usize) -> Vec<usize> {
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
        best.clone_from(group);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_in_three_dimensions() {
        let pts = [[0, 0, 0], [3, 60, 4], [3, -4, 4], [0, 10, 0]];
        assert_eq!(distance_sqr(pts[0], pts[2]), 41);
        // Close in x and z, but not in y.
        assert_eq!(largest_group(&pts, 10, 64), [0, 2]);
        assert_eq!(largest_group(&pts, 15, 64), [0, 2, 3]);
        let far = [
            [-30_000_000, -64, -30_000_000],
            [30_000_000, 319, 30_000_000],
        ];
        assert_eq!(
            distance_sqr(far[0], far[1]),
            2 * 60_000_000_u64.pow(2) + 383 * 383
        );
    }

    #[test]
    fn groups_are_the_largest_close_set() {
        let pts = [[0, 0], [1000, 0], [100, 0], [1050, 0], [1100, 0], [50, 80]];
        // [0,0], [100,0] and [50,80] are 100, 94 and 94 apart.
        assert_eq!(largest_group(&pts, 100, 64), [0, 2, 5]);
        assert_eq!(largest_group(&pts, 100, 2), [0, 2]);
        assert_eq!(largest_group(&pts, 10, 64).len(), 1);
        assert!(largest_group::<2>(&[], 10, 2).is_empty());
        assert_eq!(largest_group(&pts[1..5], 100, 64), [0, 2, 3]);
        assert_eq!(largest_group(&pts[1..5], 99, 64).len(), 2);
    }

    #[test]
    fn groups_match_brute_force() {
        // Every subset of a dozen scattered points, against the search.
        let mut state = 0x9e37_79b9_u32;
        for round in 0..200 {
            let pts: Vec<[i32; 2]> = (0..12)
                .map(|_| {
                    state ^= state << 13;
                    state ^= state >> 17;
                    state ^= state << 5;
                    [(state % 400) as i32, ((state >> 9) % 400) as i32]
                })
                .collect();
            let apart = 60 + round % 120;
            let limit = u64::from(apart).pow(2);
            let largest = (0_u32..1 << pts.len())
                .filter(|mask| {
                    let members: Vec<usize> =
                        (0..pts.len()).filter(|i| mask >> i & 1 == 1).collect();
                    members.iter().all(|&a| {
                        members
                            .iter()
                            .all(|&b| distance_sqr(pts[a], pts[b]) <= limit)
                    })
                })
                .map(u32::count_ones)
                .max()
                .unwrap() as usize;
            let group = largest_group(&pts, apart, 64);
            assert_eq!(group.len(), largest, "round {round}");
            assert_eq!(largest_group(&pts, apart, 3).len(), largest.min(3));
        }
    }
}
