//! Structure placement (`StructurePlacement`): which chunks a structure set may start in. Depends
//! only on the seed, except for concentric rings, which also move towards preferred biomes.
//! See `docs/specs/structure-placement.md`.

use std::collections::HashSet;
use std::f64::consts::PI;
use std::sync::OnceLock;

use spyglass_biome::{Biomes, HorizontalArea};
use spyglass_rng::{LegacyRandomSource, RandomSource, worldgen};

use crate::biomes::BiomeSet;
use crate::load::StructureSets;

/// A structure set's placement (`AbstractSpreadingStructurePlacement` and its subclasses).
#[derive(Debug, Clone)]
pub struct Placement {
    /// `locate_offset`: added to a chunk's minimum block for locate results.
    pub locate_offset: [i32; 3],
    /// `frequency_reduction_method`.
    pub frequency_reduction: FrequencyReduction,
    /// `frequency`, in 0..=1. Below 1, [`frequency_reduction`](Self::frequency_reduction)
    /// thins the placement chunks.
    pub frequency: f32,
    /// `salt`.
    pub salt: i32,
    /// `exclusion_zone`.
    pub exclusion_zone: Option<ExclusionZone>,
    /// The placement type and its own fields.
    pub kind: PlacementKind,
}

/// The placement types.
#[derive(Debug, Clone)]
pub enum PlacementKind {
    /// `minecraft:random_spread`.
    RandomSpread(RandomSpread),
    /// `minecraft:concentric_rings`.
    ConcentricRings(ConcentricRings),
}

/// `RandomSpreadStructurePlacement`: one candidate chunk per `spacing` × `spacing` cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RandomSpread {
    /// Cell size in chunks.
    pub spacing: i32,
    /// Minimum gap: the candidate is within the first `spacing - separation` chunks of its cell.
    pub separation: i32,
    /// How the offset in the cell is drawn.
    pub spread_type: SpreadType,
}

/// `RandomSpreadType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpreadType {
    /// `nextInt(n)`.
    Linear,
    /// `(nextInt(n) + nextInt(n)) / 2`.
    Triangular,
}

impl SpreadType {
    /// `RandomSpreadType.evaluate`.
    fn evaluate(self, random: &mut LegacyRandomSource, limit: i32) -> i32 {
        match self {
            SpreadType::Linear => random.next_int_bounded(limit),
            SpreadType::Triangular => {
                (random.next_int_bounded(limit) + random.next_int_bounded(limit)) / 2
            }
        }
    }
}

/// `ConcentricRingsStructurePlacement`: `count` positions on rings around the origin.
#[derive(Debug, Clone)]
pub struct ConcentricRings {
    /// Ring spacing, in chunks.
    pub distance: i32,
    /// Positions on the first ring.
    pub spread: i32,
    /// Total positions.
    pub count: i32,
    /// Biomes each position moves towards.
    pub preferred_biomes: BiomeSet,
}

/// `AbstractSpreadingStructurePlacement.FrequencyReductionMethod`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrequencyReduction {
    /// `default`: `probabilityReducer`.
    Default,
    /// `legacy_type_1`: `legacyPillagerOutpostReducer`.
    LegacyType1,
    /// `legacy_type_2`: `legacyArbitrarySaltProbabilityReducer`.
    LegacyType2,
    /// `legacy_type_3`: `legacyProbabilityReducerWithDouble`.
    LegacyType3,
}

impl FrequencyReduction {
    /// `FrequencyReducer.shouldGenerate(seed, salt, chunkX, chunkZ, frequency)`.
    pub fn should_generate(self, seed: i64, salt: i32, x: i32, z: i32, frequency: f32) -> bool {
        match self {
            // The game passes (salt, x, z) as setLargeFeatureWithSalt's (x, z, salt).
            FrequencyReduction::Default => {
                worldgen::large_feature_with_salt(seed, salt, x, z).next_float() < frequency
            }
            FrequencyReduction::LegacyType1 => {
                let (i, j) = (x >> 4, z >> 4);
                let mut random = LegacyRandomSource::new(i64::from(i ^ j << 4) ^ seed);
                random.next_int();
                // `(int)(1.0F / frequency)`: Rust's `as` saturates like Java's f2i.
                random.next_int_bounded((1.0_f32 / frequency) as i32) == 0
            }
            FrequencyReduction::LegacyType2 => {
                worldgen::large_feature_with_salt(seed, x, z, 10_387_320).next_float() < frequency
            }
            FrequencyReduction::LegacyType3 => {
                worldgen::large_feature(seed, x, z).next_double() < f64::from(frequency)
            }
        }
    }
}

/// `AbstractSpreadingStructurePlacement.ExclusionZone`: no placement within `chunk_count`
/// chunks of another set's placement chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExclusionZone {
    /// Index of the other set in [`StructureSets::sets`].
    pub other_set: usize,
    /// Distance in chunks, along each axis, in 1..=16.
    pub chunk_count: i32,
}

impl RandomSpread {
    /// `getPotentialStructureChunk(seed, chunkX, chunkZ)`: the candidate chunk of the cell
    /// containing `(x, z)`.
    pub fn potential_chunk(&self, seed: i64, salt: i32, x: i32, z: i32) -> [i32; 2] {
        let cell_x = x.div_euclid(self.spacing);
        let cell_z = z.div_euclid(self.spacing);
        self.cell_chunk(seed, salt, cell_x, cell_z)
    }

    /// The candidate chunk of cell `(cell_x, cell_z)`.
    pub fn cell_chunk(&self, seed: i64, salt: i32, cell_x: i32, cell_z: i32) -> [i32; 2] {
        let mut random = worldgen::large_feature_with_salt(seed, cell_x, cell_z, salt);
        let limit = self.spacing - self.separation;
        let dx = self.spread_type.evaluate(&mut random, limit);
        let dz = self.spread_type.evaluate(&mut random, limit);
        [
            cell_x.wrapping_mul(self.spacing).wrapping_add(dx),
            cell_z.wrapping_mul(self.spacing).wrapping_add(dz),
        ]
    }
}

/// `Math.round(double)`: the closest `long`, ties towards positive infinity, saturating; NaN is 0.
pub(crate) fn java_round(x: f64) -> i64 {
    let floor = x.floor();
    // Exact: x - floor(x) has no rounding error for any finite x.
    let rounded = if x - floor >= 0.5 { floor + 1.0 } else { floor };
    rounded as i64
}

/// `(int) Math.round(trig * scale)`, where `trig` is a `Math.sin` or `Math.cos` result, and the
/// other value it rounds to if `trig` moves by up to 4 ulps either way (ADR 0012): `None` when
/// the answer is the same on every platform.
pub(crate) fn round_trig_product(trig: f64, scale: f64) -> (i32, Option<i32>) {
    let answer = java_round(trig * scale) as i32;
    let (mut up, mut down) = (trig, trig);
    let mut other = None;
    for _ in 0..4 {
        up = up.next_up();
        down = down.next_down();
        for v in [up, down] {
            let rounded = java_round(v * scale) as i32;
            if rounded != answer {
                other = Some(rounded);
            }
        }
    }
    (answer, other)
}

/// A concentric-ring position for one seed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RingPosition {
    /// The chunk.
    pub chunk: [i32; 2],
    /// Whether the game's answer may differ by platform, because the ring point's `Math.round`
    /// of a sine or cosine is within a few ulps of a boundary (ADR 0012).
    pub platform_dependent: bool,
    /// Whether the game's answer may differ between runs, because a tied biome cell in the
    /// preferred-biome search could go either way.
    pub tie_dependent: bool,
    /// Every other chunk the game may use for this position, on another platform or after
    /// other tie outcomes.
    pub alternatives: Vec<[i32; 2]>,
    /// Whether [`alternatives`](Self::alternatives) is complete; false when a search had more
    /// tied cells than spyglass replays.
    pub alternatives_known: bool,
}

impl ConcentricRings {
    /// `ChunkGeneratorStructureState.generateRingPositions`: every position for `seed`, in the
    /// game's order.
    pub fn positions(&self, seed: i64, biomes: &Biomes<'_>) -> Vec<RingPosition> {
        let mut positions = Vec::with_capacity(self.count.max(0) as usize);
        if self.count == 0 {
            return positions;
        }
        let distance = self.distance;
        let count = self.count;
        let mut spread = self.spread;
        // RandomSource.create() then setSeed(seed): a legacy source.
        let mut random = LegacyRandomSource::new(seed);
        let mut angle = random.next_double() * PI * 2.0;
        let mut position_in_circle = 0;
        let mut circle = 0;
        for i in 0..count {
            let ring_distance = f64::from(
                (4 * distance).wrapping_add(distance.wrapping_mul(circle).wrapping_mul(6)),
            ) + (random.next_double() - 0.5) * (f64::from(distance) * 2.5);
            let (x, other_x) = round_trig_product(angle.cos(), ring_distance);
            let (z, other_z) = round_trig_product(angle.sin(), ring_distance);
            let fork = random.fork();
            let mut position = self.settle(x, z, biomes, &mut fork.clone());
            position.platform_dependent = other_x.is_some() || other_z.is_some();
            // Another platform's ring point settles from the same fork.
            for ax in [Some(x), other_x].into_iter().flatten() {
                for az in [Some(z), other_z].into_iter().flatten() {
                    if [ax, az] == [x, z] {
                        continue;
                    }
                    let other = self.settle(ax, az, biomes, &mut fork.clone());
                    position.alternatives_known &= other.alternatives_known;
                    for chunk in std::iter::once(other.chunk).chain(other.alternatives) {
                        if chunk != position.chunk && !position.alternatives.contains(&chunk) {
                            position.alternatives.push(chunk);
                        }
                    }
                }
            }
            positions.push(position);
            angle += 2.0 * PI / f64::from(spread);
            position_in_circle += 1;
            if position_in_circle == spread {
                circle += 1;
                position_in_circle = 0;
                spread += 2 * spread / (circle + 1);
                spread = spread.min(count - i);
                angle += random.next_double() * PI * 2.0;
            }
        }
        positions
    }

    /// The ring point's move towards a preferred biome: `findBiomeHorizontal` from the chunk
    /// centre at y 0, radius 112, through the forked random. Not yet marked platform-dependent.
    fn settle(
        &self,
        x: i32,
        z: i32,
        biomes: &Biomes<'_>,
        random: &mut LegacyRandomSource,
    ) -> RingPosition {
        let area = HorizontalArea {
            center: [(x << 4).wrapping_add(8), 0, (z << 4).wrapping_add(8)],
            radius: 112,
            increment: 1,
            find_closest: false,
        };
        let search = biomes.find_biome_horizontal(
            area,
            |biome| self.preferred_biomes.contains(biome),
            random,
        );
        // No match leaves the ring point where it was.
        let chunk_of = |p: Option<[i32; 3]>| p.map_or([x, z], |p| [p[0] >> 4, p[2] >> 4]);
        let chunk = chunk_of(search.found.as_ref().map(|f| f.position));
        let mut alternatives = Vec::new();
        for other in search.alternatives.iter().map(|&p| chunk_of(p)) {
            if other != chunk && !alternatives.contains(&other) {
                alternatives.push(other);
            }
        }
        RingPosition {
            chunk,
            platform_dependent: false,
            tie_dependent: search.tie_dependent(),
            alternatives,
            alternatives_known: search.alternatives_known,
        }
    }
}

/// A dimension's placements bound to a seed. Each concentric-ring set's positions are computed
/// the first time they're needed, then kept.
#[derive(Debug, Clone)]
pub struct Placements<'s> {
    sets: &'s StructureSets,
    seed: i64,
    biomes: Biomes<'s>,
    /// Ring positions by set index, once computed.
    rings: Vec<OnceLock<Vec<RingPosition>>>,
}

impl StructureSets {
    /// The placements for `seed`. `biomes` must be the dimension's biomes for the same seed;
    /// concentric rings need them.
    pub fn with_seed<'s>(&'s self, seed: i64, biomes: &Biomes<'s>) -> Placements<'s> {
        Placements {
            sets: self,
            seed,
            biomes: biomes.clone(),
            rings: self.sets().iter().map(|_| OnceLock::new()).collect(),
        }
    }
}

impl Placements<'_> {
    /// The seed.
    pub fn seed(&self) -> i64 {
        self.seed
    }

    /// The structure sets.
    pub fn sets(&self) -> &StructureSets {
        self.sets
    }

    /// A concentric-ring set's positions, or `None` for other placement types. The first call
    /// for a set computes them, which takes a while (128 biome searches for strongholds).
    pub fn ring_positions(&self, set: usize) -> Option<&[RingPosition]> {
        match &self.sets.sets()[set].placement.kind {
            PlacementKind::ConcentricRings(rings) => {
                Some(self.rings[set].get_or_init(|| rings.positions(self.seed, &self.biomes)))
            }
            PlacementKind::RandomSpread(_) => None,
        }
    }

    /// `StructurePlacement.isStructureChunk`: whether set `set` may start in chunk `(x, z)`:
    /// the placement test, then frequency, then the exclusion zone.
    pub fn is_structure_chunk(&self, set: usize, x: i32, z: i32) -> bool {
        let placement = &self.sets.sets()[set].placement;
        let placed = match &placement.kind {
            PlacementKind::RandomSpread(spread) => {
                spread.potential_chunk(self.seed, placement.salt, x, z) == [x, z]
            }
            PlacementKind::ConcentricRings(_) => self
                .ring_positions(set)
                .is_some_and(|ring| ring.iter().any(|p| p.chunk == [x, z])),
        };
        placed
            && (placement.frequency >= 1.0
                || placement.frequency_reduction.should_generate(
                    self.seed,
                    placement.salt,
                    x,
                    z,
                    placement.frequency,
                ))
            && !placement.exclusion_zone.is_some_and(|zone| {
                self.has_structure_chunk_in_range(zone.other_set, x, z, zone.chunk_count)
            })
    }

    /// `ChunkGeneratorStructureState.hasStructureChunkInRange`.
    fn has_structure_chunk_in_range(&self, set: usize, x: i32, z: i32, range: i32) -> bool {
        let around = |c: i32| c.wrapping_sub(range)..=c.wrapping_add(range);
        around(x).any(|i| around(z).any(|j| self.is_structure_chunk(set, i, j)))
    }

    /// Every chunk in `min..=max` (chunk coordinates, inclusive) where set `set` may start, in
    /// x-then-z order of cells.
    pub fn structure_chunks_in(&self, set: usize, min: [i32; 2], max: [i32; 2]) -> Vec<[i32; 2]> {
        let placement = &self.sets.sets()[set].placement;
        let inside =
            |c: &[i32; 2]| (min[0]..=max[0]).contains(&c[0]) && (min[1]..=max[1]).contains(&c[1]);
        let mut chunks: Vec<[i32; 2]> = match &placement.kind {
            PlacementKind::RandomSpread(spread) => {
                let cells = |lo: i32, hi: i32| {
                    lo.div_euclid(spread.spacing)..=hi.div_euclid(spread.spacing)
                };
                cells(min[1], max[1])
                    .flat_map(|cz| cells(min[0], max[0]).map(move |cx| (cx, cz)))
                    .map(|(cx, cz)| spread.cell_chunk(self.seed, placement.salt, cx, cz))
                    .filter(inside)
                    .collect()
            }
            PlacementKind::ConcentricRings(_) => self
                .ring_positions(set)
                .into_iter()
                .flatten()
                .map(|p| p.chunk)
                .filter(inside)
                .collect(),
        };
        // Ring positions can repeat; keep each chunk's first.
        let mut seen = HashSet::new();
        chunks.retain(|&c @ [x, z]| seen.insert(c) && self.is_structure_chunk(set, x, z));
        chunks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn java_round_ties_up() {
        assert_eq!(java_round(2.5), 3);
        assert_eq!(java_round(-2.5), -2);
        assert_eq!(java_round(-2.500_000_000_000_1), -3);
        // The value just below 0.5 rounds to 0, which floor(x + 0.5) gets wrong.
        assert_eq!(java_round(0.499_999_999_999_999_94), 0);
        assert_eq!(java_round(f64::NAN), 0);
        assert_eq!(java_round(1e300), i64::MAX);
        assert_eq!(java_round(-1e300), i64::MIN);
    }

    #[test]
    fn trig_products_near_a_boundary_are_flagged() {
        // 0.5 * 5 = 2.5 sits on a boundary; one ulp below rounds to 2.
        assert_eq!(round_trig_product(0.5, 5.0), (3, Some(2)));
        // 0.3 * 5 rounds to exactly 1.5 in f64, so it's on a boundary too; 0.31 * 5 isn't.
        assert_eq!(round_trig_product(0.3, 5.0), (2, Some(1)));
        assert_eq!(round_trig_product(0.31, 5.0), (2, None));
    }

    #[test]
    fn random_spread_stays_in_its_cell() {
        let spread = RandomSpread {
            spacing: 34,
            separation: 8,
            spread_type: SpreadType::Linear,
        };
        for (x, z) in [(0, 0), (-1, -1), (33, -34), (-35, 1000)] {
            let [cx, cz] = spread.potential_chunk(12345, 10_387_312, x, z);
            assert_eq!(cx.div_euclid(34), x.div_euclid(34));
            assert_eq!(cz.div_euclid(34), z.div_euclid(34));
            assert!(cx.rem_euclid(34) < 26 && cz.rem_euclid(34) < 26);
        }
        let triangular = RandomSpread {
            spread_type: SpreadType::Triangular,
            ..spread
        };
        let [cx, _] = triangular.potential_chunk(1, 2, -1, -1);
        assert_eq!(cx.div_euclid(34), -1);
    }

    #[test]
    fn potential_chunk_matches_a_hand_computation() {
        // Cell (-1, 0) for spacing 10, separation 4: offsets from setLargeFeatureWithSalt.
        let spread = RandomSpread {
            spacing: 10,
            separation: 4,
            spread_type: SpreadType::Linear,
        };
        let mut random = worldgen::large_feature_with_salt(99, -1, 0, 7);
        let (dx, dz) = (random.next_int_bounded(6), random.next_int_bounded(6));
        assert_eq!(spread.potential_chunk(99, 7, -3, 5), [-10 + dx, dz]);
    }

    #[test]
    fn default_reducer_swaps_its_arguments() {
        // probabilityReducer passes (salt, x, z) where setLargeFeatureWithSalt takes (x, z, salt).
        let f = worldgen::large_feature_with_salt(5, 11, 2, 3).next_float();
        assert!(FrequencyReduction::Default.should_generate(5, 11, 2, 3, f.next_up()));
        assert!(!FrequencyReduction::Default.should_generate(5, 11, 2, 3, f));
    }

    #[test]
    fn pillager_reducer_uses_regions_of_16() {
        // Chunks in the same 16 x 16 region give the same answer.
        let a = FrequencyReduction::LegacyType1.should_generate(42, 0, 16, 32, 0.2);
        let b = FrequencyReduction::LegacyType1.should_generate(42, 0, 31, 47, 0.2);
        assert_eq!(a, b);
        // At frequency 1, nextInt(1) is always 0.
        assert!(FrequencyReduction::LegacyType1.should_generate(42, 0, 3, 3, 1.0));
    }
}
