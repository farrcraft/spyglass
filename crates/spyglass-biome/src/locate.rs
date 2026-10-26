//! The game's nearest-biome search (`BiomeSource.findClosestBiome3d`, used by `/locate biome`)
//! and the iteration orders it uses. See `docs/specs/locate.md`.

use crate::parameters::BiomeId;
use crate::search::Lookup;
use crate::source::Biomes;

/// `BlockPos.spiralAround(ZERO, radius, EAST, SOUTH)` as `(x, z)` offsets: the centre, then
/// outwards east, south, west, west, north, north, east × 3, ..., covering the square of
/// `radius` exactly and ending at `(radius, -radius)`. As in the game, leg `k` of
/// `0..=4 * radius` yields `k / 2 + 1` positions, the first a step in the previous leg's
/// direction.
pub fn spiral_around(radius: i32) -> impl Iterator<Item = [i32; 2]> {
    // East (+x), south (+z), west, north.
    const DIRECTIONS: [[i32; 2]; 4] = [[1, 0], [0, 1], [-1, 0], [0, -1]];
    let legs = radius.wrapping_mul(4);
    // The cursor starts one step south of the centre, and the first move is north.
    let mut cursor = [0_i32, 1];
    let mut leg = -1_i32;
    let mut leg_size = 0;
    let mut leg_index = 0;
    std::iter::from_fn(move || {
        let [dx, dz] = DIRECTIONS[(leg + 4).rem_euclid(4) as usize];
        cursor = [cursor[0].wrapping_add(dx), cursor[1].wrapping_add(dz)];
        if leg_index >= leg_size {
            if leg >= legs {
                return None;
            }
            leg += 1;
            leg_index = 0;
            leg_size = leg / 2 + 1;
        }
        leg_index += 1;
        Some(cursor)
    })
}

/// `Mth.outFromOrigin(origin, lower, upper, step)`: `origin` clamped to `lower..=upper`, then
/// values alternately above and below it, `step` further out each time, continuing one way
/// once the other leaves the range. Empty if `lower > upper` or `step < 1`, where the game
/// throws.
pub fn out_from_origin(
    origin: i32,
    lower: i32,
    upper: i32,
    step: i32,
) -> impl Iterator<Item = i32> {
    let valid = lower <= upper && step >= 1;
    let start = origin.clamp(lower, upper.max(lower));
    let has_next = move |value: i32| {
        let d = start.wrapping_sub(value).wrapping_abs();
        start.wrapping_sub(d) >= lower || start.wrapping_add(d) <= upper
    };
    let next = move |&value: &i32| {
        let below = value <= start;
        let d = start.wrapping_sub(value).wrapping_abs();
        let can_go_up = start.wrapping_add(d).wrapping_add(step) <= upper;
        if !(below && can_go_up) {
            let down = start
                .wrapping_sub(d)
                .wrapping_sub(if below { step } else { 0 });
            if down >= lower {
                return Some(down);
            }
        }
        Some(start.wrapping_add(d).wrapping_add(step))
    };
    std::iter::successors(valid.then_some(start), next).take_while(move |&v| has_next(v))
}

/// A cell in [`Biomes::find_closest_biome_3d`]'s answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BiomeHit {
    /// The block position the game would report.
    pub position: [i32; 3],
    /// The lookup there.
    pub biome: Lookup,
    /// Whether the game certainly matches here: no tied biome could fail the predicate.
    /// Otherwise a tied cell's biomes disagree, and the game's answer depends on lookup history.
    pub certain: bool,
}

/// The answer to [`Biomes::find_closest_biome_3d`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BiomeSearch {
    /// Cells that may be the game's answer, in its search order, ending at the first certain
    /// match. The game's answer is the first of them that matches in its run. If the last one
    /// isn't certain, the search ran out (or reached [`MAX_UNCERTAIN`]), and the game may also
    /// find nothing, or something later.
    pub hits: Vec<BiomeHit>,
}

impl BiomeSearch {
    /// The certain match that ends the list, if there is one.
    pub fn certain(&self) -> Option<&BiomeHit> {
        self.hits.last().filter(|h| h.certain)
    }
}

/// Where [`Biomes::find_closest_biome_3d`] looks: `BiomeSource.findClosestBiome3d`'s
/// arguments and the level's height, in blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchVolume {
    /// The centre.
    pub from: [i32; 3],
    /// How far out, horizontally.
    pub radius: i32,
    /// Between columns. Must be positive.
    pub horizontal_step: i32,
    /// Between heights in a column. Must be positive.
    pub vertical_step: i32,
    /// The level's lowest block.
    pub min_y: i32,
    /// The level's highest block.
    pub max_y: i32,
}

/// The most uncertain hits [`Biomes::find_closest_biome_3d`] collects before giving up.
pub const MAX_UNCERTAIN: usize = 16;

impl Biomes<'_> {
    /// `BiomeSource.findClosestBiome3d(from, radius, horizontal_step, vertical_step, matches)`,
    /// with the volume's fields:
    /// columns at `from + horizontal_step * offset` for offsets from
    /// [`spiral_around`]`(radius / horizontal_step)` (floor division), and in each column the
    /// heights [`out_from_origin`]`(from.y, min_y + 1, max_y + 1, vertical_step)`, where `min_y`
    /// and `max_y` are the level's lowest and highest block. The first quart whose biome
    /// matches is the game's answer, at that block position. Nothing if no biome in the
    /// parameter list matches.
    ///
    /// A tied cell is a hit if its candidates disagree on `matches`, so the list holds every
    /// cell the game could stop at. Steps must be positive.
    pub fn find_closest_biome_3d(
        &self,
        volume: SearchVolume,
        matches: impl Fn(BiomeId) -> bool,
    ) -> BiomeSearch {
        let SearchVolume {
            from,
            radius,
            horizontal_step,
            vertical_step,
            min_y,
            max_y,
        } = volume;
        assert!(
            horizontal_step > 0 && vertical_step > 0,
            "steps must be positive"
        );
        let mut search = BiomeSearch::default();
        let names = self.source().parameters().names().len();
        if !(0..names).any(|b| matches(BiomeId(b as u16))) {
            return search;
        }
        let ys: Vec<i32> = out_from_origin(
            from[1],
            min_y.wrapping_add(1),
            max_y.wrapping_add(1),
            vertical_step,
        )
        .collect();
        for [ox, oz] in spiral_around(radius.div_euclid(horizontal_step)) {
            let x = from[0].wrapping_add(ox.wrapping_mul(horizontal_step));
            let z = from[2].wrapping_add(oz.wrapping_mul(horizontal_step));
            for &y in &ys {
                let target =
                    crate::TargetPoint::quantize(self.climate().sample(x >> 2, y >> 2, z >> 2));
                let lookup = self.source().lookup(&target);
                let hit = matches(lookup.biome());
                let disagree = lookup.candidates().iter().any(|&b| matches(b) != hit);
                if hit || disagree {
                    let certain = !disagree;
                    search.hits.push(BiomeHit {
                        position: [x, y, z],
                        biome: lookup,
                        certain,
                    });
                    if certain || search.hits.len() >= MAX_UNCERTAIN {
                        return search;
                    }
                }
            }
        }
        search
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spiral_order() {
        let first: Vec<[i32; 2]> = spiral_around(2).take(10).collect();
        assert_eq!(
            first,
            [
                [0, 0],
                [1, 0],
                [1, 1],
                [0, 1],
                [-1, 1],
                [-1, 0],
                [-1, -1],
                [0, -1],
                [1, -1],
                [2, -1]
            ]
        );
        // Legs 0..=4r of length leg / 2 + 1: exactly the square.
        let mut all: Vec<[i32; 2]> = spiral_around(2).collect();
        assert_eq!(all.len(), 25);
        assert_eq!(all[24], [2, -2]);
        all.sort();
        all.dedup();
        assert_eq!(all.len(), 25);
        assert!(all.iter().all(|p| p[0].abs() <= 2 && p[1].abs() <= 2));
        assert_eq!(spiral_around(0).collect::<Vec<_>>(), [[0, 0]]);
        assert_eq!(spiral_around(200).count(), 401 * 401);
    }

    #[test]
    fn out_from_origin_alternates() {
        let ys: Vec<i32> = out_from_origin(64, -63, 321, 64).collect();
        assert_eq!(ys, [64, 128, 0, 192, 256, 320]);
        let ys: Vec<i32> = out_from_origin(0, -2, 2, 1).collect();
        assert_eq!(ys, [0, 1, -1, 2, -2]);
        // Clamped, then only one way.
        let ys: Vec<i32> = out_from_origin(-500, -63, 321, 64).collect();
        assert_eq!(ys, [-63, 1, 65, 129, 193, 257, 321]);
        let ys: Vec<i32> = out_from_origin(1000, 0, 10, 4).collect();
        assert_eq!(ys, [10, 6, 2]);
        assert_eq!(out_from_origin(5, 5, 5, 1).collect::<Vec<_>>(), [5]);
        assert_eq!(out_from_origin(0, 1, 0, 1).count(), 0);
        assert_eq!(out_from_origin(0, 0, 1, 0).count(), 0);
    }
}
