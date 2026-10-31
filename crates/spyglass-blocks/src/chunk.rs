//! A chunk's blocks, and the `WORLD_SURFACE_WG` heightmap generation keeps up to date as blocks
//! change (`docs/specs/material.md#the-chunk`).

use crate::state::{AIR, Palette, State};

/// A chunk's blocks over the level's height, with its `WORLD_SURFACE_WG` heightmap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    /// Chunk x and z.
    pub pos: [i32; 2],
    /// The level's lowest y.
    pub min_y: i32,
    /// The level's height.
    pub height: i32,
    /// By column (x fastest within z), then y from the bottom.
    blocks: Vec<State>,
    /// `Heightmap.getFirstAvailable` per column: one above the top block that isn't air, or
    /// `min_y` if there's none.
    first_available: [i32; 256],
    /// Blocks that depend on a tied biome, a bit per position in `blocks`' order.
    uncertain: Vec<u64>,
}

impl Chunk {
    /// An empty chunk of air.
    pub fn new(pos: [i32; 2], min_y: i32, height: i32) -> Self {
        Chunk {
            pos,
            min_y,
            height,
            blocks: vec![AIR; 256 * height.max(0) as usize],
            first_available: [min_y; 256],
            uncertain: vec![0; (256 * height.max(0) as usize).div_ceil(64)],
        }
    }

    /// Whether the block at `(x, y, z)` is the game's whatever lookup history it has: it
    /// doesn't depend on a tied biome (`docs/specs/material.md#ties`). Outside the level, true.
    pub fn is_certain(&self, pos: [i32; 3]) -> bool {
        self.index(pos)
            .is_none_or(|i| self.uncertain[i / 64] & (1 << (i % 64)) == 0)
    }

    /// Marks the block at `(x, y, z)` as depending on a tied biome.
    pub fn mark_uncertain(&mut self, pos: [i32; 3]) {
        if let Some(i) = self.index(pos) {
            self.uncertain[i / 64] |= 1 << (i % 64);
        }
    }

    fn index(&self, [x, y, z]: [i32; 3]) -> Option<usize> {
        let dy = y.checked_sub(self.min_y)?;
        if !(0..self.height).contains(&dy) {
            return None;
        }
        Some((((z & 15) * 16 + (x & 15)) * self.height + dy) as usize)
    }

    /// The block at `(x, y, z)` (x and z in or relative to this chunk; only their low four bits
    /// count): air outside the level, as `ProtoChunk.getBlockState` gives void air there.
    pub fn get(&self, pos: [i32; 3]) -> State {
        self.index(pos).map_or(AIR, |i| self.blocks[i])
    }

    /// Sets a block without touching the heightmap: for filling, and for decoration, which
    /// leaves `WORLD_SURFACE_WG` as terrain left it.
    pub fn put(&mut self, pos: [i32; 3], state: State) {
        if let Some(i) = self.index(pos) {
            self.blocks[i] = state;
        }
    }

    /// `Heightmap.primeHeightmaps`: each column's first available y from its blocks.
    pub(crate) fn prime_heightmap(&mut self, palette: &Palette) {
        for column in 0..256 {
            let (x, z) = (column as i32 % 16, column as i32 / 16);
            self.first_available[column] = (self.min_y..self.min_y + self.height)
                .rev()
                .find(|&y| !palette.is_air(self.get([x, y, z])))
                .map_or(self.min_y, |y| y + 1);
        }
    }

    /// `ChunkAccess.getHeight(WORLD_SURFACE_WG, x, z)`: the top block that isn't air, or one
    /// below the level if there's none.
    pub fn height_at(&self, x: i32, z: i32) -> i32 {
        self.first_available[((z & 15) * 16 + (x & 15)) as usize] - 1
    }

    /// `ProtoChunk.setBlockState` within the level, with `Heightmap.update` for
    /// `WORLD_SURFACE_WG`. Outside the level, nothing.
    pub fn set(&mut self, palette: &Palette, pos @ [x, y, z]: [i32; 3], state: State) {
        let Some(i) = self.index(pos) else {
            return;
        };
        self.blocks[i] = state;
        let column = ((z & 15) * 16 + (x & 15)) as usize;
        let first = self.first_available[column];
        if y <= first - 2 {
            return;
        }
        if !palette.is_air(state) {
            if y >= first {
                self.first_available[column] = y + 1;
            }
        } else if first - 1 == y {
            self.first_available[column] = (self.min_y..y)
                .rev()
                .find(|&yy| !palette.is_air(self.get([x, yy, z])))
                .map_or(self.min_y, |yy| yy + 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_heightmap_follows_the_top_block() {
        let mut palette = Palette::new();
        let stone = palette.state("minecraft:stone");
        let mut chunk = Chunk::new([0, 0], -4, 16);
        assert_eq!(chunk.height_at(3, 5), -5);
        for y in -4..3 {
            chunk.put([3, y, 5], stone);
        }
        chunk.prime_heightmap(&palette);
        assert_eq!(chunk.height_at(3, 5), 2);
        // A block above raises it, and removing the top block lowers it to the next.
        chunk.set(&palette, [3, 6, 5], stone);
        assert_eq!(chunk.height_at(3, 5), 6);
        chunk.set(&palette, [3, 6, 5], AIR);
        assert_eq!(chunk.height_at(3, 5), 2);
        // Air below the top changes nothing; outside the level, nothing is set.
        chunk.set(&palette, [3, 0, 5], AIR);
        assert_eq!(chunk.height_at(3, 5), 2);
        chunk.set(&palette, [3, 40, 5], stone);
        assert_eq!((chunk.get([3, 40, 5]), chunk.height_at(3, 5)), (AIR, 2));
    }
}
