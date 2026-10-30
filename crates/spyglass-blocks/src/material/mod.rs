//! The material rule: 26.3's `MaterialSystem.buildSurface`, which gives each solid block of a
//! chunk its block from the dimension's rule, with ore veins, clay bands, eroded badlands
//! pillars and frozen-ocean icebergs; and 26.2's `SurfaceSystem.buildSurface`, the same with
//! `SurfaceRules`, in `f64` (`docs/specs/material.md`).

mod rule;

use std::cell::Cell;
use std::collections::HashMap;
use std::sync::Arc;

use spyglass_biome::{BiomeId, BiomeNames, BiomeRegion};
use spyglass_data::anchor::AnchorHeights;
use spyglass_data::codec::{At, LoadError, LoadErrorKind, field, full_id, int, object};
use spyglass_density::v1_21::{self, is_v1_21};
use spyglass_density::{Binder, Df, NOISE_SETTINGS, Resources, Sampler};
use spyglass_noise::mth::lerp2_f64;
use spyglass_noise::{NoiseStack, NormalNoiseParameters, v1_21::NormalNoise};
use spyglass_rng::{
    LegacyPositionalRandomFactory, LegacyRandomSource, PositionalRandomFactory, RandomSource,
    XoroshiroPositionalRandomFactory, XoroshiroRandomSource,
};
use spyglass_terrain::Terrain;

pub use rule::{Condition, OreVein, Rule, RuleLoader};

use crate::chunk::Chunk;
use crate::state::{Palette, State};
use crate::temperature::{Climate, Temperature};
use crate::zoom::BiomeZoom;

/// The noises `MaterialSystem` creates for itself, in the order of [`SystemNoises`]' fields.
const SYSTEM_NOISES: [&str; 9] = [
    "minecraft:clay_bands_offset",
    "minecraft:surface",
    "minecraft:surface_secondary",
    "minecraft:badlands_pillar",
    "minecraft:badlands_pillar_roof",
    "minecraft:badlands_surface",
    "minecraft:iceberg_pillar",
    "minecraft:iceberg_pillar_roof",
    "minecraft:iceberg_surface",
];

/// `DimensionType.WAY_BELOW_MIN_Y`.
const WAY_BELOW_MIN_Y: i32 = -2032 - 2032;

/// The states `MaterialSystem` places itself.
#[derive(Debug, Clone, Copy)]
struct Fixed {
    white_terracotta: State,
    orange_terracotta: State,
    terracotta: State,
    yellow_terracotta: State,
    brown_terracotta: State,
    red_terracotta: State,
    light_gray_terracotta: State,
    packed_ice: State,
    snow_block: State,
}

/// A dimension's material rule and what `MaterialSystem` needs besides, seed-independent.
#[derive(Debug, Clone)]
pub struct MaterialSettings {
    rule: Rule,
    /// `default_block`.
    default_block: State,
    sea_level: i32,
    /// The router's `chunk_surface_level`, the material system's preliminary surface; `None` for
    /// 26.2's surface rules, whose preliminary surface is the terrain's.
    chunk_surface_level: Option<Df>,
    noises: HashMap<String, NormalNoiseParameters>,
    /// `legacy_random_source`.
    legacy: bool,
    anchors: AnchorHeights,
    fixed: Fixed,
}

impl MaterialSettings {
    /// Loads `noise_settings`' material rule, default block, sea level and preliminary surface,
    /// adding the states it can place to `palette`. `anchors` are the dimension's
    /// ([`TerrainSettings::anchors`](spyglass_terrain::TerrainSettings::anchors)).
    pub fn load<R: Resources + ?Sized>(
        resources: &R,
        noise_settings: &str,
        anchors: AnchorHeights,
        palette: &mut Palette,
    ) -> Result<Self, LoadError> {
        let id = full_id(noise_settings);
        let root = At::root(&id);
        let json = resources
            .resource(NOISE_SETTINGS, &id)
            .map_err(|e| root.error(e.into()))?;
        let settings = object(&json, &root)?;
        let mut loader = RuleLoader::new(resources, palette);
        let default_block = loader.state(
            field(settings, "default_block", &root)?,
            &root.child("default_block"),
        )?;
        let sea_level = int(
            field(settings, "sea_level", &root)?,
            &root.child("sea_level"),
        )?;
        let legacy = match settings.get("legacy_random_source") {
            None => false,
            Some(v) => v.as_bool().ok_or_else(|| {
                root.child("legacy_random_source")
                    .error(LoadErrorKind::Shape("true or false"))
            })?,
        };
        // 26.2's noise settings have `surface_rule`, of the same rule and condition types
        // without ore veins, and no `chunk_surface_level`.
        let v1_21 = is_v1_21(&json);
        let key = if v1_21 {
            "surface_rule"
        } else {
            "material_rule"
        };
        let rule = loader.rule(field(settings, key, &root)?, &root.child(key))?;
        if v1_21 && rule.has_ore_vein() {
            return Err(root.child(key).error(LoadErrorKind::Unsupported {
                what: "rule type in 26.2's surface rules",
                name: "minecraft:ore_vein".to_string(),
            }));
        }
        let chunk_surface_level = if v1_21 {
            None
        } else {
            let router_at = root.child("noise_router");
            let router = object(field(settings, "noise_router", &root)?, &router_at)?;
            Some(loader.functions.inline(
                &id,
                "noise_router.chunk_surface_level",
                field(router, "chunk_surface_level", &router_at)?,
            )?)
        };
        for noise in SYSTEM_NOISES {
            loader.functions.noise(noise)?;
        }
        let noises = loader.functions.noises().clone();
        let fixed = Fixed {
            white_terracotta: palette.state("minecraft:white_terracotta"),
            orange_terracotta: palette.state("minecraft:orange_terracotta"),
            terracotta: palette.state("minecraft:terracotta"),
            yellow_terracotta: palette.state("minecraft:yellow_terracotta"),
            brown_terracotta: palette.state("minecraft:brown_terracotta"),
            red_terracotta: palette.state("minecraft:red_terracotta"),
            light_gray_terracotta: palette.state("minecraft:light_gray_terracotta"),
            packed_ice: palette.state("minecraft:packed_ice"),
            snow_block: palette.state("minecraft:snow_block"),
        };
        Ok(MaterialSettings {
            rule,
            default_block,
            sea_level,
            chunk_surface_level,
            noises,
            legacy,
            anchors,
            fixed,
        })
    }

    /// The default block's state.
    pub fn default_block(&self) -> State {
        self.default_block
    }

    /// Binds the rule for `seed`: its noises, random factories and density functions, as the
    /// seed's `RandomState` holds them. `biomes` names the biome ids the dimension's source
    /// gives, and `climates` their climates, in the same order.
    pub fn bind(
        &self,
        seed: i64,
        palette: &Palette,
        biomes: &BiomeNames,
        climates: &[Climate],
    ) -> Result<BoundMaterial, LoadError> {
        let world = if self.legacy {
            World::Legacy(LegacyRandomSource::new(seed).fork_positional())
        } else {
            World::Xoroshiro(XoroshiroRandomSource::new(seed).fork_positional())
        };
        let bands = bands(&mut world.named("minecraft:clay_bands"), &self.fixed);
        let (rule, family, temperature) = match &self.chunk_surface_level {
            Some(chunk_surface_level) => {
                let mut binder = Binder::with_legacy_random(&self.noises, seed, self.legacy)
                    .with_anchors(self.anchors);
                let noises = SystemNoises::bind(|n| binder.noise_stack(n))?;
                let rule = Compiler {
                    binder: Binders::Material(&mut binder),
                    world: &world,
                    biomes,
                    anchors: self.anchors,
                }
                .rule(&self.rule)?;
                let chunk_surface_level = binder.bind(chunk_surface_level)?;
                let family = Family::Material {
                    noises,
                    chunk_surface_level,
                };
                (rule, family, Temperature::new())
            }
            None => {
                let mut binder = v1_21::Binder::new(&self.noises, seed, self.legacy);
                let noises = SystemNoises::bind(|n| binder.normal_noise(n))?;
                let rule = Compiler {
                    binder: Binders::V1_21(&mut binder),
                    world: &world,
                    biomes,
                    anchors: self.anchors,
                }
                .rule(&self.rule)?;
                let family = Family::V1_21 {
                    noises,
                    start_at_y0: self.legacy,
                };
                (rule, family, Temperature::v1_21())
            }
        };
        let _ = palette;
        Ok(BoundMaterial {
            rule,
            family,
            world,
            bands,
            default_block: self.default_block,
            sea_level: self.sea_level,
            zoom: BiomeZoom::new(seed),
            temperature,
            climates: climates.to_vec(),
            eroded_badlands: biomes.id("minecraft:eroded_badlands"),
            frozen_oceans: [
                biomes.id("minecraft:frozen_ocean"),
                biomes.id("minecraft:deep_frozen_ocean"),
            ],
            fixed: self.fixed,
        })
    }
}

/// `MaterialSystem`'s own noises, bound to a seed: 26.3's stacks or 26.2's normal noises.
#[derive(Debug, Clone)]
struct SystemNoises<N> {
    clay_bands_offset: N,
    surface: N,
    surface_secondary: N,
    badlands_pillar: N,
    badlands_pillar_roof: N,
    badlands_surface: N,
    iceberg_pillar: N,
    iceberg_pillar_roof: N,
    iceberg_surface: N,
}

impl<N> SystemNoises<N> {
    /// Each of [`SYSTEM_NOISES`] from `noise`, in order (`RandomState.getOrCreateNoise`).
    fn bind(mut noise: impl FnMut(&str) -> Result<N, LoadError>) -> Result<Self, LoadError> {
        let [clay, surface, secondary, bp, bpr, bs, ip, ipr, is] = SYSTEM_NOISES.map(&mut noise);
        Ok(SystemNoises {
            clay_bands_offset: clay?,
            surface: surface?,
            surface_secondary: secondary?,
            badlands_pillar: bp?,
            badlands_pillar_roof: bpr?,
            badlands_surface: bs?,
            iceberg_pillar: ip?,
            iceberg_pillar_roof: ipr?,
            iceberg_surface: is?,
        })
    }
}

/// What differs between the families (ADR 0026), bound to a seed.
#[derive(Debug)]
enum Family {
    /// 26.3 on: `MaterialSystem`, whose noises give `f32`.
    Material {
        noises: SystemNoises<Arc<NoiseStack>>,
        /// The router's `chunk_surface_level`.
        chunk_surface_level: Sampler,
    },
    /// 26.2: `SurfaceSystem`, in `f64`, with the terrain's preliminary surface.
    V1_21 {
        noises: SystemNoises<Arc<NormalNoise>>,
        /// `legacy_random_source`: the start biome is the one at y 0.
        start_at_y0: bool,
    },
}

/// A noise a condition samples, widened to `f64`.
#[derive(Debug)]
enum BoundNoise {
    Stack(Arc<NoiseStack>),
    Normal(Arc<NormalNoise>),
}

impl BoundNoise {
    fn get(&self, x: f64, y: f64, z: f64) -> f64 {
        match self {
            BoundNoise::Stack(n) => f64::from(n.get(x, y, z)),
            BoundNoise::Normal(n) => n.get_value(x, y, z),
        }
    }
}

impl Family {
    /// `getSurfaceDepth`'s noise.
    fn surface(&self, x: i32, z: i32) -> f64 {
        let (x, z) = (f64::from(x), f64::from(z));
        match self {
            Family::Material { noises, .. } => f64::from(noises.surface.get(x, 0.0, z)),
            Family::V1_21 { noises, .. } => noises.surface.get_value(x, 0.0, z),
        }
    }

    /// `getSurfaceSecondary`.
    fn surface_secondary(&self, x: i32, z: i32) -> f64 {
        let (x, z) = (f64::from(x), f64::from(z));
        match self {
            Family::Material { noises, .. } => f64::from(noises.surface_secondary.get(x, 0.0, z)),
            Family::V1_21 { noises, .. } => noises.surface_secondary.get_value(x, 0.0, z),
        }
    }

    /// `getBand`'s offset: `Math.round` of the offset noise times 4, in `f32` from 26.3, in
    /// `f64` (then narrowed from `long`) in 26.2.
    fn band_offset(&self, x: i32, z: i32) -> i32 {
        let (x, z) = (f64::from(x), f64::from(z));
        match self {
            Family::Material { noises, .. } => {
                round_f32(noises.clay_bands_offset.get(x, 0.0, z) * 4.0)
            }
            Family::V1_21 { noises, .. } => {
                round_f64(noises.clay_bands_offset.get_value(x, 0.0, z) * 4.0) as i32
            }
        }
    }

    /// The pillar noise of the badlands or iceberg extension: `min(|surface · 8.25|,
    /// pillar(x · f, 0, z · f) · 15)`, with the product `f32` from 26.3, and the roof noise,
    /// `|roof(x · r, 0, z · r) · 1.5|`, sampled only if `keep` holds for the pillar.
    fn pillar(
        &self,
        iceberg: bool,
        x: i32,
        z: i32,
        keep: impl FnOnce(f64) -> bool,
    ) -> Option<(f64, f64)> {
        let (f, r) = if iceberg { (1.28, 1.17) } else { (0.2, 0.75) };
        let (x, z) = (f64::from(x), f64::from(z));
        let (pillar, roof) = match self {
            Family::Material { noises, .. } => {
                let n = noises;
                let (s, p, roof) = if iceberg {
                    (
                        &n.iceberg_surface,
                        &n.iceberg_pillar,
                        &n.iceberg_pillar_roof,
                    )
                } else {
                    (
                        &n.badlands_surface,
                        &n.badlands_pillar,
                        &n.badlands_pillar_roof,
                    )
                };
                let surface = (f64::from(s.get(x, 0.0, z)) * 8.25).abs();
                let pillar = surface.min(f64::from(p.get(x * f, 0.0, z * f) * 15.0));
                if !keep(pillar) {
                    return None;
                }
                (pillar, (f64::from(roof.get(x * r, 0.0, z * r)) * 1.5).abs())
            }
            Family::V1_21 { noises, .. } => {
                let n = noises;
                let (s, p, roof) = if iceberg {
                    (
                        &n.iceberg_surface,
                        &n.iceberg_pillar,
                        &n.iceberg_pillar_roof,
                    )
                } else {
                    (
                        &n.badlands_surface,
                        &n.badlands_pillar,
                        &n.badlands_pillar_roof,
                    )
                };
                let surface = (s.get_value(x, 0.0, z) * 8.25).abs();
                let pillar = surface.min(p.get_value(x * f, 0.0, z * f) * 15.0);
                if !keep(pillar) {
                    return None;
                }
                (pillar, (roof.get_value(x * r, 0.0, z * r) * 1.5).abs())
            }
        };
        Some((pillar, roof))
    }
}

/// The seed's positional random factory, of the kind `legacy_random_source` chooses.
#[derive(Debug, Clone)]
enum World {
    Xoroshiro(XoroshiroPositionalRandomFactory),
    Legacy(LegacyPositionalRandomFactory),
}

/// A random source of either kind.
enum Source {
    Xoroshiro(XoroshiroRandomSource),
    Legacy(LegacyRandomSource),
}

impl World {
    fn at(&self, x: i32, y: i32, z: i32) -> Source {
        match self {
            World::Xoroshiro(f) => Source::Xoroshiro(f.at(x, y, z)),
            World::Legacy(f) => Source::Legacy(f.at(x, y, z)),
        }
    }

    /// `fromHashOf(name)`.
    fn named(&self, name: &str) -> Source {
        match self {
            World::Xoroshiro(f) => Source::Xoroshiro(f.from_hash_of(name)),
            World::Legacy(f) => Source::Legacy(f.from_hash_of(name)),
        }
    }

    /// `RandomState.getOrCreateRandomFactory(name)`: `fromHashOf(name).forkPositional()`.
    fn factory(&self, name: &str) -> World {
        match self.named(name) {
            Source::Xoroshiro(mut s) => World::Xoroshiro(s.fork_positional()),
            Source::Legacy(mut s) => World::Legacy(s.fork_positional()),
        }
    }
}

impl Source {
    fn next_float(&mut self) -> f32 {
        match self {
            Source::Xoroshiro(s) => s.next_float(),
            Source::Legacy(s) => s.next_float(),
        }
    }

    fn next_double(&mut self) -> f64 {
        match self {
            Source::Xoroshiro(s) => s.next_double(),
            Source::Legacy(s) => s.next_double(),
        }
    }

    fn next_int(&mut self, bound: i32) -> i32 {
        match self {
            Source::Xoroshiro(s) => s.next_int_bounded(bound),
            Source::Legacy(s) => s.next_int_bounded(bound),
        }
    }

    /// `nextIntBetweenInclusive(min, max)`: `nextInt(max − min + 1) + min`.
    fn next_int_between_inclusive(&mut self, min: i32, max: i32) -> i32 {
        self.next_int(max - min + 1) + min
    }

    fn next_boolean(&mut self) -> bool {
        match self {
            Source::Xoroshiro(s) => s.next_boolean(),
            Source::Legacy(s) => s.next_boolean(),
        }
    }
}

/// `MaterialSystem.generateBands`: the 192 clay bands of badlands, from `fromHashOf("clay_bands")`.
fn bands(random: &mut Source, fixed: &Fixed) -> Vec<State> {
    const LEN: usize = 192;
    let mut bands = vec![fixed.terracotta; LEN];
    let mut i = 0;
    while i < LEN {
        i += random.next_int(5) as usize + 1;
        if i < LEN {
            bands[i] = fixed.orange_terracotta;
        }
        i += 1;
    }
    make_bands(random, &mut bands, 1, fixed.yellow_terracotta);
    make_bands(random, &mut bands, 2, fixed.brown_terracotta);
    make_bands(random, &mut bands, 1, fixed.red_terracotta);
    let count = random.next_int_between_inclusive(9, 15);
    let mut placed = 0;
    let mut i = 0;
    while placed < count && i < LEN {
        bands[i] = fixed.white_terracotta;
        if i > 1 && random.next_boolean() {
            bands[i - 1] = fixed.light_gray_terracotta;
        }
        if i + 1 < LEN && random.next_boolean() {
            bands[i + 1] = fixed.light_gray_terracotta;
        }
        placed += 1;
        i += random.next_int(16) as usize + 4;
    }
    bands
}

/// `MaterialSystem.makeBands`: 6 to 15 runs of `state`, each `base` to `base + 2` long.
fn make_bands(random: &mut Source, bands: &mut [State], base: i32, state: State) {
    let runs = random.next_int_between_inclusive(6, 15);
    for _ in 0..runs {
        let len = (base + random.next_int(3)) as usize;
        let start = random.next_int(bands.len() as i32) as usize;
        for j in 0..len {
            if start + j >= bands.len() {
                break;
            }
            bands[start + j] = state;
        }
    }
}

/// A rule with its noises, random factories and functions bound.
#[derive(Debug)]
enum Bound {
    Block(State),
    Sequence(Vec<Bound>),
    Condition(Box<BoundCondition>, Box<Bound>),
    Bandlands,
    OreVein(Box<BoundVein>),
}

#[derive(Debug)]
struct BoundVein {
    ore: State,
    raw_ore: State,
    filler: State,
    raw_ore_chance: f32,
    density: Sampler,
    richness: Sampler,
    filler_gap: Sampler,
    random: World,
}

#[derive(Debug)]
enum BoundCondition {
    /// By biome id: whether it's in the set.
    Biome(Vec<bool>),
    Noise {
        noise: BoundNoise,
        min: f64,
        max: f64,
        is_3d: bool,
    },
    VerticalGradient {
        random: World,
        true_at_and_below: i32,
        false_at_and_above: i32,
    },
    YAbove {
        y: i32,
        surface_depth_multiplier: i32,
        add_stone_depth: bool,
    },
    Water {
        offset: i32,
        surface_depth_multiplier: i32,
        add_stone_depth: bool,
    },
    Temperature,
    Steep,
    Hole,
    AbovePreliminarySurface,
    Not(Box<BoundCondition>),
    StoneDepth {
        offset: i32,
        add_surface_depth: bool,
        secondary_depth_range: i32,
        ceiling: bool,
    },
}

/// The binder of the family's functions and noises.
enum Binders<'c, 'n> {
    Material(&'c mut Binder<'n>),
    V1_21(&'c mut v1_21::Binder<'n>),
}

/// Binds rules for one seed (`MaterialRule.compile`, or 26.2's `RuleSource.apply`).
struct Compiler<'c, 'n> {
    binder: Binders<'c, 'n>,
    world: &'c World,
    biomes: &'c BiomeNames,
    anchors: AnchorHeights,
}

impl Compiler<'_, '_> {
    fn rule(&mut self, rule: &Rule) -> Result<Bound, LoadError> {
        Ok(match rule {
            Rule::Block(s) => Bound::Block(*s),
            Rule::Sequence(rules) => Bound::Sequence(
                rules
                    .iter()
                    .map(|r| self.rule(r))
                    .collect::<Result<_, _>>()?,
            ),
            Rule::Condition(c, r) => {
                Bound::Condition(Box::new(self.condition(c)?), Box::new(self.rule(r)?))
            }
            Rule::Bandlands => Bound::Bandlands,
            Rule::OreVein(v) => {
                // 26.2's surface rules have no ore veins: they're rejected when loaded.
                let Binders::Material(binder) = &mut self.binder else {
                    unreachable!("ore veins only load from 26.3 on");
                };
                Bound::OreVein(Box::new(BoundVein {
                    ore: v.ore_block,
                    raw_ore: v.raw_ore_block,
                    filler: v.filler_block,
                    raw_ore_chance: v.raw_ore_chance,
                    density: binder.bind(&v.density)?,
                    richness: binder.bind(&v.richness)?,
                    filler_gap: binder.bind(&v.filler_gap)?,
                    random: self.world.factory("minecraft:ore"),
                }))
            }
        })
    }

    fn condition(&mut self, condition: &Condition) -> Result<BoundCondition, LoadError> {
        Ok(match condition {
            Condition::Biome(ids) => {
                let mut set = vec![false; self.biomes.names().len()];
                for id in ids {
                    if let Some(b) = self.biomes.id(id) {
                        set[usize::from(b.0)] = true;
                    }
                }
                BoundCondition::Biome(set)
            }
            Condition::NoiseThreshold {
                noise,
                min,
                max,
                is_3d,
            } => BoundCondition::Noise {
                noise: match &mut self.binder {
                    Binders::Material(b) => BoundNoise::Stack(b.noise_stack(noise)?),
                    Binders::V1_21(b) => BoundNoise::Normal(b.normal_noise(noise)?),
                },
                min: *min,
                max: *max,
                is_3d: *is_3d,
            },
            Condition::VerticalGradient {
                random_name,
                true_at_and_below,
                false_at_and_above,
            } => BoundCondition::VerticalGradient {
                random: self.world.factory(random_name),
                true_at_and_below: true_at_and_below.resolve(self.anchors),
                false_at_and_above: false_at_and_above.resolve(self.anchors),
            },
            Condition::YAbove {
                anchor,
                surface_depth_multiplier,
                add_stone_depth,
            } => BoundCondition::YAbove {
                y: anchor.resolve(self.anchors),
                surface_depth_multiplier: *surface_depth_multiplier,
                add_stone_depth: *add_stone_depth,
            },
            Condition::Water {
                offset,
                surface_depth_multiplier,
                add_stone_depth,
            } => BoundCondition::Water {
                offset: *offset,
                surface_depth_multiplier: *surface_depth_multiplier,
                add_stone_depth: *add_stone_depth,
            },
            Condition::Temperature => BoundCondition::Temperature,
            Condition::Steep => BoundCondition::Steep,
            Condition::Hole => BoundCondition::Hole,
            Condition::AbovePreliminarySurface => BoundCondition::AbovePreliminarySurface,
            Condition::Not(c) => BoundCondition::Not(Box::new(self.condition(c)?)),
            Condition::StoneDepth {
                offset,
                add_surface_depth,
                secondary_depth_range,
                ceiling,
            } => BoundCondition::StoneDepth {
                offset: *offset,
                add_surface_depth: *add_surface_depth,
                secondary_depth_range: *secondary_depth_range,
                ceiling: *ceiling,
            },
        })
    }
}

/// A dimension's material rule bound to a seed: `RandomState.surfaceSystem()` with the rule
/// compiled.
#[derive(Debug)]
pub struct BoundMaterial {
    rule: Bound,
    family: Family,
    world: World,
    bands: Vec<State>,
    default_block: State,
    sea_level: i32,
    zoom: BiomeZoom,
    temperature: Temperature,
    climates: Vec<Climate>,
    eroded_badlands: Option<BiomeId>,
    frozen_oceans: [Option<BiomeId>; 2],
    fixed: Fixed,
}

/// The biomes around a chunk, as a `WorldGenRegion` hands them to the biome manager: quart
/// biomes, with quart y clamped to the level's (`ChunkAccess.getNoiseBiome`).
pub struct ChunkBiomes<'r> {
    region: &'r BiomeRegion,
    min_qy: i32,
    max_qy: i32,
}

impl<'r> ChunkBiomes<'r> {
    /// The biomes of `region`, which must cover a chunk's quarts and one more on every
    /// horizontal side, over a level from `min_y` of `height` blocks.
    pub fn new(region: &'r BiomeRegion, min_y: i32, height: i32) -> Self {
        let min_qy = min_y >> 2;
        ChunkBiomes {
            region,
            min_qy,
            max_qy: min_qy + (height >> 2) - 1,
        }
    }

    /// The biome of quart `(qx, qy, qz)`, and whether it's tied.
    fn quart(&self, [qx, qy, qz]: [i32; 3]) -> (BiomeId, bool) {
        let qy = qy.clamp(self.min_qy, self.max_qy);
        let lookup = self
            .region
            .get(qx, qy, qz)
            .expect("the region covers the chunk's biome lookups");
        (lookup.biome(), lookup.is_tied())
    }

    /// The candidates of quart `(qx, qy, qz)`.
    fn candidates(&self, [qx, qy, qz]: [i32; 3]) -> Vec<BiomeId> {
        let qy = qy.clamp(self.min_qy, self.max_qy);
        self.region
            .get(qx, qy, qz)
            .expect("the region covers the chunk's biome lookups")
            .candidates()
            .to_vec()
    }
}

/// What the rule is evaluated at: the column, then the block (`MaterialRuleContext`).
struct Context<'a> {
    system: &'a BoundMaterial,
    biomes: &'a ChunkBiomes<'a>,
    /// For 26.2's preliminary surface.
    terrain: &'a Terrain,
    /// 26.2's `preliminarySurfaceCache`: the preliminary surface at the corners of the chunk's
    /// surface cell, once read.
    cell: &'a Cell<Option<[i32; 4]>>,
    x: i32,
    z: i32,
    /// The surface gradients in x and z, as steep reads them.
    gradient: Cell<[i32; 2]>,
    /// Whether [`gradient`](Self::gradient) is fixed for the column.
    gradient_read: Cell<bool>,
    surface_depth: i32,
    surface_secondary: Cell<Option<f64>>,
    min_surface_level: Cell<Option<i32>>,
    y: i32,
    stone_depth_above: i32,
    stone_depth_below: i32,
    water_height: i32,
    biome: Cell<Option<BiomeId>>,
    /// Whether the block's biome was looked up and is tied.
    tied: Cell<bool>,
}

impl Context<'_> {
    fn biome(&self) -> BiomeId {
        if let Some(b) = self.biome.get() {
            return b;
        }
        let (b, tied) = self.system.biome_at(self.biomes, [self.x, self.y, self.z]);
        self.biome.set(Some(b));
        self.tied.set(tied);
        b
    }

    fn surface_secondary(&self) -> f64 {
        if let Some(v) = self.surface_secondary.get() {
            return v;
        }
        let v = self.system.family.surface_secondary(self.x, self.z);
        self.surface_secondary.set(Some(v));
        v
    }

    /// `getMinSurfaceLevel`: the preliminary surface, floored, plus the surface depth, minus 8.
    fn min_surface_level(&self) -> i32 {
        if let Some(v) = self.min_surface_level.get() {
            return v;
        }
        let level = match &self.system.family {
            Family::Material {
                chunk_surface_level,
                ..
            } => floor_f32(chunk_surface_level.sample(self.x, 0, self.z)),
            // The terrain's preliminary surface at the four corners of the 16-block cell,
            // interpolated with `Mth.lerp2`.
            Family::V1_21 { .. } => {
                let corners = self.cell.get().unwrap_or_else(|| {
                    let (i, j) = (self.x >> 4, self.z >> 4);
                    let at = |di: i32, dj: i32| {
                        self.terrain
                            .preliminary_surface_level(
                                i.wrapping_add(di).wrapping_shl(4),
                                j.wrapping_add(dj).wrapping_shl(4),
                            )
                            .expect("26.2's surface rules come with 26.2's terrain")
                    };
                    let corners = [at(0, 0), at(1, 0), at(0, 1), at(1, 1)];
                    self.cell.set(Some(corners));
                    corners
                });
                let [c0, c1, c2, c3] = corners.map(f64::from);
                let fx = f64::from((self.x & 15) as f32 / 16.0);
                let fz = f64::from((self.z & 15) as f32 / 16.0);
                floor_f64(lerp2_f64(fx, fz, c0, c1, c2, c3))
            }
        };
        let v = level.wrapping_add(self.surface_depth).wrapping_sub(8);
        self.min_surface_level.set(Some(v));
        v
    }
}

/// `Mth.floor(float)`.
fn floor_f32(v: f32) -> i32 {
    let i = v as i32;
    if v < i as f32 { i - 1 } else { i }
}

/// `Mth.map(v, a, b, c, d)`: `lerp(inverseLerp(v, a, b), c, d)`.
fn map(v: f64, a: f64, b: f64, c: f64, d: f64) -> f64 {
    let t = (v - a) / (b - a);
    c + t * (d - c)
}

/// `Math.round(double)`: the nearest integer, halves up, NaN 0, saturating to `long`.
fn round_f64(v: f64) -> i64 {
    if v.is_nan() {
        return 0;
    }
    let f = v.floor();
    let r = if v - f >= 0.5 { f + 1.0 } else { f };
    r as i64
}

/// `Math.round(float)`: the nearest integer, halves up, NaN 0, saturating.
fn round_f32(v: f32) -> i32 {
    if v.is_nan() {
        return 0;
    }
    let f = v.floor();
    let r = if v - f >= 0.5 { f + 1.0 } else { f };
    r as i32
}

impl BoundMaterial {
    /// The quart block `pos` takes its biome from (`BiomeManager.getBiome`'s zoom).
    pub fn biome_quart(&self, pos: [i32; 3]) -> [i32; 3] {
        self.zoom.quart(pos)
    }

    /// `BiomeManager.getBiome`: the biome block `pos` takes, through the zoom, and whether its
    /// quart is tied.
    fn biome_at(&self, biomes: &ChunkBiomes<'_>, pos: [i32; 3]) -> (BiomeId, bool) {
        biomes.quart(self.zoom.quart(pos))
    }

    /// `getSurfaceDepth`.
    fn surface_depth(&self, x: i32, z: i32) -> i32 {
        let noise = self.family.surface(x, z);
        let random = self.world.at(x, 0, z).next_double();
        (noise * 2.75 + 3.0 + random * 0.25) as i32
    }

    /// `getBand`.
    fn band(&self, x: i32, y: i32, z: i32) -> State {
        let offset = self.family.band_offset(x, z);
        let len = self.bands.len() as i32;
        self.bands[y.wrapping_add(offset).wrapping_add(len).rem_euclid(len) as usize]
    }

    fn test(&self, condition: &BoundCondition, c: &Context<'_>) -> bool {
        match condition {
            BoundCondition::Biome(set) => {
                set.get(usize::from(c.biome().0)).copied().unwrap_or(false)
            }
            BoundCondition::Noise {
                noise,
                min,
                max,
                is_3d,
            } => {
                let y = if *is_3d { f64::from(c.y) } else { 0.0 };
                let v = noise.get(f64::from(c.x), y, f64::from(c.z));
                v >= *min && v <= *max
            }
            BoundCondition::VerticalGradient {
                random,
                true_at_and_below,
                false_at_and_above,
            } => {
                if c.y <= *true_at_and_below {
                    return true;
                }
                if c.y >= *false_at_and_above {
                    return false;
                }
                let chance = map(
                    f64::from(c.y),
                    f64::from(*true_at_and_below),
                    f64::from(*false_at_and_above),
                    1.0,
                    0.0,
                );
                f64::from(random.at(c.x, c.y, c.z).next_float()) < chance
            }
            BoundCondition::YAbove {
                y,
                surface_depth_multiplier,
                add_stone_depth,
            } => {
                let above = if *add_stone_depth {
                    c.stone_depth_above
                } else {
                    0
                };
                c.y.wrapping_add(above)
                    >= y.wrapping_add(c.surface_depth.wrapping_mul(*surface_depth_multiplier))
            }
            BoundCondition::Water {
                offset,
                surface_depth_multiplier,
                add_stone_depth,
            } => {
                if c.water_height == i32::MIN {
                    return true;
                }
                let above = if *add_stone_depth {
                    c.stone_depth_above
                } else {
                    0
                };
                c.y.wrapping_add(above)
                    >= c.water_height
                        .wrapping_add(*offset)
                        .wrapping_add(c.surface_depth.wrapping_mul(*surface_depth_multiplier))
            }
            BoundCondition::Temperature => {
                let climate = self.climates[usize::from(c.biome().0)];
                self.temperature
                    .cold_enough_to_snow(climate, [c.x, c.y, c.z], self.sea_level)
            }
            BoundCondition::Steep => {
                c.gradient_read.set(true);
                let [gx, gz] = c.gradient.get();
                gx <= -4 || gz >= 4
            }
            BoundCondition::Hole => c.surface_depth <= 0,
            BoundCondition::AbovePreliminarySurface => c.y >= c.min_surface_level(),
            BoundCondition::Not(inner) => !self.test(inner, c),
            BoundCondition::StoneDepth {
                offset,
                add_surface_depth,
                secondary_depth_range,
                ceiling,
            } => {
                let depth = if *ceiling {
                    c.stone_depth_below
                } else {
                    c.stone_depth_above
                };
                let surface = if *add_surface_depth {
                    c.surface_depth
                } else {
                    0
                };
                let secondary = if *secondary_depth_range == 0 {
                    0
                } else {
                    map(
                        c.surface_secondary(),
                        -1.0,
                        1.0,
                        0.0,
                        f64::from(*secondary_depth_range),
                    ) as i32
                };
                depth
                    <= 1i32
                        .wrapping_add(*offset)
                        .wrapping_add(surface)
                        .wrapping_add(secondary)
            }
        }
    }

    fn apply(&self, rule: &Bound, c: &Context<'_>) -> Option<State> {
        match rule {
            Bound::Block(s) => Some(*s),
            Bound::Sequence(rules) => rules.iter().find_map(|r| self.apply(r, c)),
            Bound::Condition(condition, then) => {
                if self.test(condition, c) {
                    self.apply(then, c)
                } else {
                    None
                }
            }
            Bound::Bandlands => Some(self.band(c.x, c.y, c.z)),
            Bound::OreVein(v) => {
                let density = v.density.sample(c.x, c.y, c.z);
                // fcmpg/ifgt: NaN goes on.
                if density <= 0.0 {
                    return None;
                }
                let mut random = v.random.at(c.x, c.y, c.z);
                // fcmpl/ifle: NaN goes on.
                if random.next_float() > density {
                    return None;
                }
                let richness = v.richness.sample(c.x, c.y, c.z);
                if random.next_float() < richness && v.filler_gap.sample(c.x, c.y, c.z) < 0.0 {
                    return Some(if random.next_float() < v.raw_ore_chance {
                        v.raw_ore
                    } else {
                        v.ore
                    });
                }
                Some(v.filler)
            }
        }
    }

    /// `MaterialSystem.buildSurface` (26.2's `SurfaceSystem.buildSurface`) on `chunk`, filled
    /// from noise: each column top down, with eroded badlands pillars first and frozen-ocean
    /// icebergs last. `terrain` is the seed's, for 26.2's preliminary surface.
    pub fn build_surface(
        &self,
        chunk: &mut Chunk,
        palette: &Palette,
        biomes: &ChunkBiomes<'_>,
        terrain: &Terrain,
    ) {
        let cell = Cell::new(None);
        let (start_at_y0, v1_21) = match self.family {
            Family::Material { .. } => (false, false),
            Family::V1_21 { start_at_y0, .. } => (start_at_y0, true),
        };
        let [cx, cz] = chunk.pos;
        let (x0, z0) = (cx.wrapping_shl(4), cz.wrapping_shl(4));
        let min_y = chunk.min_y;
        let max_y = chunk.min_y + chunk.height - 1;
        let is_stone = |s: State| !palette.is_air(s) && !palette.is_fluid(s);
        for dx in 0..16 {
            for dz in 0..16 {
                let (x, z) = (x0 + dx, z0 + dz);
                let start = chunk.height_at(dx, dz) + 1;
                // 26.2 with `legacy_random_source` reads the start biome at y 0.
                let start_pos = [x, if start_at_y0 { 0 } else { start }, z];
                let (biome, start_tied) = self.biome_at(biomes, start_pos);
                // A tied start biome may decide a pillar or an iceberg either way: then the
                // whole column is uncertain.
                if start_tied {
                    let candidates = biomes.candidates(self.zoom.quart(start_pos));
                    let extended = |b: &BiomeId| {
                        Some(*b) == self.eroded_badlands || self.frozen_oceans.contains(&Some(*b))
                    };
                    if candidates.iter().any(extended) {
                        for y in min_y..=max_y {
                            chunk.mark_uncertain([x, y, z]);
                        }
                    }
                }
                if Some(biome) == self.eroded_badlands {
                    self.eroded_badlands_extension(chunk, palette, x, z, start);
                }
                let top = chunk.height_at(dx, dz) + 1;
                let gradient = |chunk: &Chunk| {
                    [
                        chunk.height_at((dx + 1).min(15), dz)
                            - chunk.height_at((dx - 1).max(0), dz),
                        chunk.height_at(dx, (dz + 1).min(15))
                            - chunk.height_at(dx, (dz - 1).max(0)),
                    ]
                };
                let mut c = Context {
                    system: self,
                    biomes,
                    terrain,
                    cell: &cell,
                    x,
                    z,
                    gradient: Cell::new(gradient(chunk)),
                    // From 26.3, the gradients are read here; 26.2 reads them when steep is
                    // first tested in the column.
                    gradient_read: Cell::new(!v1_21),
                    surface_depth: self.surface_depth(x, z),
                    surface_secondary: Cell::new(None),
                    min_surface_level: Cell::new(None),
                    y: 0,
                    stone_depth_above: 0,
                    stone_depth_below: 0,
                    water_height: i32::MIN,
                    biome: Cell::new(None),
                    tied: Cell::new(false),
                };
                let mut stone_depth_above = 0;
                let mut water_height = i32::MIN;
                let mut next_ceiling_stone_y = i32::MAX;
                let mut y = top;
                while y >= min_y {
                    let state = chunk.get([x, y, z]);
                    if palette.is_air(state) {
                        stone_depth_above = 0;
                        water_height = i32::MIN;
                    } else if palette.is_fluid(state) {
                        if water_height == i32::MIN {
                            water_height = y + 1;
                        }
                    } else {
                        if next_ceiling_stone_y >= y {
                            next_ceiling_stone_y = WAY_BELOW_MIN_Y;
                            let mut below = y - 1;
                            while below >= min_y - 1 {
                                if !is_stone(chunk.get([x, below, z])) {
                                    next_ceiling_stone_y = below + 1;
                                    break;
                                }
                                below -= 1;
                            }
                        }
                        stone_depth_above += 1;
                        c.y = y;
                        c.stone_depth_above = stone_depth_above;
                        c.stone_depth_below = y - next_ceiling_stone_y + 1;
                        c.water_height = water_height;
                        c.biome.set(None);
                        c.tied.set(false);
                        // 26.2 tries the rule only on the default block itself, so the noise
                        // fill's ore veins stay; from 26.3, on every stone within the level.
                        let tried = if v1_21 {
                            state == self.default_block
                        } else {
                            (min_y..=max_y).contains(&y)
                        };
                        if tried {
                            if !c.gradient_read.get() {
                                c.gradient.set(gradient(chunk));
                            }
                            if let Some(s) = self.apply(&self.rule, &c) {
                                chunk.set(palette, [x, y, z], s);
                            }
                            if c.tied.get() {
                                chunk.mark_uncertain([x, y, z]);
                            }
                        }
                    }
                    y -= 1;
                }
                if self.frozen_oceans.contains(&Some(biome)) {
                    let min_surface = c.min_surface_level();
                    self.frozen_ocean_extension(chunk, palette, min_surface, biome, x, z, start);
                }
            }
        }
    }

    /// `erodedBadlandsExtension`: a pillar of the default block over the surface where the
    /// pillar noises are high, unless water is in the way.
    fn eroded_badlands_extension(
        &self,
        chunk: &mut Chunk,
        palette: &Palette,
        x: i32,
        z: i32,
        start: i32,
    ) {
        // dcmpg/ifgt: NaN goes on.
        let Some((pillar, roof)) = self.family.pillar(false, x, z, |p| p > 0.0 || p.is_nan())
        else {
            return;
        };
        let top = 64.0 + (pillar * pillar * 2.5).min((roof * 50.0).ceil() + 24.0);
        let top = floor_f64(top);
        if start > top {
            return;
        }
        let default = palette.block_id(self.default_block);
        let mut y = top;
        while y >= chunk.min_y {
            let state = chunk.get([x, y, z]);
            if palette.block_id(state) == default {
                break;
            }
            if palette.block_id(state) == "minecraft:water" {
                return;
            }
            y -= 1;
        }
        let mut y = top;
        while y >= chunk.min_y && palette.is_air(chunk.get([x, y, z])) {
            chunk.set(palette, [x, y, z], self.default_block);
            y -= 1;
        }
    }

    /// `frozenOceanExtension`: an iceberg of packed ice, capped with snow, over and under the
    /// sea where the iceberg noises are high.
    #[allow(clippy::too_many_arguments)]
    fn frozen_ocean_extension(
        &self,
        chunk: &mut Chunk,
        palette: &Palette,
        min_surface_level: i32,
        biome: BiomeId,
        x: i32,
        z: i32,
        start: i32,
    ) {
        // dcmpg/ifgt: NaN goes on.
        let Some((pillar, roof)) = self.family.pillar(true, x, z, |p| p > 1.8 || p.is_nan()) else {
            return;
        };
        let mut top = (pillar * pillar * 1.2).min((roof * 40.0).ceil() + 14.0);
        let climate = self.climates[usize::from(biome.0)];
        if self
            .temperature
            .melts_icebergs(climate, [x, self.sea_level, z], self.sea_level)
        {
            top -= 2.0;
        }
        let sea = f64::from(self.sea_level);
        let v1_21 = matches!(self.family, Family::V1_21 { .. });
        // From 26.3, NaN makes an iceberg; 26.2's dcmpl/ifle sends it to the low branch.
        let high = if v1_21 {
            top > 2.0
        } else {
            top > 2.0 || top.is_nan()
        };
        let (top, bottom) = if high {
            (top + sea, sea - top - 7.0)
        } else if v1_21 {
            // 26.2 goes on with both at 0: only air below y 0 can become ice.
            (0.0, 0.0)
        } else {
            return;
        };
        let mut random = self.world.at(x, 0, z);
        let max_snow = 2 + random.next_int(4);
        let min_snow_y = self.sea_level + 18 + random.next_int(10);
        let mut snow = 0;
        let mut y = start.max(top as i32 + 1);
        while y >= min_surface_level {
            let state = chunk.get([x, y, z]);
            // 26.2 places nothing over water when the iceberg's bottom is 0.
            let place = (palette.is_air(state) && y < top as i32 && random.next_double() > 0.01)
                || (palette.block_id(state) == "minecraft:water"
                    && y > bottom as i32
                    && y < self.sea_level
                    && (!v1_21 || bottom != 0.0)
                    && random.next_double() > 0.15);
            if place {
                if snow <= max_snow && y > min_snow_y {
                    chunk.set(palette, [x, y, z], self.fixed.snow_block);
                    snow += 1;
                } else {
                    chunk.set(palette, [x, y, z], self.fixed.packed_ice);
                }
            }
            y -= 1;
        }
    }
}

/// `Mth.floor(double)`.
fn floor_f64(v: f64) -> i32 {
    let i = v as i32;
    if v < f64::from(i) { i - 1 } else { i }
}
