//! A chunk's blocks before features (ADR 0050): the noise fill, the material rule with its ore
//! veins, carvers, and the biome lookups they use. See `docs/specs/material.md` and
//! `docs/specs/carvers.md`.

pub mod behaviour;
pub mod carver;
pub mod chunk;
pub mod material;
pub mod state;
pub mod temperature;
pub mod zoom;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use spyglass_biome::{BiomeRegion, BiomeSource, Biomes};
use spyglass_data::anchor::AnchorHeights;
use spyglass_data::bundle::Bundle;
use spyglass_data::codec::LoadError;
use spyglass_data::versions::Behavior;
use spyglass_rng::LegacyRandomSource;
use spyglass_rng::worldgen::large_feature;
use spyglass_terrain::{Block, Probe, Substances, Terrain, TerrainSettings, VeinBlock};

use carver::{Carver, CarverSettings, CarvingMask, Carvings, Legacy, Sink};
pub use chunk::Chunk;
use material::{BoundMaterial, ChunkBiomes, MaterialSettings, NoiseColumn, Slot, VeinAt, VeinPart};
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
    /// Each biome's carvers.
    carvers: Carving,
    /// `minecraft:cave_air`, which 26.2's Nether carver fills with.
    cave_air: State,
    /// Which heights the carving mask spans.
    mask_bounds: MaskBounds,
}

/// Which heights the carving mask spans, from 1 above the bottom to 7 below the top
/// (`docs/specs/carvers.md#264-the-carving-mask-with-the-rule`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MaskBounds {
    /// The vertical anchors' context, to 26.4-snapshot-5.
    Context,
    /// The level's (switch `carving_mask_level_bounds`).
    Level,
    /// The anchors' context, its top kept within the level's (switch
    /// `carving_mask_context_within_level`).
    ContextWithinLevel,
}

impl MaskBounds {
    /// The bounds a version's switches give.
    fn of(behavior: &Behavior) -> Self {
        if behavior.carving_mask_level_bounds {
            MaskBounds::Level
        } else if behavior.carving_mask_context_within_level {
            MaskBounds::ContextWithinLevel
        } else {
            MaskBounds::Context
        }
    }

    /// The mask's lowest and highest y, given the anchors' `context` and the `level`.
    fn range(self, context: AnchorHeights, level: AnchorHeights) -> (i32, i32) {
        let top = |heights: AnchorHeights| heights.min_y + heights.height - 1;
        match self {
            MaskBounds::Context => (context.min_y + 1, top(context) - 7),
            MaskBounds::Level => (level.min_y + 1, top(level) - 7),
            MaskBounds::ContextWithinLevel => (
                context.min_y.max(level.min_y) + 1,
                top(context).min(top(level)) - 7,
            ),
        }
    }
}

/// The carvers, and what applying a carving mask needs (26.3 on).
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
        let cave_air = palette.state("minecraft:cave_air");
        let v1_21 = material.is_v1_21();
        // A tag the code names but the data leaves out is empty; 26.2 has none.
        let uncarvable = if !v1_21 && bundle.resource("tags/block", UNCARVABLE).is_ok() {
            spyglass_terrain::tag(bundle, "tags/block", UNCARVABLE)?
        } else {
            Vec::new()
        };
        let carvers = Carving {
            carvers: CarverSettings::load(bundle, biomes.biome_names(), v1_21)?,
            uncarvable: (0..palette.len())
                .map(|s| {
                    uncarvable
                        .iter()
                        .any(|id| id == palette.block_id(s as State))
                })
                .collect(),
        };
        Ok(BlockSettings {
            terrain,
            biomes,
            material,
            climates,
            palette,
            veins,
            carvers,
            cave_air,
            mask_bounds: MaskBounds::of(&spyglass_density::Resources::behavior(bundle)),
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

    /// Whether the dimension has ore veins: in its material rule from 26.3, in its noise router
    /// in 26.2.
    pub fn has_veins(&self) -> bool {
        if self.material.is_v1_21() {
            self.terrain
                .router_v1_21()
                .is_some_and(|router| router.ore_veins_enabled())
        } else {
            self.material.has_ore_veins()
        }
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
            windows: Mutex::new(HashMap::new()),
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
    /// Chunks' biome windows ([`ChunkGenerator::biome_window`]), kept because a feature query
    /// asks for the same chunk's several times: the outline, the filter on which features can
    /// place and the surface. Each depends only on its chunk, so keeping it changes no answer.
    /// Cleared when it holds [`MAX_KEPT_WINDOWS`].
    windows: Mutex<HashMap<[i32; 2], Arc<BiomeRegion>>>,
}

/// How many chunks' biome windows a [`ChunkGenerator`] keeps, about 7 KB each: enough for the
/// rows of chunks a feature query keeps around the one it's placing.
const MAX_KEPT_WINDOWS: usize = 2048;

impl ChunkGenerator<'_> {
    /// The biome block `pos` has, as chunk generation sees it (`BiomeManager.getBiome`): the
    /// biome of a nearby quart, picked by the zoom, with the quart's y within the level. With
    /// the tied biomes, when the quart is tied.
    pub fn block_biome(&self, pos: [i32; 3]) -> spyglass_biome::Lookup {
        let terrain = &self.settings.terrain;
        let [qx, qy, qz] = self.material.biome_quart(pos);
        let min_qy = terrain.level_min_y >> 2;
        let qy = qy.clamp(min_qy, min_qy + (terrain.level_height >> 2) - 1);
        self.biomes.biome_at(qx, qy, qz)
    }

    /// Every biome chunk `(cx, cz)` can store, with tied candidates: each quart's over the
    /// level's height, and a quart's border around, since 26.4's upscaled biomes can take a
    /// neighbouring quart's. Decoration runs a placed feature only where one of these lists it.
    pub fn chunk_biomes(&self, cx: i32, cz: i32) -> Vec<spyglass_biome::BiomeId> {
        let window = self.biome_window(cx, cz);
        let [min_qx, min_qy, min_qz] = window.min();
        let [size_x, size_y, size_z] = window.size().map(|s| s as i32);
        let mut out = Vec::new();
        for qz in min_qz..min_qz + size_z {
            for qx in min_qx..min_qx + size_x {
                for qy in min_qy..min_qy + size_y {
                    let lookup = window.get(qx, qy, qz).expect("inside the window");
                    for b in
                        std::iter::once(lookup.biome()).chain(lookup.candidates().iter().copied())
                    {
                        if !out.contains(&b) {
                            out.push(b);
                        }
                    }
                }
            }
        }
        out
    }

    /// The biomes of chunk `(cx, cz)`'s quarts and one more on every horizontal side, over the
    /// level's height: what its surface reads, and what [`chunk_biomes`](Self::chunk_biomes)
    /// lists.
    fn biome_window(&self, cx: i32, cz: i32) -> Arc<BiomeRegion> {
        let lock = || self.windows.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(window) = lock().get(&[cx, cz]) {
            return window.clone();
        }
        let terrain = &self.settings.terrain;
        let (x0, z0) = (cx.wrapping_shl(4), cz.wrapping_shl(4));
        let window = Arc::new(self.biomes.biomes_in(
            [(x0 >> 2) - 1, terrain.level_min_y >> 2, (z0 >> 2) - 1],
            [6, (terrain.level_height >> 2) as u32, 6],
        ));
        let mut kept = lock();
        if kept.len() >= MAX_KEPT_WINDOWS {
            kept.clear();
        }
        kept.insert([cx, cz], window.clone());
        window
    }

    /// `Biome.BIOME_INFO_NOISE` at `(x, z)`, which placement's noise counts sample.
    pub fn biome_info(&self, x: f64, z: f64) -> f64 {
        self.material.biome_info(x, z)
    }

    /// The noise fill's block at `y` at every `(x, z)` with x in `xs` and z in `zs`, z outer
    /// and x inner, and whether a solid block is above it ([`Terrain::probes_on`]).
    pub fn probes_on(&self, xs: &[i32], zs: &[i32], y: i32) -> Vec<Probe> {
        self.terrain.probes_on(xs, zs, y)
    }

    /// The ore vein's block at `pos`, if the chunk has one there, given the noise fill's block
    /// at `pos`: in 26.2, the fill's own vein blocks; from 26.3, the material rule's veins at
    /// the fill's solid positions ([`BoundMaterial::vein_at`]).
    pub fn vein_at(&self, pos: [i32; 3], block: Block) -> Option<VeinAt> {
        match block {
            Block::Vein(v) => {
                // `VEINS` holds each vein's ore, raw ore and filler, in that order.
                let index = vein_index(v);
                let part = [VeinPart::Ore, VeinPart::RawOre, VeinPart::Filler][index % 3];
                Some(VeinAt {
                    ore: self.settings.veins[index - index % 3],
                    part,
                })
            }
            Block::Default if !self.settings.material.is_v1_21() => self.material.vein_at(pos),
            _ => None,
        }
    }

    /// The positions carvers carve in chunk `(cx, cz)`, by the kind of the first carver to
    /// carve each, over the carving mask's range (`docs/specs/carvers.md`). Where a start
    /// chunk's carver biome is tied, its first candidate's carvers.
    pub fn carvings(&self, cx: i32, cz: i32) -> Carvings {
        let (min_y, max_y) = self.mask_range();
        let mut carvings = Carvings::new(min_y, max_y);
        let mut doubt = CarvingMask::new(min_y, max_y);
        let anchors = self.settings.terrain.anchors();
        self.visit_starts([cx, cz], &mut doubt, |carver, _, random, start| {
            let mut sink = carvings.sink(carver.kind());
            carver.carve(anchors, random, [cx, cz], start, &mut sink);
        });
        carvings
    }

    /// Every position a carver could carve in chunk `(cx, cz)`: [`carvings`](Self::carvings)'
    /// positions, and where a start chunk's carver biome is tied, every candidate's.
    pub fn carvable(&self, cx: i32, cz: i32) -> CarvingMask {
        let (min_y, max_y) = self.mask_range();
        let mut mask = CarvingMask::new(min_y, max_y);
        let mut doubt = CarvingMask::new(min_y, max_y);
        let anchors = self.settings.terrain.anchors();
        self.visit_starts([cx, cz], &mut doubt, |carver, _, random, start| {
            carver.carve(anchors, random, [cx, cz], start, &mut mask);
        });
        mask.add(&doubt);
        mask
    }

    /// The level's lowest y and the y above its top.
    pub fn level(&self) -> (i32, i32) {
        let terrain = &self.settings.terrain;
        (
            terrain.level_min_y,
            terrain.level_min_y + terrain.level_height,
        )
    }

    /// The final density of the noise fill over a box, the columns' values
    /// ([`Terrain::densities`]): positive where it fills the default block. `None` for 26.2.
    pub fn densities(&self, min: [i32; 3], size: [i32; 3]) -> Option<Vec<f32>> {
        self.terrain.densities(min, size)
    }

    /// The biomes where the material rule's extensions add blocks over the noise fill: eroded
    /// badlands' pillars and the frozen oceans' icebergs.
    pub fn extension_biomes(&self) -> Vec<spyglass_biome::BiomeId> {
        self.material.extension_biomes()
    }

    /// Where the material rule can give a position air: in the biomes returned, or `None` if
    /// anywhere ([`BoundMaterial::air_biomes`]).
    pub fn rule_air_biomes(&self) -> Option<Vec<spyglass_biome::BiomeId>> {
        self.material.air_biomes(&self.settings.palette)
    }

    /// The carving mask's range ([`MaskBounds`]).
    fn mask_range(&self) -> (i32, i32) {
        let terrain = &self.settings.terrain;
        let level = AnchorHeights {
            min_y: terrain.level_min_y,
            height: terrain.level_height,
        };
        self.settings.mask_bounds.range(terrain.anchors(), level)
    }

    /// The chunk at chunk `(cx, cz)` after the noise fill and the material rule, without carvers.
    pub fn chunk(&self, cx: i32, cz: i32) -> Chunk {
        if self.settings.material.default_block().is_none() {
            return self.built_chunk(cx, cz, false);
        }
        self.surface(cx, cz).0
    }

    /// The chunk at chunk `(cx, cz)` after the noise fill, the material rule and carvers
    /// (`docs/specs/carvers.md`): in 26.3, carved into a mask and applied after
    /// (`generateCarvers`); in 26.2, straight into the chunk (`applyCarvers`); in 26.4, the mask
    /// applied with the rule (`ChunkTerrainBuilder`).
    pub fn carved_chunk(&self, cx: i32, cz: i32) -> Chunk {
        let settings = self.settings;
        if settings.material.default_block().is_none() {
            return self.built_chunk(cx, cz, true);
        }
        let carving = &settings.carvers;
        let v1_21 = settings.material.is_v1_21();
        let (mut chunk, region) = self.surface(cx, cz);
        let biomes = ChunkBiomes::new(&region, chunk.min_y, chunk.height);
        let anchors = settings.terrain.anchors();
        let (min_y, max_y) = self.mask_range();
        let mut mask = CarvingMask::new(min_y, max_y);
        // What a tied carver biome could carve instead.
        let mut doubt = CarvingMask::new(min_y, max_y);
        let mut substances = self.terrain.substances();
        self.visit_starts(
            [cx, cz],
            &mut doubt,
            |carver, legacy, random, start| match legacy {
                Some(legacy) if v1_21 => {
                    let mut direct = Direct {
                        generator: self,
                        chunk: &mut chunk,
                        mask: &mut mask,
                        legacy,
                        lava_level: legacy.lava_level.resolve(anchors),
                        nether_lava: anchors.min_y + 31,
                        biomes: &biomes,
                        substances: &mut substances,
                        grass: false,
                    };
                    carver.carve(anchors, random, [cx, cz], start, &mut direct);
                }
                _ => carver.carve(anchors, random, [cx, cz], start, &mut mask),
            },
        );
        if !v1_21 && !mask.is_empty() {
            self.apply_carving_mask(&mut chunk, &mask, carving, &biomes, &mut substances);
        }
        Self::mark_doubt(&mut chunk, &doubt);
        chunk
    }

    /// The carvers that start within 8 chunks of `chunk`, x outer and z inner, each with its
    /// seeded random once `isStartChunk` passed. Where a start chunk's carver biome is tied and
    /// its candidates have different carvers, each candidate's carving goes into `doubt`.
    fn visit_starts(
        &self,
        [cx, cz]: [i32; 2],
        doubt: &mut CarvingMask,
        mut carve: impl FnMut(&Carver, Option<&Legacy>, &mut LegacyRandomSource, [i32; 2]),
    ) {
        let carving = &self.settings.carvers;
        let anchors = self.settings.terrain.anchors();
        let random = |i: usize, start: [i32; 2]| {
            large_feature(self.seed.wrapping_add(i as i64), start[0], start[1])
        };
        for dx in -8..=8 {
            for dz in -8..=8 {
                let start = [cx.wrapping_add(dx), cz.wrapping_add(dz)];
                // The source's biome at the start chunk's corner quart at y 0, uncached.
                let lookup =
                    self.biomes
                        .biome_at(start[0].wrapping_mul(4), 0, start[1].wrapping_mul(4));
                let first = lookup.biome();
                for (i, (carver, legacy)) in carving.carvers.of(first).enumerate() {
                    let mut random = random(i, start);
                    if carver.is_start_chunk(&mut random) {
                        carve(carver, legacy, &mut random, start);
                    }
                }
                if lookup
                    .candidates()
                    .iter()
                    .any(|&b| !carving.carvers.same(b, first))
                {
                    for &b in lookup.candidates() {
                        for (i, (carver, _)) in carving.carvers.of(b).enumerate() {
                            let mut random = random(i, start);
                            if carver.is_start_chunk(&mut random) {
                                carver.carve(anchors, &mut random, [cx, cz], start, doubt);
                            }
                        }
                    }
                }
            }
        }
    }

    /// Marks every position `doubt` carves, and the one below it, as uncertain.
    fn mark_doubt(chunk: &mut Chunk, doubt: &CarvingMask) {
        let [x0, z0] = chunk.pos.map(|c| c.wrapping_shl(4));
        for (x, z, bottom, top) in doubt.runs() {
            for y in bottom - 1..=top {
                chunk.mark_uncertain([x0 + x, y, z0 + z]);
            }
        }
    }

    /// 26.4's `buildTerrain`: the chunk's noise columns, with carvers' mask if `carvers`, filled
    /// by `ChunkTerrainBuilder` (`docs/specs/material.md#264`).
    fn built_chunk(&self, cx: i32, cz: i32, carvers: bool) -> Chunk {
        let settings = self.settings;
        let (min_y, height) = (settings.terrain.level_min_y, settings.terrain.level_height);
        // NoiseChunk.prepareColumn: the aquifer's answer up to each column's top.
        let mut columns: Vec<NoiseColumn> = Vec::with_capacity(256);
        for column in self.terrain.base_columns(cx, cz) {
            let slots: Vec<Slot> = column
                .blocks
                .iter()
                .map(|b| match b {
                    Block::Air => Slot::Air,
                    Block::Water => Slot::Fluid(state::WATER),
                    Block::Lava => Slot::Fluid(state::LAVA),
                    Block::Default | Block::Vein(_) => Slot::Solid,
                })
                .collect();
            let top = slots
                .iter()
                .rposition(|s| *s != Slot::Air)
                .map_or(-1, |i| i as i32);
            columns.push(NoiseColumn {
                min_y: column.min_y,
                slots,
                top,
                gradient: [0, 0],
            });
        }
        // The gradients: the neighbours' tops within the chunk, before any pillar.
        let tops: Vec<i32> = columns.iter().map(|c| c.top).collect();
        let top = |dx: i32, dz: i32| tops[(dz * 16 + dx) as usize];
        for dz in 0..16 {
            for dx in 0..16 {
                columns[(dz * 16 + dx) as usize].gradient = [
                    top((dx + 1).min(15), dz) - top((dx - 1).max(0), dz),
                    top(dx, (dz + 1).min(15)) - top(dx, (dz - 1).max(0)),
                ];
            }
        }
        // generateCarvingMask.
        let anchors = settings.terrain.anchors();
        let mask_range = self.mask_range();
        let mut doubt = CarvingMask::new(mask_range.0, mask_range.1);
        let mask = carvers.then(|| {
            let mut mask = CarvingMask::new(mask_range.0, mask_range.1);
            self.visit_starts([cx, cz], &mut doubt, |carver, _, random, start| {
                carver.carve(anchors, random, [cx, cz], start, &mut mask);
            });
            mask
        });
        let mut chunk = Chunk::new([cx, cz], min_y, height);
        let region = self.biome_window(cx, cz);
        let biomes = ChunkBiomes::new(&region, min_y, height);
        let mut substances = self.terrain.substances();
        self.material.build_terrain(
            &mut chunk,
            &settings.palette,
            &biomes,
            &mut columns,
            mask.as_ref(),
            &settings.carvers.uncarvable,
            &mut |pos| Self::substance(&mut substances, pos),
        );
        Self::mark_doubt(&mut chunk, &doubt);
        chunk
    }

    /// `computeSubstance(x, y, z, 0)`: the aquifer's block at a carved position, as a state, or
    /// `None` where it's solid (the game's `null`).
    fn substance(substances: &mut Substances<'_>, [x, y, z]: [i32; 3]) -> Option<State> {
        match substances.at(x, y, z) {
            Block::Air => Some(state::AIR),
            Block::Water => Some(state::WATER),
            Block::Lava => Some(state::LAVA),
            Block::Default | Block::Vein(_) => None,
        }
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
        substances: &mut Substances<'_>,
    ) {
        let palette = &self.settings.palette;
        let [cx, cz] = chunk.pos;
        for (lx, lz, bottom, top) in mask.runs() {
            let (x, z) = (cx.wrapping_shl(4) + lx, cz.wrapping_shl(4) + lz);
            // Whether this run has carved grass or mycelium above.
            let mut grass = false;
            for y in (bottom..=top).rev() {
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
                let Some(substance) = Self::substance(substances, [x, y, z]) else {
                    continue;
                };
                chunk.set(palette, [x, y, z], substance);
                let below = [x, y - 1, z];
                if grass && palette.block_id(chunk.get(below)) == "minecraft:dirt" {
                    let fluid = palette.is_fluid(substance);
                    let (top, tied) = self.material.top_material(
                        chunk,
                        biomes,
                        Some(&self.terrain),
                        below,
                        fluid,
                    );
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
    fn surface(&self, cx: i32, cz: i32) -> (Chunk, Arc<BiomeRegion>) {
        let settings = self.settings;
        let (min_y, height) = (settings.terrain.level_min_y, settings.terrain.level_height);
        let mut chunk = Chunk::new([cx, cz], min_y, height);
        let default = settings
            .material
            .default_block()
            .expect("chunks filled by default block have one");
        let columns = self.terrain.base_columns(cx, cz);
        for dz in 0..16 {
            for dx in 0..16 {
                let column = &columns[(dz * 16 + dx) as usize];
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
        let region = self.biome_window(cx, cz);
        let biomes = ChunkBiomes::new(&region, min_y, height);
        self.material
            .build_surface(&mut chunk, &settings.palette, &biomes, &self.terrain);
        (chunk, region)
    }
}

/// 26.2's carving straight into the chunk (`WorldCarver.carveEllipsoid` and `carveBlock`), one
/// carver at a time.
struct Direct<'c, 'g> {
    generator: &'c ChunkGenerator<'g>,
    chunk: &'c mut Chunk,
    /// The chunk's carving mask: positions carved before are skipped.
    mask: &'c mut CarvingMask,
    legacy: &'c Legacy,
    /// `lava_level`, resolved.
    lava_level: i32,
    /// The Nether carver's lava: up to 31 above the generation context's bottom.
    nether_lava: i32,
    biomes: &'c ChunkBiomes<'c>,
    substances: &'c mut Substances<'g>,
    /// Whether this column of this ellipsoid has carved grass or mycelium.
    grass: bool,
}

impl Sink for Direct<'_, '_> {
    fn min_y(&self) -> i32 {
        Sink::min_y(&*self.mask)
    }

    fn max_y(&self) -> i32 {
        Sink::max_y(&*self.mask)
    }

    fn column(&mut self) {
        self.grass = false;
    }

    fn carve(&mut self, lx: i32, y: i32, lz: i32) {
        if self.mask.test_and_carve(lx, y, lz) {
            return;
        }
        let generator = self.generator;
        let settings = generator.settings;
        let palette = &settings.palette;
        let [cx, cz] = self.chunk.pos;
        let pos = [cx.wrapping_shl(4) + lx, y, cz.wrapping_shl(4) + lz];
        let id = palette.block_id(self.chunk.get(pos));
        // NetherWorldCarver.carveBlock: lava low down, cave air above, no aquifer.
        if self.legacy.nether {
            if self.legacy.replaceable.contains(id) {
                let state = if y <= self.nether_lava {
                    state::LAVA
                } else {
                    settings.cave_air
                };
                self.chunk.set(palette, pos, state);
            }
            return;
        }
        if matches!(id, "minecraft:grass_block" | "minecraft:mycelium") {
            self.grass = true;
        }
        if !self.legacy.replaceable.contains(id) {
            return;
        }
        // getCarveState: lava at and below lava_level, else the aquifer's substance.
        let substance = if y <= self.lava_level {
            state::LAVA
        } else {
            match ChunkGenerator::substance(self.substances, pos) {
                Some(s) => s,
                None => return,
            }
        };
        self.chunk.set(palette, pos, substance);
        let below = [pos[0], y - 1, pos[2]];
        if self.grass && palette.block_id(self.chunk.get(below)) == "minecraft:dirt" {
            let fluid = palette.is_fluid(substance);
            let (top, tied) = generator.material.top_material(
                self.chunk,
                self.biomes,
                Some(&generator.terrain),
                below,
                fluid,
            );
            if let Some(top) = top {
                self.chunk.set(palette, below, top);
            }
            if tied {
                self.chunk.mark_uncertain(below);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_carving_mask_spans_the_bounds_each_version_gives() {
        let heights = |min_y, height| AnchorHeights { min_y, height };
        // The Nether: noise 128 tall in a 256-block level, so the anchors' context is 0 to 127.
        let level = heights(0, 256);
        let context = AnchorHeights::within_level(heights(0, 128), level);
        assert_eq!(MaskBounds::Context.range(context, level), (1, 120));
        assert_eq!(MaskBounds::Level.range(context, level), (1, 248));
        assert_eq!(
            MaskBounds::ContextWithinLevel.range(context, level),
            (1, 120)
        );
        // Noise as tall as the level but starting higher: the context's top, 255, is past the
        // level's, 191, so only keeping it within the level tells the last two apart.
        let level = heights(-64, 256);
        let context = AnchorHeights::within_level(heights(0, 256), level);
        assert_eq!(MaskBounds::Context.range(context, level), (1, 248));
        assert_eq!(MaskBounds::Level.range(context, level), (-63, 184));
        assert_eq!(
            MaskBounds::ContextWithinLevel.range(context, level),
            (1, 184)
        );
        // The Overworld: all three agree.
        let level = heights(-64, 384);
        let context = AnchorHeights::within_level(level, level);
        for bounds in [
            MaskBounds::Context,
            MaskBounds::Level,
            MaskBounds::ContextWithinLevel,
        ] {
            assert_eq!(bounds.range(context, level), (-63, 312), "{bounds:?}");
        }
    }

    #[test]
    fn each_version_s_switches_choose_its_bounds() {
        let at = |data_version| MaskBounds::of(&Behavior::of(data_version));
        assert_eq!(at(5023), MaskBounds::Context);
        assert_eq!(at(5125), MaskBounds::Context);
        assert_eq!(at(5126), MaskBounds::Level);
        assert_eq!(at(5127), MaskBounds::Level);
        assert_eq!(at(5128), MaskBounds::ContextWithinLevel);
    }
}
