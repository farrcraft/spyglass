//! A chunk's blocks before features (ADR 0050): the noise fill, the material rule with its ore
//! veins, carvers, and the biome lookups they use. See `docs/specs/material.md` and
//! `docs/specs/carvers.md`.

pub mod carver;
pub mod chunk;
pub mod material;
pub mod state;
pub mod temperature;
pub mod zoom;

use std::collections::HashMap;

use spyglass_biome::{BiomeRegion, BiomeSource, Biomes};
use spyglass_data::bundle::Bundle;
use spyglass_data::codec::{At, LoadError, LoadErrorKind};
use spyglass_rng::worldgen::large_feature;
use spyglass_terrain::{Block, BlockColumn, Terrain, TerrainSettings, VeinBlock};

use carver::{CarverSettings, CarvingMask};
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
    /// Each biome's carvers, from 26.3 on; `None` for 26.2's, which aren't ported yet.
    carvers: Option<Carving>,
}

/// What applying a carving mask needs (26.3 on).
#[derive(Debug, Clone)]
struct Carving {
    carvers: CarverSettings,
    /// By state: whether it's in `#minecraft:uncarvable`.
    uncarvable: Vec<bool>,
}

/// `BlockTags.UNCARVABLE`.
const UNCARVABLE: &str = "minecraft:uncarvable";

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
        let carvers = if material.is_v1_21() {
            None
        } else {
            // A tag the code names but the data leaves out is empty.
            let uncarvable = if bundle.resource("tags/block", UNCARVABLE).is_ok() {
                spyglass_terrain::tag(bundle, "tags/block", UNCARVABLE)?
            } else {
                Vec::new()
            };
            let carvers = CarverSettings::load(bundle, biomes.biome_names())?;
            Some(Carving {
                carvers,
                uncarvable: (0..palette.len())
                    .map(|s| {
                        uncarvable
                            .iter()
                            .any(|id| id == palette.block_id(s as State))
                    })
                    .collect(),
            })
        };
        Ok(BlockSettings {
            terrain,
            biomes,
            material,
            climates,
            palette,
            veins,
            carvers,
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
            seed,
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
    seed: i64,
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
        self.surface(cx, cz).0
    }

    /// The chunk at chunk `(cx, cz)` after the noise fill, the material rule and carvers
    /// (`NoiseBasedChunkGenerator.generateCarvers`, `docs/specs/carvers.md`). 26.2's carvers
    /// aren't ported yet, and are an error.
    pub fn carved_chunk(&self, cx: i32, cz: i32) -> Result<Chunk, LoadError> {
        let settings = self.settings;
        let Some(carving) = &settings.carvers else {
            return Err(At::root("carvers").error(LoadErrorKind::Unsupported {
                what: "carvers of the 1.21 family, as 26.2's, in",
                name: "the chunk generator".to_string(),
            }));
        };
        let (mut chunk, region) = self.surface(cx, cz);
        let anchors = settings.terrain.anchors();
        // The mask leaves out the level's bottom block and its top eight.
        let top = anchors.min_y + anchors.height - 1 - 7;
        let mut mask = CarvingMask::new(anchors.min_y + 1, top);
        // What a tied carver biome could carve instead.
        let mut doubt = CarvingMask::new(anchors.min_y + 1, top);
        for dx in -8..=8 {
            for dz in -8..=8 {
                let start = [cx.wrapping_add(dx), cz.wrapping_add(dz)];
                // getBiomeGenerationSettingsForCarver: the source's biome at the chunk's corner
                // quart at y 0.
                let lookup =
                    self.biomes
                        .biome_at(start[0].wrapping_mul(4), 0, start[1].wrapping_mul(4));
                let carve = |biome, mask: &mut CarvingMask| {
                    for (i, carver) in carving.carvers.of(biome).enumerate() {
                        let mut random =
                            large_feature(self.seed.wrapping_add(i as i64), start[0], start[1]);
                        if carver.is_start_chunk(&mut random) {
                            carver.carve(anchors, &mut random, [cx, cz], start, mask);
                        }
                    }
                };
                let first = lookup.biome();
                carve(first, &mut mask);
                if lookup
                    .candidates()
                    .iter()
                    .any(|&b| !carving.carvers.same(b, first))
                {
                    for &b in lookup.candidates() {
                        carve(b, &mut doubt);
                    }
                }
            }
        }
        if !mask.is_empty() {
            let biomes = ChunkBiomes::new(&region, chunk.min_y, chunk.height);
            self.apply_carving_mask(&mut chunk, &mask, carving, &biomes);
        }
        let [x0, z0] = [cx.wrapping_shl(4), cz.wrapping_shl(4)];
        for (x, z, y0, y1) in doubt.runs() {
            for y in y0 - 1..=y1 {
                chunk.mark_uncertain([x0 + x, y, z0 + z]);
            }
        }
        Ok(chunk)
    }

    /// `applyCarvingMask`: each carved position, unless it's `#uncarvable`, gets the aquifer's
    /// substance at density 0; under a run that carved grass or mycelium, dirt becomes the
    /// rule's top material.
    fn apply_carving_mask(
        &self,
        chunk: &mut Chunk,
        mask: &CarvingMask,
        carving: &Carving,
        biomes: &ChunkBiomes<'_>,
    ) {
        let palette = &self.settings.palette;
        let [cx, cz] = chunk.pos;
        let mut columns: HashMap<(i32, i32), BlockColumn> = HashMap::new();
        for (lx, lz, y0, y1) in mask.runs() {
            let (x, z) = (cx.wrapping_shl(4) + lx, cz.wrapping_shl(4) + lz);
            let column = columns
                .entry((lx, lz))
                .or_insert_with(|| self.terrain.aquifer_column(x, z, Some(0.0)));
            let mut grass = false;
            for y in (y0..=y1).rev() {
                let state = chunk.get([x, y, z]);
                if carving.uncarvable[usize::from(state)] {
                    continue;
                }
                if matches!(
                    palette.block_id(state),
                    "minecraft:grass_block" | "minecraft:mycelium"
                ) {
                    grass = true;
                }
                let substance = match column.at(y) {
                    Block::Air => state::AIR,
                    Block::Water => state::WATER,
                    Block::Lava => state::LAVA,
                    // computeSubstance's null: solid, left as it is.
                    Block::Default | Block::Vein(_) => continue,
                };
                chunk.set(palette, [x, y, z], substance);
                let below = [x, y - 1, z];
                if grass && palette.block_id(chunk.get(below)) == "minecraft:dirt" {
                    let fluid = palette.is_fluid(substance);
                    let (top, tied) = self.material.top_material(chunk, biomes, below, fluid);
                    if let Some(top) = top {
                        chunk.set(palette, below, top);
                    }
                    if tied {
                        chunk.mark_uncertain(below);
                    }
                }
            }
        }
    }

    /// The chunk after the noise fill and the material rule, and the biome region around it.
    fn surface(&self, cx: i32, cz: i32) -> (Chunk, BiomeRegion) {
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
        (chunk, region)
    }
}
