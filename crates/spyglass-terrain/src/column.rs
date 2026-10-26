//! The generator's column (`NoiseBasedChunkGenerator.iterateNoiseColumn`) and the height
//! queries built on it: `getBaseHeight`, `getBaseColumn`, `getFirstFreeHeight` and
//! `getFirstOccupiedHeight`. See `docs/specs/terrain.md`.

use spyglass_data::codec::LoadError;
use spyglass_density::Volume;

use crate::aquifer::{Aquifer, Block, SurfaceLevels};
use crate::settings::{BoundTerrain, TerrainSettings};

/// `Heightmap.Types`, the two the generator answers for before chunks exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Heightmap {
    /// `WORLD_SURFACE_WG`: any block but air, so fluids count.
    WorldSurfaceWg,
    /// `OCEAN_FLOOR_WG`: blocks in `#minecraft:blocks_motion_in_heightmap`. In a column, at
    /// most the default block is.
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
    bound: BoundTerrain,
    /// The column's range: the noise range clamped to the level (`clampToHeightAccessor`).
    min_y: i32,
    height: i32,
    level_min_y: i32,
    default_block_blocks_motion: bool,
    /// The aquifers' surface levels, kept for the seed: each region's aquifer samples the
    /// levels around it, and nearby regions share most of them.
    levels: SurfaceLevels,
}

impl Terrain {
    /// Binds `settings` for `seed`.
    pub fn new(settings: &TerrainSettings, seed: i64) -> Result<Self, LoadError> {
        let min_y = settings.min_y.max(settings.level_min_y);
        let top =
            (settings.min_y + settings.height).min(settings.level_min_y + settings.level_height);
        Ok(Terrain {
            bound: settings.bind(seed)?,
            min_y,
            height: top - min_y,
            level_min_y: settings.level_min_y,
            default_block_blocks_motion: settings.default_block_blocks_motion,
            levels: SurfaceLevels::default(),
        })
    }

    /// The bound functions.
    pub fn bound(&self) -> &BoundTerrain {
        &self.bound
    }

    /// Whether `heightmap` counts `block` (`Heightmap.Types.isOpaque`).
    pub fn counts(&self, heightmap: Heightmap, block: Block) -> bool {
        match heightmap {
            Heightmap::WorldSurfaceWg => block != Block::Air,
            Heightmap::OceanFloorWg => block == Block::Default && self.default_block_blocks_motion,
        }
    }

    /// The aquifer for a region, if the settings have one.
    fn aquifer(&self, region: &Volume) -> Option<Aquifer<'_>> {
        let b = &self.bound;
        b.aquifer.as_ref().map(|s| {
            Aquifer::with_levels(s, b.aquifer_random, region, b.fluid_picker, &self.levels)
        })
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
        if self.height <= 0 {
            return None;
        }
        let column = Volume::blocks([1, self.height, 1], [x, self.min_y, z]);
        let density = self.bound.final_density.sample_volume(&column);
        let mut aquifer = self.aquifer(&column);
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
        let picker = self.bound.fluid_picker;
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

    /// `getBaseHeight(x, z, heightmap)`: one above the highest block `heightmap` counts, or the
    /// level's minimum y if there's none.
    pub fn base_height(&self, x: i32, z: i32, heightmap: Heightmap) -> i32 {
        self.scan(x, z, Some(heightmap), None)
            .unwrap_or(self.level_min_y)
    }

    /// [`base_height`](Self::base_height) for every column of the box from `min`, `size` columns
    /// along x and z, z outer and x inner. Chunk by chunk, as chunk generation samples terrain:
    /// one density volume and one aquifer per chunk, which give each column the same answers
    /// as its own would (`docs/specs/terrain.md`) for much less work.
    pub fn base_heights(&self, min: [i32; 2], size: [i32; 2], heightmap: Heightmap) -> Vec<i32> {
        let [w, d] = size.map(|s| s.max(0));
        let mut out = vec![self.level_min_y; (w as usize) * (d as usize)];
        if self.height <= 0 || w == 0 || d == 0 {
            return out;
        }
        let [x_end, z_end] = [min[0] + w, min[1] + d];
        let mut z0 = min[1];
        while z0 < z_end {
            let z1 = (((z0 >> 4) + 1) << 4).min(z_end);
            let mut x0 = min[0];
            while x0 < x_end {
                let x1 = (((x0 >> 4) + 1) << 4).min(x_end);
                let tile = Volume::blocks([x1 - x0, self.height, z1 - z0], [x0, self.min_y, z0]);
                let density = self.bound.final_density.sample_volume(&tile);
                let mut aquifer = self.aquifer(&tile);
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
                        out[i] = height.unwrap_or(self.level_min_y);
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
        let mut out = vec![self.level_min_y; xs.len() * zs.len()];
        if self.height <= 0 {
            return out;
        }
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
                let density = self.bound.final_density.sample_volume(&tile);
                let mut aquifer = self.aquifer(&tile);
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
                        out[j * xs.len() + i] = height.unwrap_or(self.level_min_y);
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

    /// `getBaseColumn(x, z)`: every block of the column.
    pub fn base_column(&self, x: i32, z: i32) -> BlockColumn {
        let mut blocks = Vec::new();
        self.scan(x, z, None, Some(&mut blocks));
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
}
