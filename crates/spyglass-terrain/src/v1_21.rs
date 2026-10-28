//! 26.2's terrain, the 1.21 family's chunk path (ADR 0026): every column through a one-cell
//! `NoiseChunk` in `f64`, its aquifer, and the ore veinifier
//! (`NoiseBasedChunkGenerator.iterateNoiseColumn` and `NoiseChunk.getInterpolatedState`). See
//! `docs/specs/terrain.md#262-terrain`.

use std::collections::BTreeMap;
use std::sync::Arc;

use spyglass_data::codec::LoadError;
use spyglass_density::v1_21::{
    CornerStore, NoiseChunk, NoiseRouter, NoiseSettings, RandomState, RouterEntry,
};
use spyglass_rng::{
    PositionalRandomFactory, RandomSource, XoroshiroPositionalRandomFactory, XoroshiroRandomSource,
};

use crate::aquifer::{Aquifer, Block, FluidPicker, SurfaceLevels, VeinBlock, clamped_map};

/// `OreVeinifier.VeinType`: a vein's blocks and the y range it occurs in, both ends included.
#[derive(Debug, Clone, Copy)]
struct VeinType {
    ore: VeinBlock,
    raw_ore_block: VeinBlock,
    filler: VeinBlock,
    min_y: i32,
    max_y: i32,
}

/// `VeinType.COPPER`.
const COPPER: VeinType = VeinType {
    ore: VeinBlock::CopperOre,
    raw_ore_block: VeinBlock::RawCopperBlock,
    filler: VeinBlock::Granite,
    min_y: 0,
    max_y: 50,
};

/// `VeinType.IRON`.
const IRON: VeinType = VeinType {
    ore: VeinBlock::DeepslateIronOre,
    raw_ore_block: VeinBlock::RawIronBlock,
    filler: VeinBlock::Tuff,
    min_y: -60,
    max_y: -8,
};

/// `OreVeinifier.create(veinToggle, veinRidged, veinGap, oreRandom)`'s filler at block
/// `(x, y, z)`: a vein's block, or `None` (the game's `null`). `toggle` is `vein_toggle` there;
/// `ridged` and `gap` evaluate `vein_ridged` and `vein_gap`, each only if it's reached.
fn vein(
    random: &XoroshiroPositionalRandomFactory,
    [x, y, z]: [i32; 3],
    toggle: f64,
    ridged: impl FnOnce() -> f64,
    gap: impl FnOnce() -> f64,
) -> Option<VeinBlock> {
    let kind = if toggle > 0.0 { COPPER } else { IRON };
    let strength = toggle.abs();
    let above = kind.max_y.wrapping_sub(y);
    let below = y.wrapping_sub(kind.min_y);
    if below < 0 || above < 0 {
        return None;
    }
    let edge = above.min(below);
    // EDGE_ROUNDOFF_BEGIN, MAX_EDGE_ROUNDOFF.
    let roundoff = clamped_map(f64::from(edge), 0.0, 20.0, -0.2, 0.0);
    // VEININESS_THRESHOLD, (double) 0.4f.
    if strength + roundoff < f64::from(0.4f32) {
        return None;
    }
    let mut r = random.at(x, y, z);
    // VEIN_SOLIDNESS.
    if r.next_float() > 0.7 {
        return None;
    }
    if ridged() >= 0.0 {
        return None;
    }
    // MIN_RICHNESS, MAX_RICHNESS and MAX_RICHNESS_THRESHOLD, each a float widened.
    let richness = clamped_map(
        strength,
        f64::from(0.4f32),
        f64::from(0.6f32),
        f64::from(0.1f32),
        f64::from(0.3f32),
    );
    // SKIP_ORE_IF_GAP_NOISE_IS_BELOW, (double) -0.3f; CHANCE_OF_RAW_ORE_BLOCK.
    if f64::from(r.next_float()) < richness && gap() > f64::from(-0.3f32) {
        return Some(if r.next_float() < 0.02 {
            kind.raw_ore_block
        } else {
            kind.ore
        });
    }
    Some(kind.filler)
}

/// One seed's 26.2 terrain: the router bound as `RandomState`, and what each column's
/// `NoiseChunk` needs.
#[derive(Debug)]
pub(crate) struct ChunkTerrain {
    state: RandomState,
    /// The noise settings clamped to the level (`clampToHeightAccessor`).
    noise: NoiseSettings,
    level_min_y: i32,
    level_height: i32,
    picker: FluidPicker,
    /// `RandomState.aquiferRandom()`, if `aquifers_enabled`.
    aquifer_random: Option<XoroshiroPositionalRandomFactory>,
    /// `RandomState.oreRandom()`, if `ore_veins_enabled`.
    ore_random: Option<XoroshiroPositionalRandomFactory>,
    /// Surface levels for the seed, shared by every column's aquifer.
    levels: SurfaceLevels,
    /// Cell corners for the seed, shared by every column's `NoiseChunk` and by clones.
    corners: Arc<CornerStore>,
}

impl Clone for ChunkTerrain {
    fn clone(&self) -> Self {
        ChunkTerrain {
            state: self.state.clone(),
            levels: self.levels.clone(),
            corners: self.corners.clone(),
            ..*self
        }
    }
}

/// A column to scan: its position, and where its answer goes.
type Column = (i32, i32, usize);

impl ChunkTerrain {
    /// Binds `router` for `seed`, in a level from `level_min_y`, `level_height` blocks high.
    /// Settings with `legacy_random_source` and aquifers or ore veins are rejected when loaded.
    pub(crate) fn new(
        router: &NoiseRouter,
        picker: FluidPicker,
        [level_min_y, level_height]: [i32; 2],
        seed: i64,
    ) -> Result<Self, LoadError> {
        // RandomState: the seed's positional factory, then fromHashOf(id).forkPositional().
        let factory = |id: &str| {
            XoroshiroRandomSource::new(seed)
                .fork_positional()
                .from_hash_of(id)
                .fork_positional()
        };
        Ok(ChunkTerrain {
            state: router.bind(seed)?,
            noise: router.noise_settings().clamped(level_min_y, level_height),
            level_min_y,
            level_height,
            picker,
            aquifer_random: router
                .aquifers_enabled()
                .then(|| factory("minecraft:aquifer")),
            ore_random: router.ore_veins_enabled().then(|| factory("minecraft:ore")),
            levels: SurfaceLevels::default(),
            corners: Arc::default(),
        })
    }

    /// The column's range, whole cells: its lowest y and its height (`cellNoiseMinY ·
    /// cellHeight`, `cellCountY · cellHeight`), the height 0 if no cell fits.
    pub(crate) fn range(noise: NoiseSettings) -> (i32, i32) {
        let ch = noise.cell_height();
        let min_y = noise.min_y.div_euclid(ch).wrapping_mul(ch);
        let height = noise.height.div_euclid(ch).max(0).wrapping_mul(ch);
        (min_y, height)
    }

    /// The noise settings clamped to the level.
    pub(crate) fn noise(&self) -> NoiseSettings {
        self.noise
    }

    /// `iterateNoiseColumn` for every column of `columns`, which share one cell: the cell's
    /// `NoiseChunk` and its aquifer, then each column from the top down. For each column,
    /// `visit(index, y, block)` is called with each block until it returns true, which ends
    /// that column. Without `veins`, the veinifier isn't asked: callers that only test whether a
    /// block counts for a heightmap pass false when a vein block counts as the default block
    /// does.
    fn scan_cell(
        &self,
        columns: &[Column],
        veins: bool,
        mut visit: impl FnMut(usize, i32, Block) -> bool,
    ) {
        let Some(&(x, z, _)) = columns.first() else {
            return;
        };
        let (min_y, height) = Self::range(self.noise);
        if height <= 0 {
            return;
        }
        let chunk = NoiseChunk::for_column(&self.state, x, z, self.level_min_y, self.level_height)
            .sharing_corners(self.corners.clone());
        // The aquifer of the chunk holding the cell's corner (SectionPos.blockToSectionCoord).
        let width = self.noise.cell_width();
        let corner = [x, z].map(|v| v.div_euclid(width).wrapping_mul(width) >> 4);
        let mut aquifer = self.aquifer_random.map(|random| {
            Aquifer::for_chunk(
                &chunk,
                random,
                corner,
                [self.noise.min_y, self.noise.height],
                self.picker,
                Some(&self.levels),
            )
        });
        let ore = if veins {
            self.ore_random.as_ref()
        } else {
            None
        };
        for &(x, z, index) in columns {
            for y in (min_y..min_y + height).rev() {
                let density = chunk.interpolated_density(x, y, z);
                let mut block = match aquifer.as_mut() {
                    Some(aquifer) => aquifer.substance(x, y, z, density),
                    // Aquifer.createDisabled.
                    None if density > 0.0 => Block::Default,
                    None => self.picker.status(y).at(y),
                };
                if let (Block::Default, Some(random)) = (block, ore) {
                    let toggle = chunk.compute_positioned(RouterEntry::VeinToggle, x, y, z);
                    let ridged = || chunk.compute_positioned(RouterEntry::VeinRidged, x, y, z);
                    let gap = || chunk.compute_positioned(RouterEntry::VeinGap, x, y, z);
                    if let Some(v) = vein(random, [x, y, z], toggle, ridged, gap) {
                        block = Block::Vein(v);
                    }
                }
                if visit(index, y, block) {
                    break;
                }
            }
        }
    }

    /// [`scan_cell`](Self::scan_cell) for every column of `columns`, grouped by cell, cell by
    /// cell. The answers are each column's own: columns of one cell share its `NoiseChunk`, as
    /// `iterateNoiseColumn` would make the same one for each, and every cell shares the seed's
    /// corners ([`CornerStore`]).
    pub(crate) fn scan(
        &self,
        columns: impl IntoIterator<Item = Column>,
        veins: bool,
        mut visit: impl FnMut(usize, i32, Block) -> bool,
    ) {
        let width = self.noise.cell_width();
        let mut cells: BTreeMap<(i32, i32), Vec<Column>> = BTreeMap::new();
        for column in columns {
            let key = (column.1.div_euclid(width), column.0.div_euclid(width));
            cells.entry(key).or_default().push(column);
        }
        for columns in cells.values() {
            self.scan_cell(columns, veins, &mut visit);
        }
    }

    /// The aquifer's answer for every block of column `(x, z)` from the top down, as
    /// `iterateNoiseColumn` asks it, with `density` at every block or the column's own
    /// interpolated final density: `computeSubstance` alone, with no veins.
    pub(crate) fn aquifer_column(&self, x: i32, z: i32, density: Option<f64>) -> Vec<Block> {
        let (min_y, height) = Self::range(self.noise);
        let mut out = vec![Block::Air; height.max(0) as usize];
        if height <= 0 {
            return out;
        }
        let chunk = NoiseChunk::for_column(&self.state, x, z, self.level_min_y, self.level_height);
        let width = self.noise.cell_width();
        let corner = [x, z].map(|v| v.div_euclid(width).wrapping_mul(width) >> 4);
        let mut aquifer = self.aquifer_random.map(|random| {
            Aquifer::for_chunk(
                &chunk,
                random,
                corner,
                [self.noise.min_y, self.noise.height],
                self.picker,
                None,
            )
        });
        for i in (0..height as usize).rev() {
            let y = min_y + i as i32;
            let d = density.unwrap_or_else(|| chunk.interpolated_density(x, y, z));
            out[i] = match aquifer.as_mut() {
                Some(aquifer) => aquifer.substance(x, y, z, d),
                None if d > 0.0 => Block::Default,
                None => self.picker.status(y).at(y),
            };
        }
        out
    }
}

/// Hand-built 26.2 data for tests: noise settings `minecraft:t` (the Overworld's range, cells
/// 4 × 8 × 4, sea level 63, stone and water) with every router entry 0 but those in `entries`,
/// and a level the same as the noise range. Read as 26.2 (data version 4903).
#[cfg(test)]
pub(crate) mod fixture {
    use std::collections::HashMap;

    use serde_json::{Value, json};
    use spyglass_data::bundle::BundleError;
    use spyglass_data::versions::Behavior;
    use spyglass_density::Resources;

    /// 26.2's resources.
    pub(crate) struct V26_2(pub(crate) HashMap<String, Value>);

    impl Resources for V26_2 {
        fn resource(&self, registry: &str, id: &str) -> Result<Value, BundleError> {
            self.0.resource(registry, id)
        }

        fn behavior(&self) -> Behavior {
            Behavior::of(4903)
        }
    }

    pub(crate) fn settings(entries: &[(&str, Value)], aquifers: bool, veins: bool) -> V26_2 {
        let mut router = serde_json::Map::new();
        for name in [
            "barrier",
            "fluid_level_floodedness",
            "fluid_level_spread",
            "lava",
            "temperature",
            "vegetation",
            "continents",
            "erosion",
            "depth",
            "ridges",
            "preliminary_surface_level",
            "final_density",
            "vein_toggle",
            "vein_ridged",
            "vein_gap",
        ] {
            router.insert(name.into(), json!(0.0));
        }
        for (name, value) in entries {
            router.insert((*name).into(), value.clone());
        }
        let settings = json!({
            "noise": {"min_y": -64, "height": 384, "size_horizontal": 1, "size_vertical": 2},
            "sea_level": 63,
            "default_block": {"Name": "minecraft:stone"},
            "default_fluid": {"Name": "minecraft:water", "Properties": {"level": "0"}},
            "noise_router": router,
            "aquifers_enabled": aquifers,
            "ore_veins_enabled": veins,
            "legacy_random_source": false,
            "disable_mob_generation": false,
        });
        V26_2(
            [
                ("worldgen/noise_settings/minecraft:t", settings),
                (
                    "dimension_type/minecraft:t",
                    json!({"min_y": -64, "height": 384}),
                ),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn random() -> XoroshiroPositionalRandomFactory {
        XoroshiroRandomSource::new(7)
            .fork_positional()
            .from_hash_of("minecraft:ore")
            .fork_positional()
    }

    /// The veinifier's draws at a block, to work cases out by hand.
    fn draws(pos: [i32; 3]) -> [f32; 3] {
        let mut r = random().at(pos[0], pos[1], pos[2]);
        [r.next_float(), r.next_float(), r.next_float()]
    }

    #[test]
    fn veins_keep_to_their_ranges() {
        let r = random();
        let never = || -> f64 { panic!("not reached") };
        // Copper (toggle positive) is 0 to 50, iron −60 to −8, both ends included.
        for y in [-1, 51, 100] {
            assert_eq!(
                vein(&r, [0, y, 0], 1.0, never, never),
                None,
                "copper at {y}"
            );
        }
        for y in [-61, -7, 10] {
            assert_eq!(vein(&r, [0, y, 0], -1.0, never, never), None, "iron at {y}");
        }
        // At 0 and −8 the edge roundoff is −0.2, so a strength of 0.6 is just short.
        assert_eq!(vein(&r, [0, 0, 0], 0.5999, never, never), None);
        assert_eq!(vein(&r, [0, -8, 0], -0.5999, never, never), None);
        // Twenty blocks in, there's no roundoff: 0.4f is the threshold.
        assert_eq!(vein(&r, [0, 20, 0], 0.4, never, never), None);
        // A NaN toggle is iron, and passes the threshold test, as dcmpg does: it reaches the
        // ridge wherever the solidness draw passes.
        let x = (0..100).find(|&x| draws([x, -30, 0])[0] <= 0.7).unwrap();
        let mut reached = false;
        let ridged = || {
            reached = true;
            0.0
        };
        assert_eq!(vein(&r, [x, -30, 0], f64::NAN, ridged, never), None);
        assert!(reached);
        assert_eq!(vein(&r, [x, 20, 0], f64::NAN, never, never), None);
    }

    #[test]
    fn a_vein_block_by_hand() {
        let r = random();
        // Find blocks whose first draw passes the solidness test, and work out the rest.
        let mut seen = [false; 3];
        for x in 0..2000 {
            let pos = [x, 25, 3];
            let [solid, rich, raw] = draws(pos);
            let got = vein(&r, pos, 0.5, || -0.1, || 0.0);
            if solid > 0.7 {
                assert_eq!(got, None, "{pos:?}");
                continue;
            }
            // Strength 0.5: richness halfway from 0.1f to 0.3f.
            let richness = clamped_map(
                0.5,
                f64::from(0.4f32),
                f64::from(0.6f32),
                f64::from(0.1f32),
                f64::from(0.3f32),
            );
            let want = if f64::from(rich) < richness {
                if raw < 0.02 {
                    seen[0] = true;
                    VeinBlock::RawCopperBlock
                } else {
                    seen[1] = true;
                    VeinBlock::CopperOre
                }
            } else {
                seen[2] = true;
                VeinBlock::Granite
            };
            assert_eq!(got, Some(want), "{pos:?}");
            // A ridge at or above zero leaves the block alone; a gap at −0.3f gives filler.
            assert_eq!(vein(&r, pos, 0.5, || 0.0, || 0.0), None);
            let gap = vein(&r, pos, 0.5, || -0.1, || f64::from(-0.3f32));
            assert_eq!(gap, Some(VeinBlock::Granite));
        }
        assert_eq!(seen, [true; 3], "every outcome is exercised");
        // Iron's blocks.
        let iron: Vec<_> = (0..200)
            .filter_map(|x| vein(&r, [x, -30, 0], -0.9, || -1.0, || 1.0))
            .collect();
        assert!(iron.contains(&VeinBlock::DeepslateIronOre));
        assert!(iron.contains(&VeinBlock::Tuff));
    }

    #[test]
    fn the_column_is_whole_cells() {
        let noise = |min_y, height| NoiseSettings {
            min_y,
            height,
            size_horizontal: 1,
            size_vertical: 2,
        };
        assert_eq!(ChunkTerrain::range(noise(-64, 384)), (-64, 384));
        // A level that cuts a cell: only whole cells, from the cell holding min y.
        assert_eq!(ChunkTerrain::range(noise(-60, 20)), (-64, 16));
        assert_eq!(ChunkTerrain::range(noise(0, 7)), (0, 0));
    }
}
