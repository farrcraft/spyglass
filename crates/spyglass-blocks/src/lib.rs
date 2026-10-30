//! A chunk's blocks before features (ADR 0050): the noise fill, the material rule with its ore
//! veins, and the biome lookups they use. See `docs/specs/material.md`.

pub mod chunk;
pub mod material;
pub mod state;
pub mod temperature;
pub mod zoom;

use spyglass_biome::{BiomeSource, Biomes};
use spyglass_data::bundle::Bundle;
use spyglass_data::codec::LoadError;
use spyglass_terrain::{Block, Terrain, TerrainSettings, VeinBlock};

pub use chunk::Chunk;
use material::{BoundMaterial, ChunkBiomes, MaterialSettings};
pub use state::{Palette, State};
use temperature::Climate;

/// A dimension's settings for blocks, seed-independent: its terrain, biome source and material
/// rule.
#[derive(Debug, Clone)]
pub struct BlockSettings {
    terrain: TerrainSettings,
    biomes: BiomeSource,
    material: MaterialSettings,
    climates: Vec<Climate>,
    palette: Palette,
    /// 26.2's ore vein blocks, as [`vein_index`] orders them.
    veins: [State; 6],
}

/// 26.2's ore vein blocks, in order.
const VEINS: [VeinBlock; 6] = [
    VeinBlock::CopperOre,
    VeinBlock::RawCopperBlock,
    VeinBlock::Granite,
    VeinBlock::DeepslateIronOre,
    VeinBlock::RawIronBlock,
    VeinBlock::Tuff,
];

/// A vein block's place in [`VEINS`].
fn vein_index(block: VeinBlock) -> usize {
    VEINS
        .iter()
        .position(|&v| v == block)
        .expect("every vein block is listed")
}

impl BlockSettings {
    /// Loads `dimension` of world preset `preset` (e.g. `minecraft:normal`) from `bundle`.
    pub fn load(bundle: &Bundle, preset: &str, dimension: &str) -> Result<Self, LoadError> {
        let biomes = BiomeSource::load(bundle, preset, dimension)?;
        let noise_settings = biomes.noise_settings().to_string();
        let terrain = TerrainSettings::load(bundle, &noise_settings, dimension)?;
        let mut palette = Palette::new();
        let material =
            MaterialSettings::load(bundle, &noise_settings, terrain.anchors(), &mut palette)?;
        let climates = biomes
            .biome_names()
            .names()
            .iter()
            .map(|id| Climate::load(bundle, id))
            .collect::<Result<_, _>>()?;
        let veins = VEINS.map(|v| palette.state(v.id()));
        Ok(BlockSettings {
            terrain,
            biomes,
            material,
            climates,
            palette,
            veins,
        })
    }

    /// The states blocks can be.
    pub fn palette(&self) -> &Palette {
        &self.palette
    }

    /// A biome's id.
    pub fn biome_name(&self, biome: spyglass_biome::BiomeId) -> &str {
        self.biomes.name(biome)
    }

    /// The dimension's terrain settings.
    pub fn terrain(&self) -> &TerrainSettings {
        &self.terrain
    }

    /// Everything bound to `seed`, ready to make chunks.
    pub fn generator(&self, seed: i64) -> Result<ChunkGenerator<'_>, LoadError> {
        Ok(ChunkGenerator {
            settings: self,
            terrain: Terrain::new(&self.terrain, seed)?,
            biomes: self.biomes.with_seed(seed)?,
            material: self.material.bind(
                seed,
                &self.palette,
                self.biomes.biome_names(),
                &self.climates,
            )?,
        })
    }
}

/// One seed's chunks.
pub struct ChunkGenerator<'s> {
    settings: &'s BlockSettings,
    terrain: Terrain,
    biomes: Biomes<'s>,
    material: BoundMaterial,
}

impl ChunkGenerator<'_> {
    /// The biome block `pos` has, as chunk generation sees it (`BiomeManager.getBiome`): the
    /// biome of a nearby quart, picked by the zoom, with the quart's y within the level. With
    /// the tied biomes, when the quart is tied.
    pub fn block_biome(&self, pos: [i32; 3]) -> spyglass_biome::Lookup {
        let t = &self.settings.terrain;
        let [qx, qy, qz] = self.material.biome_quart(pos);
        let min_qy = t.level_min_y >> 2;
        let qy = qy.clamp(min_qy, min_qy + (t.level_height >> 2) - 1);
        self.biomes.biome_at(qx, qy, qz)
    }

    /// The chunk at chunk `(cx, cz)` after the noise fill and the material rule, without carvers.
    pub fn chunk(&self, cx: i32, cz: i32) -> Chunk {
        let settings = self.settings;
        let (min_y, height) = (settings.terrain.level_min_y, settings.terrain.level_height);
        let mut chunk = Chunk::new([cx, cz], min_y, height);
        let default = settings.material.default_block();
        let (x0, z0) = (cx.wrapping_shl(4), cz.wrapping_shl(4));
        for dz in 0..16 {
            for dx in 0..16 {
                let column = self.terrain.base_column(x0 + dx, z0 + dz);
                for y in min_y..min_y + height {
                    let state = match column.at(y) {
                        Block::Air => state::AIR,
                        Block::Water => state::WATER,
                        Block::Lava => state::LAVA,
                        Block::Default => default,
                        Block::Vein(v) => settings.veins[vein_index(v)],
                    };
                    chunk.put([dx, y, dz], state);
                }
            }
        }
        chunk.prime_heightmap(&settings.palette);
        let quarts = height >> 2;
        let region = self.biomes.biomes_in(
            [(x0 >> 2) - 1, min_y >> 2, (z0 >> 2) - 1],
            [6, quarts as u32, 6],
        );
        let biomes = ChunkBiomes::new(&region, min_y, height);
        self.material
            .build_surface(&mut chunk, &settings.palette, &biomes, &self.terrain);
        chunk
    }
}
