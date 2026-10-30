//! A chunk's blocks before features: the noise fill, the material rule and carvers
//! (`docs/specs/material.md`, `docs/specs/carvers.md`).

use spyglass_blocks::{Chunk, ChunkGenerator, Palette};

use crate::{Result, Session};

/// Whether a chunk's blocks are asked for before or after carvers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Carvers {
    /// After carvers: with the caves and ravines they cut, as the game's chunks are before
    /// features.
    On,
    /// Before carvers: the noise fill and the material rule, with noise caves but not the
    /// caves and ravines carvers cut.
    Off,
}

/// A block, from [`Session::block_at`] and [`Session::block_column`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct BlockAt {
    /// The block state: its id, then its properties that aren't the default in brackets, as in
    /// `minecraft:grass_block[snowy=true]`.
    pub block: String,
    /// Whether this is the game's block whatever its lookup history. Otherwise it depends on a
    /// tied biome cell, and the game may have another (`docs/specs/material.md#ties`).
    pub certain: bool,
}

/// A column of blocks from the level's bottom up, from [`Session::block_column`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct BlockColumn {
    /// The y of the first block.
    pub min_y: i32,
    /// Each block, from `min_y` up to the level's top.
    pub blocks: Vec<BlockAt>,
}

/// A chunk's blocks, from [`Session::chunk_blocks`].
#[derive(Debug, Clone)]
pub struct ChunkBlocks<'w> {
    chunk: Chunk,
    palette: &'w Palette,
}

impl<'w> ChunkBlocks<'w> {
    /// The chunk, `[x, z]`.
    pub fn chunk(&self) -> [i32; 2] {
        self.chunk.pos
    }

    /// The level's lowest y.
    pub fn min_y(&self) -> i32 {
        self.chunk.min_y
    }

    /// The level's height in blocks.
    pub fn height(&self) -> i32 {
        self.chunk.height
    }

    /// The block state at `(x, y, z)`, named as [`BlockAt::block`] is. Only the low four bits
    /// of x and z count. Air outside the level.
    pub fn block(&self, pos: [i32; 3]) -> &'w str {
        self.palette.name(self.chunk.get(pos))
    }

    /// Whether the block at `(x, y, z)` is certain, as [`BlockAt::certain`] says.
    pub fn is_certain(&self, pos: [i32; 3]) -> bool {
        self.chunk.is_certain(pos)
    }

    /// The block at `(x, y, z)`.
    pub fn block_at(&self, pos: [i32; 3]) -> BlockAt {
        BlockAt {
            block: self.block(pos).to_string(),
            certain: self.is_certain(pos),
        }
    }

    /// The column at `(x, z)`, from the level's bottom up.
    pub fn column(&self, [x, z]: [i32; 2]) -> BlockColumn {
        let min_y = self.min_y();
        BlockColumn {
            min_y,
            blocks: (min_y..min_y + self.height())
                .map(|y| self.block_at([x, y, z]))
                .collect(),
        }
    }
}

impl<'w> Session<'w> {
    /// The seed's chunk generator, bound the first time it's asked for.
    fn chunks(&self) -> Result<&ChunkGenerator<'w>> {
        if let Some(generator) = self.chunks.get() {
            return Ok(generator);
        }
        let generator = self.world.block_settings()?.generator(self.seed)?;
        Ok(self.chunks.get_or_init(|| generator))
    }

    /// The blocks of chunk `[x, z]`, as the game generates them before features, with or
    /// without carvers (`docs/specs/material.md`, `docs/specs/carvers.md`). Every query
    /// generates the whole chunk, which takes a while: keep the answer to ask about more than
    /// one block. Versions whose material rule isn't ported, such as 26.4's, are an error.
    pub fn chunk_blocks(&self, chunk: [i32; 2], carvers: Carvers) -> Result<ChunkBlocks<'w>> {
        let generator = self.chunks()?;
        let [x, z] = chunk;
        Ok(ChunkBlocks {
            chunk: match carvers {
                Carvers::On => generator.carved_chunk(x, z),
                Carvers::Off => generator.chunk(x, z),
            },
            palette: self.world.block_settings()?.palette(),
        })
    }

    /// The block at `(x, y, z)`, from [its chunk](Self::chunk_blocks).
    pub fn block_at(&self, [x, y, z]: [i32; 3], carvers: Carvers) -> Result<BlockAt> {
        let blocks = self.chunk_blocks([x >> 4, z >> 4], carvers)?;
        Ok(blocks.block_at([x, y, z]))
    }

    /// The column of blocks at `(x, z)`, from [its chunk](Self::chunk_blocks).
    pub fn block_column(&self, [x, z]: [i32; 2], carvers: Carvers) -> Result<BlockColumn> {
        let blocks = self.chunk_blocks([x >> 4, z >> 4], carvers)?;
        Ok(blocks.column([x, z]))
    }
}
