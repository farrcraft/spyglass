//! The generator's column (`NoiseBasedChunkGenerator.iterateNoiseColumn`) and the height
//! queries built on it: `getBaseHeight`, `getBaseColumn`, `getFirstFreeHeight` and
//! `getFirstOccupiedHeight`. See `docs/specs/terrain.md`. 26.2's columns are another family's
//! (ADR 0026), in [`crate::v1_21`], behind the same queries.

use spyglass_data::codec::LoadError;
use spyglass_density::Volume;

use crate::aquifer::{Aquifer, Block, SurfaceLevels};
use crate::settings::{BoundTerrain, Family, TerrainSettings};
use crate::v1_21::ChunkTerrain;

/// `Heightmap.Types`, the two the generator answers for before chunks exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Heightmap {
    /// `WORLD_SURFACE_WG`: any block but air, so fluids count.
    WorldSurfaceWg,
    /// `OCEAN_FLOOR_WG`: blocks in `#minecraft:blocks_motion_in_heightmap`. In a column, at
    /// most the default block is, and in 26.2 (`blocksMotion()`), ore veins' blocks.
    OceanFloorWg,
}

impl Heightmap {
    /// Both, in the order the oracle's files list them.
    pub const ALL: [Heightmap; 2] = [Heightmap::WorldSurfaceWg, Heightmap::OceanFloorWg];

    /// The heightmap's serialization key, e.g. `WORLD_SURFACE_WG`.
    pub fn key(self) -> &'static str {
        match self {
            Heightmap::WorldSurfaceWg => "WORLD_SURFACE_WG",
            Heightmap::OceanFloorWg => "OCEAN_FLOOR_WG",
        }
    }
}

/// `NoiseColumn`: a column's blocks from `min_y` up, and air outside them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockColumn {
    /// The lowest block's y.
    pub min_y: i32,
    /// The blocks, from `min_y` up.
    pub blocks: Vec<Block>,
}

impl BlockColumn {
    /// `getBlock(pos)`: the block at `y`, air outside the column.
    pub fn at(&self, y: i32) -> Block {
        usize::try_from(i64::from(y) - i64::from(self.min_y))
            .ok()
            .and_then(|i| self.blocks.get(i).copied())
            .unwrap_or(Block::Air)
    }
}

/// One seed's terrain, ready to answer height queries.
#[derive(Debug, Clone)]
pub struct Terrain {
    family: Bound,
    /// The column's range: the noise range clamped to the level (`clampToHeightAccessor`), in
    /// 26.2 the whole cells in it.
    min_y: i32,
    height: i32,
    /// The height of a column with nothing the heightmap counts: the level's bottom, or with
    /// `solid_only_heights`, the column's.
    empty_height: i32,
    default_block_blocks_motion: bool,
}

/// A seed's terrain functions, by family (ADR 0026).
#[derive(Debug, Clone)]
enum Bound {
    Volume {
        bound: Box<BoundTerrain>,
        /// The aquifers' surface levels, kept for the seed: each region's aquifer samples the
        /// levels around it, and nearby regions share most of them.
        levels: SurfaceLevels,
    },
    V1_21(Box<ChunkTerrain>),
}

impl Terrain {
    /// Binds `settings` for `seed`.
    pub fn new(settings: &TerrainSettings, seed: i64) -> Result<Self, LoadError> {
        let (family, min_y, height) = match &settings.family {
            Family::Volume { .. } => {
                let min_y = settings.min_y.max(settings.level_min_y);
                let top = (settings.min_y + settings.height)
                    .min(settings.level_min_y + settings.level_height);
                let bound = Bound::Volume {
                    bound: Box::new(settings.bind(seed)?),
                    levels: SurfaceLevels::default(),
                };
                (bound, min_y, top - min_y)
            }
            Family::V1_21(router) => {
                let level = [settings.level_min_y, settings.level_height];
                let chunk = ChunkTerrain::new(router, settings.fluid_picker(), level, seed)?;
                let (min_y, height) = ChunkTerrain::range(chunk.noise());
                (Bound::V1_21(Box::new(chunk)), min_y, height)
            }
        };
        Ok(Terrain {
            family,
            min_y,
            height,
            empty_height: if settings.solid_only_heights {
                min_y
            } else {
                settings.level_min_y
            },
            default_block_blocks_motion: settings.default_block_blocks_motion,
        })
    }

    /// The bound functions, for settings evaluated as volumes (26.3 on); `None` for 26.2's.
    pub fn bound(&self) -> Option<&BoundTerrain> {
        match &self.family {
            Bound::Volume { bound, .. } => Some(bound.as_ref()),
            Bound::V1_21(_) => None,
        }
    }

    /// Whether `heightmap` counts `block` (`Heightmap.Types.isOpaque`). Ore veins' blocks
    /// all block motion.
    pub fn counts(&self, heightmap: Heightmap, block: Block) -> bool {
        match heightmap {
            Heightmap::WorldSurfaceWg => block != Block::Air,
            Heightmap::OceanFloorWg => match block {
                Block::Default => self.default_block_blocks_motion,
                Block::Vein(_) => true,
                Block::Air | Block::Water | Block::Lava => false,
            },
        }
    }

    /// 26.2's columns: whether the veinifier must be asked to answer `heightmap`. Its blocks
    /// replace only the default block, and count wherever the default block does.
    fn veins_matter(&self) -> bool {
        !self.default_block_blocks_motion
    }

    /// The aquifer for a region, if the settings have one.
    fn aquifer<'a>(
        bound: &'a BoundTerrain,
        levels: &'a SurfaceLevels,
        region: &Volume,
    ) -> Option<Aquifer<'a>> {
        bound.aquifer.as_ref().map(|s| {
            Aquifer::with_levels(s, bound.aquifer_random, region, bound.fluid_picker, levels)
        })
    }

    /// 26.2: the height of every column of `columns`, each `(x, z, index into out)`.
    fn chunk_heights(
        &self,
        chunk: &ChunkTerrain,
        columns: impl IntoIterator<Item = (i32, i32, usize)>,
        heightmap: Heightmap,
        out: &mut [i32],
    ) {
        chunk.scan(columns, self.veins_matter(), |i, y, block| {
            let found = self.counts(heightmap, block);
            if found {
                out[i] = y + 1;
            }
            found
        });
    }

    /// `iterateNoiseColumn`: `final_density` over the whole column as one volume, then each
    /// block from the top down, through the aquifer. With `stop`, the first block it counts
    /// ends the scan, and the answer is one above it. With `blocks`, every block is recorded.
    fn scan(
        &self,
        x: i32,
        z: i32,
        stop: Option<Heightmap>,
        blocks: Option<&mut Vec<Block>>,
    ) -> Option<i32> {
        let Bound::Volume { bound, levels } = &self.family else {
            unreachable!("26.2 scans its own columns");
        };
        if self.height <= 0 {
            return None;
        }
        let column = Volume::blocks([1, self.height, 1], [x, self.min_y, z]);
        let density = bound.final_density.sample_volume(&column);
        let mut aquifer = Self::aquifer(bound, levels, &column);
        let blocks = blocks.map(|b| {
            *b = vec![Block::Air; self.height as usize];
            &mut b[..]
        });
        self.scan_with(x, z, self.min_y, &density, aquifer.as_mut(), stop, blocks)
    }

    /// The scan's second half, given densities from `min_y` up and an aquifer for a region
    /// holding them; `blocks`, if given, has a block for each density.
    #[allow(
        clippy::too_many_arguments,
        reason = "the column, its range and densities, the aquifer and what to record"
    )]
    fn scan_with(
        &self,
        x: i32,
        z: i32,
        min_y: i32,
        density: &[f32],
        mut aquifer: Option<&mut Aquifer<'_>>,
        stop: Option<Heightmap>,
        mut blocks: Option<&mut [Block]>,
    ) -> Option<i32> {
        let Bound::Volume { bound, .. } = &self.family else {
            unreachable!("26.2 scans its own columns");
        };
        let picker = bound.fluid_picker;
        for i in (0..density.len()).rev() {
            let y = min_y + i as i32;
            let d = f64::from(density[i]);
            let block = match aquifer.as_deref_mut() {
                Some(aquifer) => aquifer.substance(x, y, z, d),
                // Aquifer.createDisabled: solid where positive, else the fluid picker's.
                None if d > 0.0 => Block::Default,
                None => picker.status(y).at(y),
            };
            if let Some(b) = blocks.as_deref_mut() {
                b[i] = block;
            }
            if stop.is_some_and(|h| self.counts(h, block)) {
                return Some(y + 1);
            }
        }
        None
    }

    /// `getBaseHeight(x, z, heightmap)`: one above the highest block `heightmap` counts, or, if
    /// there's none, the level's minimum y (the column's with `solid_only_heights`).
    pub fn base_height(&self, x: i32, z: i32, heightmap: Heightmap) -> i32 {
        if let Bound::V1_21(chunk) = &self.family {
            let mut out = [self.empty_height];
            self.chunk_heights(chunk, [(x, z, 0)], heightmap, &mut out);
            return out[0];
        }
        self.scan(x, z, Some(heightmap), None)
            .unwrap_or(self.empty_height)
    }

    /// [`base_height`](Self::base_height) for every column of the box from `min`, `size` columns
    /// along x and z, z outer and x inner. Chunk by chunk, as chunk generation samples terrain:
    /// one density volume and one aquifer per chunk, which give each column the same answers
    /// as its own would (`docs/specs/terrain.md`) for much less work. In 26.2, cell by cell: one
    /// `NoiseChunk` and aquifer per cell, as each column's own would be.
    pub fn base_heights(&self, min: [i32; 2], size: [i32; 2], heightmap: Heightmap) -> Vec<i32> {
        let [w, d] = size.map(|s| s.max(0));
        let mut out = vec![self.empty_height; (w as usize) * (d as usize)];
        if self.height <= 0 || w == 0 || d == 0 {
            return out;
        }
        let (bound, levels) = match &self.family {
            Bound::Volume { bound, levels } => (bound, levels),
            Bound::V1_21(chunk) => {
                let columns = (0..d).flat_map(|j| {
                    (0..w).map(move |i| (min[0] + i, min[1] + j, (j * w + i) as usize))
                });
                self.chunk_heights(chunk, columns, heightmap, &mut out);
                return out;
            }
        };
        let [x_end, z_end] = [min[0] + w, min[1] + d];
        let mut z0 = min[1];
        while z0 < z_end {
            let z1 = (((z0 >> 4) + 1) << 4).min(z_end);
            let mut x0 = min[0];
            while x0 < x_end {
                let x1 = (((x0 >> 4) + 1) << 4).min(x_end);
                let tile = Volume::blocks([x1 - x0, self.height, z1 - z0], [x0, self.min_y, z0]);
                let density = bound.final_density.sample_volume(&tile);
                let mut aquifer = Self::aquifer(bound, levels, &tile);
                for z in z0..z1 {
                    for x in x0..x1 {
                        let first = tile.index(x - x0, 0, z - z0);
                        let column = &density[first..first + self.height as usize];
                        let height = self.scan_with(
                            x,
                            z,
                            self.min_y,
                            column,
                            aquifer.as_mut(),
                            Some(heightmap),
                            None,
                        );
                        let i = (z - min[1]) as usize * w as usize + (x - min[0]) as usize;
                        out[i] = height.unwrap_or(self.empty_height);
                    }
                }
                x0 = x1;
            }
            z0 = z1;
        }
        out
    }

    /// [`base_height`](Self::base_height) at every `(x, z)` with x in `xs` and z in `zs`, z outer
    /// and x inner. Both must be ascending. Chunk by chunk, as
    /// [`base_heights`](Self::base_heights) is, with each chunk's volume only as large as the box
    /// around its columns, so a sparse grid costs about one column per chunk.
    pub fn base_heights_on(&self, xs: &[i32], zs: &[i32], heightmap: Heightmap) -> Vec<i32> {
        debug_assert!(xs.is_sorted() && zs.is_sorted(), "ascending coordinates");
        let mut out = vec![self.empty_height; xs.len() * zs.len()];
        if self.height <= 0 {
            return out;
        }
        let (bound, levels) = match &self.family {
            Bound::Volume { bound, levels } => (bound, levels),
            Bound::V1_21(chunk) => {
                let columns = zs.iter().enumerate().flat_map(|(j, &z)| {
                    xs.iter()
                        .enumerate()
                        .map(move |(i, &x)| (x, z, j * xs.len() + i))
                });
                self.chunk_heights(chunk, columns, heightmap, &mut out);
                return out;
            }
        };
        // Runs of coordinates in one chunk, as index ranges.
        let runs = |vs: &[i32]| {
            let mut runs = Vec::new();
            let mut first = 0;
            for i in 1..=vs.len() {
                if i == vs.len() || vs[i] >> 4 != vs[first] >> 4 {
                    runs.push(first..i);
                    first = i;
                }
            }
            runs
        };
        let (x_runs, z_runs) = (runs(xs), runs(zs));
        for zr in &z_runs {
            let (z0, z1) = (zs[zr.start], zs[zr.end - 1]);
            for xr in &x_runs {
                let (x0, x1) = (xs[xr.start], xs[xr.end - 1]);
                let tile = Volume::blocks(
                    [x1 - x0 + 1, self.height, z1 - z0 + 1],
                    [x0, self.min_y, z0],
                );
                let density = bound.final_density.sample_volume(&tile);
                let mut aquifer = Self::aquifer(bound, levels, &tile);
                for j in zr.clone() {
                    for i in xr.clone() {
                        let (x, z) = (xs[i], zs[j]);
                        let first = tile.index(x - x0, 0, z - z0);
                        let column = &density[first..first + self.height as usize];
                        let height = self.scan_with(
                            x,
                            z,
                            self.min_y,
                            column,
                            aquifer.as_mut(),
                            Some(heightmap),
                            None,
                        );
                        out[j * xs.len() + i] = height.unwrap_or(self.empty_height);
                    }
                }
            }
        }
        out
    }

    /// `getFirstFreeHeight`: the same as [`base_height`](Self::base_height).
    pub fn first_free_height(&self, x: i32, z: i32, heightmap: Heightmap) -> i32 {
        self.base_height(x, z, heightmap)
    }

    /// `getFirstOccupiedHeight`: one below [`base_height`](Self::base_height).
    pub fn first_occupied_height(&self, x: i32, z: i32, heightmap: Heightmap) -> i32 {
        self.base_height(x, z, heightmap) - 1
    }

    /// `getBaseColumn(x, z)`: every block of the column. In 26.2, solid blocks can be ore
    /// veins' (`docs/specs/terrain.md#262-columns`).
    pub fn base_column(&self, x: i32, z: i32) -> BlockColumn {
        let mut blocks = Vec::new();
        match &self.family {
            Bound::Volume { .. } => {
                self.scan(x, z, None, Some(&mut blocks));
            }
            Bound::V1_21(chunk) => {
                blocks = vec![Block::Air; self.height.max(0) as usize];
                chunk.scan([(x, z, 0)], true, |_, y, block| {
                    blocks[(y - self.min_y) as usize] = block;
                    false
                });
            }
        }
        BlockColumn {
            min_y: self.min_y,
            blocks,
        }
    }

    /// The aquifer's answer at every block of column `(x, z)`, asked from the top down as the
    /// column asks it, with `density` at every block, or with the column's own final density:
    /// `computeSubstance` alone, so solid blocks are the default block, and 26.2's veins are
    /// left out. For checking the aquifer against the game.
    pub fn aquifer_column(&self, x: i32, z: i32, density: Option<f64>) -> BlockColumn {
        let blocks = match &self.family {
            Bound::V1_21(chunk) => chunk.aquifer_column(x, z, density),
            Bound::Volume { bound, levels } => {
                if self.height <= 0 {
                    Vec::new()
                } else {
                    let column = Volume::blocks([1, self.height, 1], [x, self.min_y, z]);
                    let densities: Vec<f64> = match density {
                        Some(d) => vec![d; self.height as usize],
                        None => {
                            let values = bound.final_density.sample_volume(&column);
                            values.into_iter().map(f64::from).collect()
                        }
                    };
                    let mut aquifer = Self::aquifer(bound, levels, &column);
                    let mut blocks = vec![Block::Air; self.height as usize];
                    for i in (0..blocks.len()).rev() {
                        let y = self.min_y + i as i32;
                        let d = densities[i];
                        blocks[i] = match aquifer.as_mut() {
                            Some(aquifer) => aquifer.substance(x, y, z, d),
                            None if d > 0.0 => Block::Default,
                            None => bound.fluid_picker.status(y).at(y),
                        };
                    }
                    blocks
                }
            }
        };
        BlockColumn {
            min_y: self.min_y,
            blocks,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use serde_json::{Value, json};
    use spyglass_density::Resources;

    use super::*;

    /// Noise settings with `final_density` positive below y 50 (a gradient from 1 to −1 over
    /// 0..100), sea level 63, no aquifers, and a level of `level`.
    fn terrain(noise: Value, level: Value, aquifers: Option<Value>) -> Terrain {
        let mut settings = json!({
            "noise": noise,
            "sea_level": 63,
            "default_block": "minecraft:stone",
            "default_fluid": {"Name": "minecraft:water", "Properties": {"level": "0"}},
            "noise_router": {"final_density": {"type": "gradient", "axis": "y",
                "from_coordinate": 0, "from_value": 1, "to_coordinate": 100, "to_value": -1}},
        });
        if let Some(a) = aquifers {
            settings["aquifers"] = a;
        }
        let map: HashMap<String, Value> = [
            ("worldgen/noise_settings/minecraft:t", settings),
            ("dimension_type/minecraft:t", level),
            (
                "tags/block/minecraft:blocks_motion_in_heightmap",
                json!({"values": ["#minecraft:solid"]}),
            ),
            (
                "tags/block/minecraft:solid",
                json!({"values": ["minecraft:stone"]}),
            ),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
        let settings = TerrainSettings::load(&map, "t", "t").unwrap();
        Terrain::new(&settings, 0).unwrap()
    }

    fn overworld() -> Terrain {
        terrain(
            json!({"min_y": -64, "height": 384}),
            json!({"min_y": -64, "height": 384}),
            None,
        )
    }

    #[test]
    fn a_flat_seabed() {
        let t = overworld();
        // Stone up to y 49 (density at 50 is exactly 0, not positive), water to the sea level.
        assert_eq!(t.base_height(5, 5, Heightmap::OceanFloorWg), 50);
        assert_eq!(t.base_height(5, 5, Heightmap::WorldSurfaceWg), 63);
        assert_eq!(t.first_occupied_height(5, 5, Heightmap::OceanFloorWg), 49);
        assert_eq!(t.first_free_height(5, 5, Heightmap::WorldSurfaceWg), 63);
        let column = t.base_column(5, 5);
        assert_eq!(column.at(49), Block::Default);
        assert_eq!(column.at(50), Block::Water);
        assert_eq!(column.at(62), Block::Water);
        assert_eq!(column.at(63), Block::Air);
        // The fluid picker's lava, under −54.
        assert_eq!(column.at(-64), Block::Default);
        assert_eq!(column.at(-1000), Block::Air);
        assert_eq!(column.blocks.len(), 384);
    }

    /// Resources read as a version with every 26.4-snapshot-5 switch on.
    struct Snapshot(HashMap<String, Value>);

    impl Resources for Snapshot {
        fn resource(
            &self,
            registry: &str,
            id: &str,
        ) -> Result<Value, spyglass_data::bundle::BundleError> {
            self.0.resource(registry, id)
        }

        fn behavior(&self) -> spyglass_data::versions::Behavior {
            spyglass_data::versions::Behavior::of(5125)
        }
    }

    #[test]
    fn without_a_default_block_solid_counts_and_empty_columns_end_at_their_bottom() {
        // 26.4's noise settings: no default_block, and no tags needed. The noise starts at y 0,
        // above the level's bottom, and final_density is negative everywhere but below y 10.
        let settings = |default_block: Option<&str>| {
            let mut json = json!({
                "noise": {"min_y": 0, "height": 128}, "sea_level": -100,
                "default_fluid": "minecraft:water",
                "noise_router": {"final_density": {"type": "gradient", "axis": "y",
                    "from_coordinate": 9, "from_value": 1, "to_coordinate": 10, "to_value": -1}},
            });
            if let Some(block) = default_block {
                json["default_block"] = block.into();
            }
            let map: HashMap<String, Value> = [
                ("worldgen/noise_settings/minecraft:t".to_string(), json),
                (
                    "dimension_type/minecraft:t".to_string(),
                    json!({"min_y": -64, "height": 384}),
                ),
            ]
            .into_iter()
            .collect();
            map
        };
        let loaded = TerrainSettings::load(&Snapshot(settings(None)), "t", "t").unwrap();
        assert_eq!(loaded.default_block, None);
        let t = Terrain::new(&loaded, 0).unwrap();
        // Solid below y 10 counts for OCEAN_FLOOR_WG without any tag.
        assert_eq!(t.base_height(0, 0, Heightmap::OceanFloorWg), 10);
        // A gradient that's never positive: nothing counts, so the column's bottom, y 0.
        let mut empty = settings(None);
        empty
            .get_mut("worldgen/noise_settings/minecraft:t")
            .unwrap()["noise_router"]["final_density"] = json!(-1.0);
        let t = Terrain::new(
            &TerrainSettings::load(&Snapshot(empty.clone()), "t", "t").unwrap(),
            0,
        )
        .unwrap();
        assert_eq!(t.base_height(0, 0, Heightmap::OceanFloorWg), 0);
        // 26.3 answers the level's bottom, and needs a default block.
        let mut with_block = empty;
        with_block
            .get_mut("worldgen/noise_settings/minecraft:t")
            .unwrap()["default_block"] = json!("minecraft:stone");
        with_block.insert(
            "tags/block/minecraft:blocks_motion_in_heightmap".into(),
            json!({"values": ["minecraft:stone"]}),
        );
        let t = Terrain::new(&TerrainSettings::load(&with_block, "t", "t").unwrap(), 0).unwrap();
        assert_eq!(t.base_height(0, 0, Heightmap::OceanFloorWg), -64);
        assert!(TerrainSettings::load(&settings(None), "t", "t").is_err());
    }

    #[test]
    fn regions_answer_as_columns_do() {
        let t = overworld();
        for heightmap in Heightmap::ALL {
            let region = t.base_heights([-20, 7], [37, 3], heightmap);
            for (i, h) in region.iter().enumerate() {
                let (x, z) = (-20 + i as i32 % 37, 7 + i as i32 / 37);
                assert_eq!(*h, t.base_height(x, z, heightmap));
            }
        }
        assert!(
            t.base_heights([0, 0], [0, 5], Heightmap::WorldSurfaceWg)
                .is_empty()
        );
    }

    #[test]
    fn grids_answer_as_columns_do() {
        let t = overworld();
        let (xs, zs) = ([-33, -17, -16, -1, 0, 5, 40], [-4, 3, 12, 16]);
        for heightmap in Heightmap::ALL {
            let grid = t.base_heights_on(&xs, &zs, heightmap);
            assert_eq!(grid.len(), xs.len() * zs.len());
            for (i, h) in grid.iter().enumerate() {
                let (x, z) = (xs[i % xs.len()], zs[i / xs.len()]);
                assert_eq!(*h, t.base_height(x, z, heightmap));
            }
        }
        assert!(
            t.base_heights_on(&[], &[1], Heightmap::WorldSurfaceWg)
                .is_empty()
        );
    }

    #[test]
    fn the_level_clamps_the_column() {
        // A level from 0 to 40: no water or air above it, so stone fills it to the top, and a
        // query with nothing to find answers the level's bottom.
        let t = terrain(
            json!({"min_y": -64, "height": 384}),
            json!({"min_y": 0, "height": 40}),
            None,
        );
        assert_eq!(t.base_height(0, 0, Heightmap::OceanFloorWg), 40);
        assert_eq!(t.base_column(0, 0).min_y, 0);
        assert_eq!(t.base_column(0, 0).blocks.len(), 40);
        let above = terrain(
            json!({"min_y": 0, "height": 16}),
            json!({"min_y": 64, "height": 64}),
            None,
        );
        assert_eq!(above.base_height(0, 0, Heightmap::WorldSurfaceWg), 64);
        assert!(above.base_column(0, 0).blocks.is_empty());
    }

    #[test]
    fn v1_21_settings_load_and_answer_through_noise_chunks() {
        use crate::aquifer::VeinBlock;
        use crate::v1_21::fixture;

        // final_density positive below y 50, as in a_flat_seabed; no aquifer, copper veins
        // everywhere (toggle 1, ridge below 0, no gap).
        let gradient = json!({"type": "minecraft:y_clamped_gradient", "from_y": 0, "to_y": 100,
            "from_value": 1.0, "to_value": -1.0});
        let entries = [
            ("final_density", gradient),
            ("vein_toggle", json!(1.0)),
            ("vein_ridged", json!(-1.0)),
            ("vein_gap", json!(1.0)),
        ];
        let resources = fixture::settings(&entries, false, true);
        let settings = TerrainSettings::load(&resources, "t", "t").unwrap();
        assert!(settings.router_v1_21().is_some());
        assert!(settings.final_density().is_none());
        assert!(settings.bind(0).is_err());
        assert_eq!(
            (settings.min_y, settings.height, settings.sea_level),
            (-64, 384, 63)
        );
        assert_eq!(settings.default_block.as_deref(), Some("minecraft:stone"));
        assert_eq!(settings.default_fluid, Block::Water);
        assert!(settings.default_block_blocks_motion);
        let t = Terrain::new(&settings, 3).unwrap();
        assert!(t.bound().is_none());
        // Veins don't move heights: they replace only solid blocks, which count anyway.
        assert_eq!(t.base_height(5, 5, Heightmap::OceanFloorWg), 50);
        assert_eq!(t.base_height(5, 5, Heightmap::WorldSurfaceWg), 63);
        let column = t.base_column(5, 5);
        assert_eq!((column.min_y, column.blocks.len()), (-64, 384));
        assert_eq!(column.at(-64), Block::Default);
        assert_eq!(column.at(-1), Block::Default);
        assert_eq!(column.at(50), Block::Water);
        assert_eq!(column.at(63), Block::Air);
        // Copper's range, 0 to 49 here: veins' blocks, or stone where the solidness draw fails.
        let veins: Vec<Block> = (0..50).map(|y| column.at(y)).collect();
        assert!(
            veins
                .iter()
                .all(|b| matches!(b, Block::Default | Block::Vein(_)))
        );
        assert!(veins.contains(&Block::Vein(VeinBlock::CopperOre)));
        // The aquifer's own answer has no veins.
        let aquifer = t.aquifer_column(5, 5, None);
        assert_eq!(aquifer.at(25), Block::Default);
        assert_eq!(aquifer.at(55), Block::Water);
        // Regions and grids answer as columns do.
        for heightmap in Heightmap::ALL {
            let region = t.base_heights([-9, 3], [11, 6], heightmap);
            for (i, h) in region.iter().enumerate() {
                let (x, z) = (-9 + i as i32 % 11, 3 + i as i32 / 11);
                assert_eq!(*h, t.base_height(x, z, heightmap));
            }
            let (xs, zs) = ([-5, 0, 3, 17], [-2, 9]);
            let grid = t.base_heights_on(&xs, &zs, heightmap);
            for (i, h) in grid.iter().enumerate() {
                assert_eq!(*h, t.base_height(xs[i % 4], zs[i / 4], heightmap));
            }
        }
        // Legacy randomness with ore veins isn't supported.
        let mut legacy = fixture::settings(&[], false, true);
        legacy
            .0
            .get_mut("worldgen/noise_settings/minecraft:t")
            .unwrap()["legacy_random_source"] = json!(true);
        assert!(TerrainSettings::load(&legacy, "t", "t").is_err());
    }
}
