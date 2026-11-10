//! `WorldGenRegion` during decoration: a chunk and its 8 neighbours, the blocks features read and
//! write, and their heightmaps (`docs/specs/features.md#the-region`).

use spyglass_blocks::{Chunk, State};

use crate::blocks::{Blocks, Heightmap};

/// The heightmaps writes keep up to date (`FINAL_HEIGHTMAPS`), by index.
const LIVE: [Heightmap; 4] = [
    Heightmap::WorldSurface,
    Heightmap::OceanFloor,
    Heightmap::MotionBlocking,
    Heightmap::MotionBlockingNoLeaves,
];

/// Why a feature's answer isn't certain (`docs/specs/features.md#certainty`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Doubt {
    /// It read a block that depends on a tied biome.
    Tie,
    /// It read outside the region, which vanilla features don't.
    OutsideRegion,
    /// A structure's pieces, placed earlier in the step or in an earlier one, could have
    /// written what it read: this structure, by index in its `StructureSets`.
    Structure(usize),
    /// A neighbour decorated first could have written what it read: this placed feature of a
    /// neighbour's (ADR 0055).
    Neighbours(crate::order::PlacedRef),
    /// A feature spyglass doesn't place, this placed feature, ran earlier in the chunk and could
    /// have changed what it read.
    OtherFeature(crate::order::PlacedRef),
    /// A template placed beside a block whose shape update could change it (a fence, a plant).
    ShapeUpdate,
    /// A template placed a block that can hold water where water could flow into it.
    Waterlogging,
}

/// A container a feature placed with a loot table (`docs/specs/loot.md#containers`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Container {
    /// Where.
    pub pos: [i32; 3],
    /// The loot table's id, e.g. `minecraft:chests/simple_dungeon`.
    pub table: String,
    /// The loot seed, the feature random's `nextLong()` when it set the table.
    pub seed: i64,
}

/// One chunk of the region, with its heightmaps as first free y per column.
#[derive(Debug, Clone)]
struct Column {
    chunk: Chunk,
    /// [`LIVE`]'s, then `WORLD_SURFACE_WG` and `OCEAN_FLOOR_WG`, frozen, then [`LIVE`]'s as
    /// terrain left them.
    maps: [[i32; 256]; 10],
}

/// The 3 × 3 chunks around one being decorated.
#[derive(Debug, Clone)]
pub struct Region<'b> {
    blocks: &'b Blocks,
    center: [i32; 2],
    /// By `(dz + 1) * 3 + (dx + 1)`.
    columns: Vec<Column>,
    min_y: i32,
    height: i32,
    /// The first doubt since the caller last cleared it.
    doubt: Option<Doubt>,
    /// The containers placed since the caller last took them.
    containers: Vec<Container>,
}

impl<'b> Region<'b> {
    /// The region around chunk `center` from its 9 chunks after carvers, by `(dz + 1) * 3 + (dx
    /// + 1)`. Their states are `blocks`' palette's.
    pub fn new(blocks: &'b Blocks, center: [i32; 2], chunks: Vec<Chunk>) -> Self {
        assert_eq!(chunks.len(), 9, "a region has 9 chunks");
        let (min_y, height) = (chunks[4].min_y, chunks[4].height);
        let columns = chunks
            .into_iter()
            .map(|chunk| {
                let mut maps = [[min_y; 256]; 10];
                // [`LIVE`]'s, then the `_WG` ones: each map's heightmap bit.
                let order = [
                    Heightmap::WorldSurface,
                    Heightmap::OceanFloor,
                    Heightmap::MotionBlocking,
                    Heightmap::MotionBlockingNoLeaves,
                    Heightmap::WorldSurfaceWg,
                    Heightmap::OceanFloorWg,
                ]
                .map(|h| 1u8 << h as u8);
                #[allow(
                    clippy::needless_range_loop,
                    reason = "the column index addresses ten maps"
                )]
                for i in 0..256 {
                    let (x, z) = (i as i32 % 16, i as i32 / 16);
                    // Down the column once, each heightmap taking the first block it counts.
                    let mut remaining = order.iter().fold(0u8, |a, b| a | b);
                    for y in (min_y..min_y + height).rev() {
                        let bits = blocks.heightmap_bits(chunk.get([x, y, z])) & remaining;
                        if bits != 0 {
                            for (map, bit) in order.iter().enumerate() {
                                if bits & bit != 0 {
                                    maps[map][i] = y + 1;
                                }
                            }
                            remaining &= !bits;
                            if remaining == 0 {
                                break;
                            }
                        }
                    }
                    for map in 0..4 {
                        maps[6 + map][i] = maps[map][i];
                    }
                }
                Column { chunk, maps }
            })
            .collect();
        Region {
            blocks,
            center,
            columns,
            min_y,
            height,
            doubt: None,
            containers: Vec::new(),
        }
    }

    /// The blocks the region's states come from.
    pub fn blocks(&self) -> &'b Blocks {
        self.blocks
    }

    /// The level's lowest y.
    pub fn min_y(&self) -> i32 {
        self.min_y
    }

    /// The level's highest y.
    pub fn max_y(&self) -> i32 {
        self.min_y + self.height - 1
    }

    /// `isOutsideBuildHeight`.
    pub fn outside_build_height(&self, y: i32) -> bool {
        y < self.min_y || y > self.max_y()
    }

    /// The chunk being decorated.
    pub fn center(&self) -> [i32; 2] {
        self.center
    }

    fn column(&self, x: i32, z: i32) -> Option<usize> {
        let dx = (x >> 4).wrapping_sub(self.center[0]);
        let dz = (z >> 4).wrapping_sub(self.center[1]);
        ((-1..=1).contains(&dx) && (-1..=1).contains(&dz)).then(|| ((dz + 1) * 3 + dx + 1) as usize)
    }

    /// `getBlockState`: air outside the level. Outside the region, which vanilla features never
    /// read, air and doubt.
    pub fn get(&mut self, pos @ [x, _, z]: [i32; 3]) -> State {
        let Some(i) = self.column(x, z) else {
            self.doubt(Doubt::OutsideRegion);
            return spyglass_blocks::state::AIR;
        };
        if !self.columns[i].chunk.is_certain(pos) {
            self.doubt(Doubt::Tie);
        }
        let chunk = &self.columns[i].chunk;
        chunk.get(pos)
    }

    /// `setBlock`: dropped outside the write zone (the 3 × 3) and the level. Updates the live
    /// heightmaps; the `_WG` ones stay as terrain left them.
    pub fn set(&mut self, pos @ [x, y, z]: [i32; 3], state: State) -> bool {
        let Some(i) = self.column(x, z) else {
            return false;
        };
        if self.outside_build_height(y) {
            return true;
        }
        let blocks = self.blocks;
        let doubt = self.doubt.is_some();
        let column = &mut self.columns[i];
        column.chunk.put(pos, state);
        if doubt {
            column.chunk.mark_uncertain(pos);
        }
        let cell = ((z & 15) * 16 + (x & 15)) as usize;
        for (map, &heightmap) in LIVE.iter().enumerate() {
            // Heightmap.update: a block that counts, at or above the first free y, raises it; one
            // that doesn't, replacing the top block, lowers it to the next block down that
            // counts. Writes well below the top change nothing.
            let first = column.maps[map][cell];
            if y <= first - 2 {
                continue;
            }
            if blocks.counts_for(state, heightmap) {
                if y >= first {
                    column.maps[map][cell] = y + 1;
                }
            } else if first - 1 == y {
                column.maps[map][cell] = (self.min_y..y)
                    .rev()
                    .find(|&k| blocks.counts_for(column.chunk.get([x, k, z]), heightmap))
                    .map_or(self.min_y, |k| k + 1);
            }
        }
        true
    }

    /// `getHeight(type, x, z)`: the first free y, one above the top block the heightmap counts,
    /// from the chunk holding the column.
    pub fn height(&mut self, heightmap: Heightmap, x: i32, z: i32) -> i32 {
        let Some(i) = self.column(x, z) else {
            self.doubt(Doubt::OutsideRegion);
            return self.min_y;
        };
        let map = match heightmap {
            Heightmap::WorldSurface => 0,
            Heightmap::OceanFloor => 1,
            Heightmap::MotionBlocking => 2,
            Heightmap::MotionBlockingNoLeaves => 3,
            Heightmap::WorldSurfaceWg => 4,
            Heightmap::OceanFloorWg => 5,
        };
        self.columns[i].maps[map][((z & 15) * 16 + (x & 15)) as usize]
    }

    /// The heightmap's first free y at `(x, z)` as terrain left it, before any feature (the
    /// `_WG` ones never change): `None` outside the region. Without doubt.
    pub fn terrain_height(&self, heightmap: Heightmap, x: i32, z: i32) -> Option<i32> {
        let i = self.column(x, z)?;
        let cell = ((z & 15) * 16 + (x & 15)) as usize;
        let map = match heightmap {
            Heightmap::WorldSurface => 6,
            Heightmap::OceanFloor => 7,
            Heightmap::MotionBlocking => 8,
            Heightmap::MotionBlockingNoLeaves => 9,
            Heightmap::WorldSurfaceWg => 4,
            Heightmap::OceanFloorWg => 5,
        };
        Some(self.columns[i].maps[map][cell])
    }

    /// The first doubt since the last [`Region::take_doubt`], which clears it.
    pub fn take_doubt(&mut self) -> Option<Doubt> {
        self.doubt.take()
    }

    /// Puts what follows in doubt, so the blocks it writes are too. The first reason is kept.
    pub fn doubt(&mut self, why: Doubt) {
        self.doubt.get_or_insert(why);
    }

    /// Records a container placed at `pos` with loot table `table` and loot seed `seed`.
    pub fn container(&mut self, pos: [i32; 3], table: &str, seed: i64) {
        self.containers.push(Container {
            pos,
            table: table.to_string(),
            seed,
        });
    }

    /// The containers placed since the last [`Region::take_containers`], in the order they were.
    pub fn take_containers(&mut self) -> Vec<Container> {
        std::mem::take(&mut self.containers)
    }

    /// The chunk being decorated, as it is now.
    pub fn center_chunk(&self) -> &Chunk {
        &self.columns[4].chunk
    }
}
