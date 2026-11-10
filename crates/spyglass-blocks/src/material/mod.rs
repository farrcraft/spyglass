//! The material rule: 26.3's `MaterialSystem.buildSurface`, which gives each solid block of a
//! chunk its block from the dimension's rule, with ore veins, clay bands, eroded badlands
//! pillars and frozen-ocean icebergs; and 26.2's `SurfaceSystem.buildSurface`, the same with
//! `SurfaceRules`, in `f64` (`docs/specs/material.md`).

mod rule;

use std::cell::{Cell, RefCell};
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

use crate::carver::CarvingMask;
use crate::chunk::Chunk;
use crate::state;
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
    dirt: State,
}

/// A dimension's material rule and what `MaterialSystem` needs besides, seed-independent.
#[derive(Debug, Clone)]
pub struct MaterialSettings {
    /// The dimension's rule (26.2's surface rule).
    rule: Rule,
    /// `default_block`; 26.4's noise settings have none, and leave solid positions to the rule.
    default_block: Option<State>,
    /// The sea level, which icebergs build to.
    sea_level: i32,
    /// The router's `chunk_surface_level`, the material system's preliminary surface; `None` for
    /// 26.2's surface rules, whose preliminary surface is the terrain's.
    chunk_surface_level: Option<Df>,
    /// Every noise the rule and the material system sample, by id.
    noises: HashMap<String, NormalNoiseParameters>,
    /// `legacy_random_source`.
    legacy: bool,
    /// The dimension's heights, which vertical anchors resolve against.
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
        // 26.4's noise settings leave solid positions to the rule, which its terrain builder runs
        // with the carvers (`docs/specs/material.md#264`).
        let default_block = match settings.get("default_block") {
            None => None,
            Some(json) => Some(loader.state(json, &root.child("default_block"))?),
        };
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
            dirt: palette.state("minecraft:dirt"),
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

    /// The default block's state; `None` for 26.4's, whose chunks are built by
    /// [`BoundMaterial::build_terrain`].
    pub fn default_block(&self) -> Option<State> {
        self.default_block
    }

    /// Whether the rule has ore veins (26.3 on).
    pub fn has_ore_veins(&self) -> bool {
        self.rule.has_ore_vein()
    }

    /// Whether these are 26.2's surface rules, of the 1.21 family.
    pub fn is_v1_21(&self) -> bool {
        self.chunk_surface_level.is_none()
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
        let [
            clay_bands_offset,
            surface,
            surface_secondary,
            badlands_pillar,
            badlands_pillar_roof,
            badlands_surface,
            iceberg_pillar,
            iceberg_pillar_roof,
            iceberg_surface,
        ] = SYSTEM_NOISES.map(&mut noise);
        Ok(SystemNoises {
            clay_bands_offset: clay_bands_offset?,
            surface: surface?,
            surface_secondary: surface_secondary?,
            badlands_pillar: badlands_pillar?,
            badlands_pillar_roof: badlands_pillar_roof?,
            badlands_surface: badlands_surface?,
            iceberg_pillar: iceberg_pillar?,
            iceberg_pillar_roof: iceberg_pillar_roof?,
            iceberg_surface: iceberg_surface?,
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
        // The pillar and roof noises' horizontal scales.
        let (f, r) = if iceberg { (1.28, 1.17) } else { (0.2, 0.75) };
        let (x, z) = (f64::from(x), f64::from(z));
        let (pillar, roof) = match self {
            Family::Material { noises, .. } => {
                let (surface_noise, pillar_noise, roof_noise) = if iceberg {
                    (
                        &noises.iceberg_surface,
                        &noises.iceberg_pillar,
                        &noises.iceberg_pillar_roof,
                    )
                } else {
                    (
                        &noises.badlands_surface,
                        &noises.badlands_pillar,
                        &noises.badlands_pillar_roof,
                    )
                };
                let surface = (f64::from(surface_noise.get(x, 0.0, z)) * 8.25).abs();
                let pillar = surface.min(f64::from(pillar_noise.get(x * f, 0.0, z * f) * 15.0));
                if !keep(pillar) {
                    return None;
                }
                (
                    pillar,
                    (f64::from(roof_noise.get(x * r, 0.0, z * r)) * 1.5).abs(),
                )
            }
            Family::V1_21 { noises, .. } => {
                let (surface_noise, pillar_noise, roof_noise) = if iceberg {
                    (
                        &noises.iceberg_surface,
                        &noises.iceberg_pillar,
                        &noises.iceberg_pillar_roof,
                    )
                } else {
                    (
                        &noises.badlands_surface,
                        &noises.badlands_pillar,
                        &noises.badlands_pillar_roof,
                    )
                };
                let surface = (surface_noise.get_value(x, 0.0, z) * 8.25).abs();
                let pillar = surface.min(pillar_noise.get_value(x * f, 0.0, z * f) * 15.0);
                if !keep(pillar) {
                    return None;
                }
                (
                    pillar,
                    (roof_noise.get_value(x * r, 0.0, z * r) * 1.5).abs(),
                )
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
///
/// Plain terracotta with single orange bands, 2 to 6 apart (the first 1 to 5 in); then runs of
/// yellow, brown and red laid over it at random; then 9 to 15 white bands, 4 to 19 apart, each
/// with a light grey band either side at even odds.
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
        let length = (base + random.next_int(3)) as usize;
        let start = random.next_int(bands.len() as i32) as usize;
        for j in 0..length {
            if start + j >= bands.len() {
                break;
            }
            bands[start + j] = state;
        }
    }
}

/// A block an ore vein gives, from [`BoundMaterial::vein_at`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VeinAt {
    /// The vein's ore block, which names the vein: `minecraft:copper_ore` or
    /// `minecraft:deepslate_iron_ore` in vanilla.
    pub ore: State,
    /// Which of the vein's blocks it is.
    pub part: VeinPart,
}

/// Which of an ore vein's blocks a block is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VeinPart {
    /// The ore.
    Ore,
    /// The raw-ore block.
    RawOre,
    /// The filler stone.
    Filler,
}

/// The rule at a position alone: a block, and the vein if one gives it; no block; or a
/// condition that needs more than the position.
enum Point {
    Block(Option<VeinAt>),
    Pass,
    Unknown,
}

/// `VerticalGradientConditionSource`: true at and below one y, false at and above another, and
/// in between by a positional random, with a chance falling linearly.
fn vertical_gradient(
    random: &World,
    true_at_and_below: i32,
    false_at_and_above: i32,
    [x, y, z]: [i32; 3],
) -> bool {
    if y <= true_at_and_below {
        return true;
    }
    if y >= false_at_and_above {
        return false;
    }
    let chance = map(
        f64::from(y),
        f64::from(true_at_and_below),
        f64::from(false_at_and_above),
        1.0,
        0.0,
    );
    f64::from(random.at(x, y, z).next_float()) < chance
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
            Rule::Condition(condition, then) => Bound::Condition(
                Box::new(self.condition(condition)?),
                Box::new(self.rule(then)?),
            ),
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
            Condition::Not(inner) => BoundCondition::Not(Box::new(self.condition(inner)?)),
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
    default_block: Option<State>,
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
    region: Region<'r>,
    min_qy: i32,
    max_qy: i32,
    /// The zoom's displacements for each of the region's quarts, by the region's cell order,
    /// computed the first time a block asks for them. The 64 blocks of a quart cell share their
    /// 8 corners, so each quart's are computed once instead of for every block around it.
    fiddles: RefCell<Vec<Option<[f64; 3]>>>,
}

/// Where [`ChunkBiomes`] reads quarts from.
enum Region<'r> {
    Biomes(&'r BiomeRegion),
    /// One biome everywhere, for tests.
    #[cfg(test)]
    One(spyglass_biome::Lookup),
}

impl Region<'_> {
    /// The biome region read, if there is one.
    fn region(&self) -> Option<&BiomeRegion> {
        match self {
            Region::Biomes(region) => Some(region),
            #[cfg(test)]
            Region::One(_) => None,
        }
    }

    fn get(&self, [qx, qy, qz]: [i32; 3]) -> spyglass_biome::Lookup {
        match self {
            Region::Biomes(region) => region
                .get(qx, qy, qz)
                .expect("the region covers the chunk's biome lookups"),
            #[cfg(test)]
            Region::One(lookup) => lookup.clone(),
        }
    }
}

impl<'r> ChunkBiomes<'r> {
    /// The biomes of `region`, which must cover a chunk's quarts and one more on every
    /// horizontal side, over a level from `min_y` of `height` blocks.
    pub fn new(region: &'r BiomeRegion, min_y: i32, height: i32) -> Self {
        let min_qy = min_y >> 2;
        ChunkBiomes {
            region: Region::Biomes(region),
            min_qy,
            max_qy: min_qy + (height >> 2) - 1,
            fiddles: RefCell::new(Vec::new()),
        }
    }

    /// The quart block `pos` takes its biome from, through `zoom`, with the corners'
    /// displacements kept for the next block.
    fn zoomed(&self, zoom: &BiomeZoom, pos: [i32; 3]) -> [i32; 3] {
        let Some(region) = self.region.region() else {
            return zoom.quart(pos);
        };
        let (min, size) = (region.min(), region.size());
        let mut kept = self.fiddles.borrow_mut();
        if kept.is_empty() {
            kept.resize(size.iter().map(|&s| s as usize).product(), None);
        }
        zoom.quart_with(pos, |quart| {
            // The region's cell order, x fastest, then z, then y; a corner outside it (below
            // the level, where the biome is clamped but the displacement isn't) isn't kept.
            let offset = |d: usize| {
                let o = i64::from(quart[d]) - i64::from(min[d]);
                (0..i64::from(size[d])).contains(&o).then_some(o as usize)
            };
            let [sx, _, sz] = size.map(|s| s as usize);
            let index = match (offset(0), offset(1), offset(2)) {
                (Some(x), Some(y), Some(z)) => (y * sz + z) * sx + x,
                _ => return zoom.fiddles(quart),
            };
            *kept[index].get_or_insert_with(|| zoom.fiddles(quart))
        })
    }

    /// The biome of quart `(qx, qy, qz)`, and whether it's tied.
    fn quart(&self, [qx, qy, qz]: [i32; 3]) -> (BiomeId, bool) {
        let lookup = self
            .region
            .get([qx, qy.clamp(self.min_qy, self.max_qy), qz]);
        (lookup.biome(), lookup.is_tied())
    }

    /// The candidates of quart `(qx, qy, qz)`.
    fn candidates(&self, [qx, qy, qz]: [i32; 3]) -> Vec<BiomeId> {
        let qy = qy.clamp(self.min_qy, self.max_qy);
        self.region.get([qx, qy, qz]).candidates().to_vec()
    }
}

/// What the rule is evaluated at: the column, then the block (`MaterialRuleContext`).
struct Context<'a> {
    system: &'a BoundMaterial,
    biomes: &'a ChunkBiomes<'a>,
    /// For 26.2's preliminary surface.
    terrain: Option<&'a Terrain>,
    /// 26.2's `preliminarySurfaceCache`: the preliminary surface at the corners of the chunk's
    /// surface cell, once read.
    cell: &'a Cell<Option<[i32; 4]>>,
    /// The column.
    x: i32,
    z: i32,
    /// The surface gradients in x and z, as steep reads them.
    gradient: Cell<[i32; 2]>,
    /// Whether [`gradient`](Self::gradient) is fixed for the column.
    gradient_read: Cell<bool>,
    /// `getSurfaceDepth`: how deep the column's surface layer is, from noise and a little
    /// randomness.
    surface_depth: i32,
    /// `getSurfaceSecondary`, once read.
    surface_secondary: Cell<Option<f64>>,
    /// `getMinSurfaceLevel`, once read.
    min_surface_level: Cell<Option<i32>>,
    /// The block's y.
    y: i32,
    /// How many solid blocks down from the last air above the block is, counting itself.
    stone_depth_above: i32,
    /// How many solid blocks up from the next non-solid block below the block is, counting
    /// itself.
    stone_depth_below: i32,
    /// One above the top of the fluid over the block, or `i32::MIN` if there's none below the
    /// last air.
    water_height: i32,
    /// The block's biome, once looked up.
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
                            .and_then(|t| {
                                t.preliminary_surface_level(
                                    i.wrapping_add(di).wrapping_shl(4),
                                    j.wrapping_add(dj).wrapping_shl(4),
                                )
                            })
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
    /// `Biome.BIOME_INFO_NOISE` at `(x, z)`, in this family's form.
    pub fn biome_info(&self, x: f64, z: f64) -> f64 {
        self.temperature.biome_info(x, z)
    }

    /// The biomes whose extensions add blocks: eroded badlands' pillars and the frozen oceans'
    /// icebergs.
    pub fn extension_biomes(&self) -> Vec<BiomeId> {
        std::iter::once(self.eroded_badlands)
            .chain(self.frozen_oceans)
            .flatten()
            .collect()
    }

    /// Where the rule can give a position air (a block, band or vein block of it is air): in
    /// the biomes returned, those every such block is under a `biome` condition for, or
    /// `None` if anywhere. The noise fill's air and fluids it leaves alone.
    pub fn air_biomes(&self, palette: &Palette) -> Option<Vec<BiomeId>> {
        /// Adds to `out` the biomes in `within` (any if `None`) where `rule` gives air; false
        /// if it can give air in any biome.
        fn walk(
            system: &BoundMaterial,
            rule: &Bound,
            within: Option<&[bool]>,
            air: &dyn Fn(State) -> bool,
            out: &mut Vec<bool>,
        ) -> bool {
            let found = match rule {
                Bound::Block(s) => air(*s),
                Bound::Bandlands => system.bands.iter().any(|&s| air(s)),
                Bound::OreVein(v) => [v.ore, v.raw_ore, v.filler].into_iter().any(air),
                Bound::Sequence(rules) => {
                    return rules.iter().all(|r| walk(system, r, within, air, out));
                }
                Bound::Condition(condition, then) => {
                    let BoundCondition::Biome(set) = &**condition else {
                        return walk(system, then, within, air, out);
                    };
                    let narrowed: Vec<bool> = match within {
                        Some(w) => set.iter().zip(w).map(|(a, b)| *a && *b).collect(),
                        None => set.clone(),
                    };
                    return walk(system, then, Some(&narrowed), air, out);
                }
            };
            match (found, within) {
                (false, _) => true,
                (true, None) => false,
                (true, Some(w)) => {
                    out.resize(out.len().max(w.len()), false);
                    for (o, b) in out.iter_mut().zip(w) {
                        *o |= b;
                    }
                    true
                }
            }
        }
        let mut out = Vec::new();
        walk(self, &self.rule, None, &|s| palette.is_air(s), &mut out).then(|| {
            (0..out.len())
                .filter(|&i| out[i])
                .map(|i| BiomeId(i as u16))
                .collect()
        })
    }

    /// The quart block `pos` takes its biome from (`BiomeManager.getBiome`'s zoom).
    pub fn biome_quart(&self, pos: [i32; 3]) -> [i32; 3] {
        self.zoom.quart(pos)
    }

    /// `BiomeManager.getBiome`: the biome block `pos` takes, through the zoom, and whether its
    /// quart is tied.
    fn biome_at(&self, biomes: &ChunkBiomes<'_>, pos: [i32; 3]) -> (BiomeId, bool) {
        biomes.quart(biomes.zoomed(&self.zoom, pos))
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

    fn test(&self, condition: &BoundCondition, context: &Context<'_>) -> bool {
        match condition {
            BoundCondition::Biome(set) => set
                .get(usize::from(context.biome().0))
                .copied()
                .unwrap_or(false),
            BoundCondition::Noise {
                noise,
                min,
                max,
                is_3d,
            } => {
                let y = if *is_3d { f64::from(context.y) } else { 0.0 };
                let v = noise.get(f64::from(context.x), y, f64::from(context.z));
                v >= *min && v <= *max
            }
            BoundCondition::VerticalGradient {
                random,
                true_at_and_below,
                false_at_and_above,
            } => vertical_gradient(
                random,
                *true_at_and_below,
                *false_at_and_above,
                [context.x, context.y, context.z],
            ),
            BoundCondition::YAbove {
                y,
                surface_depth_multiplier,
                add_stone_depth,
            } => {
                let above = if *add_stone_depth {
                    context.stone_depth_above
                } else {
                    0
                };
                context.y.wrapping_add(above)
                    >= y.wrapping_add(
                        context
                            .surface_depth
                            .wrapping_mul(*surface_depth_multiplier),
                    )
            }
            BoundCondition::Water {
                offset,
                surface_depth_multiplier,
                add_stone_depth,
            } => {
                if context.water_height == i32::MIN {
                    return true;
                }
                let above = if *add_stone_depth {
                    context.stone_depth_above
                } else {
                    0
                };
                context.y.wrapping_add(above)
                    >= context.water_height.wrapping_add(*offset).wrapping_add(
                        context
                            .surface_depth
                            .wrapping_mul(*surface_depth_multiplier),
                    )
            }
            BoundCondition::Temperature => {
                let climate = self.climates[usize::from(context.biome().0)];
                self.temperature.cold_enough_to_snow(
                    climate,
                    [context.x, context.y, context.z],
                    self.sea_level,
                )
            }
            BoundCondition::Steep => {
                context.gradient_read.set(true);
                let [gx, gz] = context.gradient.get();
                gx <= -4 || gz >= 4
            }
            BoundCondition::Hole => context.surface_depth <= 0,
            BoundCondition::AbovePreliminarySurface => context.y >= context.min_surface_level(),
            BoundCondition::Not(inner) => !self.test(inner, context),
            BoundCondition::StoneDepth {
                offset,
                add_surface_depth,
                secondary_depth_range,
                ceiling,
            } => {
                let depth = if *ceiling {
                    context.stone_depth_below
                } else {
                    context.stone_depth_above
                };
                let surface = if *add_surface_depth {
                    context.surface_depth
                } else {
                    0
                };
                let secondary = if *secondary_depth_range == 0 {
                    0
                } else {
                    map(
                        context.surface_secondary(),
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

    /// The ore vein block the rule gives a solid position, as far as the rule can be followed
    /// at the position alone (`docs/specs/render.md#underground`): up to the first condition
    /// that needs the column, biome or surface. 26.3's and 26.4's rules have the veins right
    /// after the bedrock floor, before any such condition, so this is the chunk's answer.
    pub fn vein_at(&self, [x, y, z]: [i32; 3]) -> Option<VeinAt> {
        match self.at_point(&self.rule, x, y, z) {
            Point::Block(vein) => vein,
            Point::Pass | Point::Unknown => None,
        }
    }

    /// [`apply`](Self::apply) with no column: whether the rule gives a block and, if a vein
    /// gives it, which.
    fn at_point(&self, rule: &Bound, x: i32, y: i32, z: i32) -> Point {
        match rule {
            Bound::Block(_) | Bound::Bandlands => Point::Block(None),
            Bound::Sequence(rules) => {
                for r in rules {
                    match self.at_point(r, x, y, z) {
                        Point::Pass => {}
                        found => return found,
                    }
                }
                Point::Pass
            }
            Bound::Condition(condition, then) => match self.test_at_point(condition, x, y, z) {
                Some(true) => self.at_point(then, x, y, z),
                Some(false) => Point::Pass,
                None => Point::Unknown,
            },
            Bound::OreVein(v) => match self.vein(v, x, y, z) {
                Some((_, part)) => Point::Block(Some(VeinAt { ore: v.ore, part })),
                None => Point::Pass,
            },
        }
    }

    /// [`test`](Self::test) for the conditions that need only the position; `None` for the
    /// others.
    fn test_at_point(&self, condition: &BoundCondition, x: i32, y: i32, z: i32) -> Option<bool> {
        match condition {
            BoundCondition::Noise {
                noise,
                min,
                max,
                is_3d,
            } => {
                let ny = if *is_3d { f64::from(y) } else { 0.0 };
                let v = noise.get(f64::from(x), ny, f64::from(z));
                Some(v >= *min && v <= *max)
            }
            BoundCondition::VerticalGradient {
                random,
                true_at_and_below,
                false_at_and_above,
            } => Some(vertical_gradient(
                random,
                *true_at_and_below,
                *false_at_and_above,
                [x, y, z],
            )),
            BoundCondition::Hole => Some(self.surface_depth(x, z) <= 0),
            BoundCondition::Not(inner) => self.test_at_point(inner, x, y, z).map(|t| !t),
            _ => None,
        }
    }

    /// `OreVeinRule.apply`: the vein's block at `(x, y, z)` and which part it is, or `None`
    /// outside the vein.
    fn vein(&self, v: &BoundVein, x: i32, y: i32, z: i32) -> Option<(State, VeinPart)> {
        let density = v.density.sample(x, y, z);
        // fcmpg/ifgt: NaN goes on.
        if density <= 0.0 {
            return None;
        }
        let mut random = v.random.at(x, y, z);
        // fcmpl/ifle: NaN goes on.
        if random.next_float() > density {
            return None;
        }
        let richness = v.richness.sample(x, y, z);
        if random.next_float() < richness && v.filler_gap.sample(x, y, z) < 0.0 {
            return Some(if random.next_float() < v.raw_ore_chance {
                (v.raw_ore, VeinPart::RawOre)
            } else {
                (v.ore, VeinPart::Ore)
            });
        }
        Some((v.filler, VeinPart::Filler))
    }

    fn apply(&self, rule: &Bound, context: &Context<'_>) -> Option<State> {
        match rule {
            Bound::Block(s) => Some(*s),
            Bound::Sequence(rules) => rules.iter().find_map(|r| self.apply(r, context)),
            Bound::Condition(condition, then) => {
                if self.test(condition, context) {
                    self.apply(then, context)
                } else {
                    None
                }
            }
            Bound::Bandlands => Some(self.band(context.x, context.y, context.z)),
            Bound::OreVein(v) => self
                .vein(v, context.x, context.y, context.z)
                .map(|(state, _)| state),
        }
    }

    /// `MaterialSystem.topMaterial` (26.2's `SurfaceSystem.topMaterial`): the rule's block at `pos` of `chunk` as the top
    /// of the surface, with one block of stone above and below, and water above if
    /// `fluid_above`, and whether it depends on a tied biome. The gradients are the chunk's
    /// heights now. `terrain` is the seed's, for 26.2's preliminary surface.
    pub fn top_material(
        &self,
        chunk: &Chunk,
        biomes: &ChunkBiomes<'_>,
        terrain: Option<&Terrain>,
        [x, y, z]: [i32; 3],
        fluid_above: bool,
    ) -> (Option<State>, bool) {
        let (dx, dz) = (x & 15, z & 15);
        let gradient = [
            chunk.height_at((dx + 1).min(15), dz) - chunk.height_at((dx - 1).max(0), dz),
            chunk.height_at(dx, (dz + 1).min(15)) - chunk.height_at(dx, (dz - 1).max(0)),
        ];
        let cell = Cell::new(None);
        let context = Context {
            system: self,
            biomes,
            terrain,
            cell: &cell,
            x,
            z,
            gradient: Cell::new(gradient),
            gradient_read: Cell::new(true),
            surface_depth: self.surface_depth(x, z),
            surface_secondary: Cell::new(None),
            min_surface_level: Cell::new(None),
            y,
            stone_depth_above: 1,
            stone_depth_below: 1,
            water_height: if fluid_above { y + 1 } else { i32::MIN },
            biome: Cell::new(None),
            tied: Cell::new(false),
        };
        (self.apply(&self.rule, &context), context.tied.get())
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
                    let candidates = biomes.candidates(biomes.zoomed(&self.zoom, start_pos));
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
                let mut context = Context {
                    system: self,
                    biomes,
                    terrain: Some(terrain),
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
                        context.y = y;
                        context.stone_depth_above = stone_depth_above;
                        context.stone_depth_below = y - next_ceiling_stone_y + 1;
                        context.water_height = water_height;
                        context.biome.set(None);
                        context.tied.set(false);
                        // 26.2 tries the rule only on the default block itself, so the noise
                        // fill's ore veins stay; from 26.3, on every stone within the level.
                        let tried = if v1_21 {
                            Some(state) == self.default_block
                        } else {
                            (min_y..=max_y).contains(&y)
                        };
                        if tried {
                            if !context.gradient_read.get() {
                                context.gradient.set(gradient(chunk));
                            }
                            if let Some(s) = self.apply(&self.rule, &context) {
                                chunk.set(palette, [x, y, z], s);
                            }
                            if context.tied.get() {
                                chunk.mark_uncertain([x, y, z]);
                            }
                        }
                    }
                    y -= 1;
                }
                if self.frozen_oceans.contains(&Some(biome)) {
                    let min_surface = context.min_surface_level();
                    let column = [x, z, start];
                    self.frozen_ocean_extension(chunk, palette, min_surface, biome, column, None);
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
        let default_block = self
            .default_block
            .expect("noise settings filled by default block have one");
        let default = palette.block_id(default_block);
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
            chunk.set(palette, [x, y, z], default_block);
            y -= 1;
        }
    }

    /// `frozenOceanExtension`: an iceberg of packed ice, capped with snow, over and under the
    /// sea where the iceberg noises are high.
    /// From 26.4, the iceberg places nothing at positions `mask` carves.
    fn frozen_ocean_extension(
        &self,
        chunk: &mut Chunk,
        palette: &Palette,
        min_surface_level: i32,
        biome: BiomeId,
        [x, z, start]: [i32; 3],
        mask: Option<&CarvingMask>,
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
                // 26.4's block column skips carved positions, after the draws and the count.
                let carved = mask.is_some_and(|m| m.is_carved(x & 15, y, z & 15));
                let block = if snow <= max_snow && y > min_snow_y {
                    snow += 1;
                    self.fixed.snow_block
                } else {
                    self.fixed.packed_ice
                };
                if !carved {
                    chunk.set(palette, [x, y, z], block);
                }
            }
            y -= 1;
        }
    }
}

/// What 26.4's `NoiseChunk.prepareColumn` gives at one y: the aquifer's answer, solid yet
/// undecided, or a fluid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    /// `NoiseColumn.AIR`.
    Air,
    /// `NoiseColumn.SOLID`: the rule decides.
    Solid,
    /// A fluid state.
    Fluid(State),
}

/// 26.4's `NoiseColumn`: one column of the chunk's noise, up to its top non-air y, with the
/// surface gradients from the chunk's other columns' tops.
#[derive(Debug, Clone)]
pub struct NoiseColumn {
    /// The y of index 0.
    pub min_y: i32,
    /// From `min_y` up.
    pub slots: Vec<Slot>,
    /// `topIndexY`: the top non-air index, or −1.
    pub top: i32,
    /// `surfaceGradientX` and `surfaceGradientZ`.
    pub gradient: [i32; 2],
}

impl NoiseColumn {
    /// `getBlock(y)`: air outside the column.
    fn at(&self, y: i32) -> Slot {
        usize::try_from(i64::from(y) - i64::from(self.min_y))
            .ok()
            .and_then(|i| self.slots.get(i).copied())
            .unwrap_or(Slot::Air)
    }

    /// `setSolid(y)`, which raises the top.
    fn set_solid(&mut self, y: i32) {
        let i = y - self.min_y;
        self.slots[i as usize] = Slot::Solid;
        self.top = self.top.max(i);
    }

    /// `getCeilingBelowIndex(i)`: one above the first index below `i` that isn't solid, or
    /// `WAY_BELOW_MIN_Y` if all are.
    fn ceiling_below(&self, i: i32) -> i32 {
        (0..i)
            .rev()
            .find(|&j| self.slots[j as usize] != Slot::Solid)
            .map_or(WAY_BELOW_MIN_Y, |j| j + 1)
    }
}

impl BoundMaterial {
    /// 26.4's `ChunkTerrainBuilder.fillChunk`: each column of `columns` (by `(z << 4) + x`,
    /// z outer) into the empty `chunk`, with the rule and, where `mask` carves, the carving.
    /// `substance` is the aquifer's answer at a carved position at density 0: a state, or
    /// `None` where it's solid. `uncarvable` is by state.
    #[allow(clippy::too_many_arguments)]
    pub fn build_terrain(
        &self,
        chunk: &mut Chunk,
        palette: &Palette,
        biomes: &ChunkBiomes<'_>,
        columns: &mut [NoiseColumn],
        mask: Option<&CarvingMask>,
        uncarvable: &[bool],
        substance: &mut dyn FnMut([i32; 3]) -> Option<State>,
    ) {
        let [cx, cz] = chunk.pos;
        let (x0, z0) = (cx.wrapping_shl(4), cz.wrapping_shl(4));
        let (min_y, max_y) = (chunk.min_y, chunk.min_y + chunk.height - 1);
        for dz in 0..16 {
            for dx in 0..16 {
                let (x, z) = (x0 + dx, z0 + dz);
                let column = &mut columns[(dz * 16 + dx) as usize];
                let start = column.min_y + column.top + 1;
                // ChunkAccess.getBiome: the stored biome, at y clamped to the level.
                let start_pos = [x, start.clamp(min_y, max_y), z];
                let (biome, start_tied) = self.biome_at(biomes, start_pos);
                if start_tied {
                    let candidates = biomes.candidates(biomes.zoomed(&self.zoom, start_pos));
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
                    self.extend_eroded_badlands(column, x, z);
                }
                let frozen = self.frozen_oceans.contains(&Some(biome));
                let min_surface = self.fill_column(
                    chunk,
                    biomes,
                    column,
                    [x, z],
                    mask,
                    uncarvable,
                    substance,
                    frozen,
                );
                if let Some(min_surface) = min_surface {
                    self.frozen_ocean_extension(
                        chunk,
                        palette,
                        min_surface,
                        biome,
                        [x, z, start],
                        mask,
                    );
                }
            }
        }
    }

    /// `ChunkTerrainBuilder.fillColumn`: the column top down. Fluids go in as they are; each
    /// solid position in the level gets the rule's block, or where it's carved and the block
    /// isn't `#uncarvable`, the aquifer's substance. Dirt under a carved top block is tried
    /// again as the surface. Returns the minimum surface level if `frozen`, for the iceberg.
    #[allow(clippy::too_many_arguments)]
    fn fill_column(
        &self,
        chunk: &mut Chunk,
        biomes: &ChunkBiomes<'_>,
        column: &NoiseColumn,
        [x, z]: [i32; 2],
        mask: Option<&CarvingMask>,
        uncarvable: &[bool],
        substance: &mut dyn FnMut([i32; 3]) -> Option<State>,
        frozen: bool,
    ) -> Option<i32> {
        let (min_y, max_y) = (chunk.min_y, chunk.min_y + chunk.height - 1);
        let cell = Cell::new(None);
        let mut context = Context {
            system: self,
            biomes,
            terrain: None,
            cell: &cell,
            x,
            z,
            gradient: Cell::new(column.gradient),
            gradient_read: Cell::new(true),
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
        let mut next_ceiling = i32::MAX;
        // Whether a carved top block is above, with only carved positions between: dirt is
        // then the surface, and tried again as it.
        let mut under_carved_top = false;
        for i in (0..=column.top).rev() {
            let y = column.min_y + i;
            let pos = [x, y, z];
            let in_level = (min_y..=max_y).contains(&y);
            let carved = mask.is_some_and(|m| m.is_carved(x & 15, y, z & 15));
            match column.slots[i as usize] {
                Slot::Air => {
                    stone_depth_above = 0;
                    water_height = i32::MIN;
                    under_carved_top &= carved;
                    continue;
                }
                Slot::Fluid(fluid) => {
                    if water_height == i32::MIN {
                        water_height = y + 1;
                    }
                    if in_level {
                        chunk.put(pos, fluid);
                    }
                    under_carved_top &= carved;
                    continue;
                }
                Slot::Solid => {}
            }
            if i <= next_ceiling {
                next_ceiling = column.ceiling_below(i);
            }
            stone_depth_above += 1;
            if !in_level {
                continue;
            }
            // Indices, not ys: with no ceiling, `WAY_BELOW_MIN_Y` is taken as an index.
            let stone_depth_below = i.wrapping_sub(next_ceiling).wrapping_add(1);
            context.y = y;
            context.stone_depth_above = stone_depth_above;
            context.stone_depth_below = stone_depth_below;
            context.water_height = water_height;
            context.biome.set(None);
            context.tied.set(false);
            let mut result = self.apply(&self.rule, &context);
            let mut tied = context.tied.get();
            // A carved position the aquifer finds solid gets the rule's block after all.
            if carved
                && result.is_none_or(|s| !uncarvable[usize::from(s)])
                && let Some(s) = substance(pos)
            {
                under_carved_top |= stone_depth_above == 1;
                if s != state::AIR {
                    chunk.put(pos, s);
                }
                if tied {
                    chunk.mark_uncertain(pos);
                }
                continue;
            }
            if under_carved_top {
                if result == Some(self.fixed.dirt) {
                    context.stone_depth_above = 1;
                    context.biome.set(None);
                    context.tied.set(false);
                    result = self.apply(&self.rule, &context);
                    tied |= context.tied.get();
                }
                under_carved_top = false;
            }
            if let Some(s) = result {
                chunk.put(pos, s);
            }
            if tied {
                chunk.mark_uncertain(pos);
            }
        }
        frozen.then(|| context.min_surface_level())
    }

    /// 26.4's `SurfaceExtensions.extendErodedBadlands`: the pillar, as solid positions of the
    /// noise column, for the rule to decide.
    fn extend_eroded_badlands(&self, column: &mut NoiseColumn, x: i32, z: i32) {
        // dcmpg/ifgt: NaN goes on.
        let Some((pillar, roof)) = self.family.pillar(false, x, z, |p| p > 0.0 || p.is_nan())
        else {
            return;
        };
        let top = floor_f64(64.0 + (pillar * pillar * 2.5).min((roof * 50.0).ceil() + 24.0));
        if column.min_y + column.top + 1 > top {
            return;
        }
        let mut y = top;
        while y >= column.min_y {
            match column.at(y) {
                Slot::Solid => break,
                Slot::Fluid(s) if s == state::WATER => return,
                _ => {}
            }
            y -= 1;
        }
        let mut y = top;
        while y >= column.min_y && column.at(y) == Slot::Air {
            column.set_solid(y);
            y -= 1;
        }
    }
}

/// `Mth.floor(double)`.
fn floor_f64(v: f64) -> i32 {
    let i = v as i32;
    if v < f64::from(i) { i - 1 } else { i }
}

#[cfg(test)]
mod tests {
    //! Each rule and condition type on fixture data, through the loader and the binder, at
    //! hand-set context values.

    use serde_json::{Value, json};
    use spyglass_biome::Lookup;

    use super::*;

    const SETTINGS: &str = "test:settings";
    const ANCHORS: AnchorHeights = AnchorHeights {
        min_y: -64,
        height: 384,
    };
    /// Biome 0 is temperate, biome 1 cold enough to snow.
    const PLAINS: u16 = 0;
    const SNOWY: u16 = 1;

    /// Noise settings with `rule`, the system's noises and `test:noise`, and `extra` entries.
    fn resources(rule: Value, extra: &[(&str, Value)]) -> HashMap<String, Value> {
        let mut r = HashMap::new();
        r.insert(
            format!("{NOISE_SETTINGS}/{SETTINGS}"),
            json!({
                "default_block": "minecraft:stone",
                "sea_level": 63,
                "material_rule": rule,
                "noise_router": {"chunk_surface_level": 64.0},
            }),
        );
        for id in SYSTEM_NOISES.into_iter().chain(["test:noise"]) {
            r.insert(
                format!("worldgen/noise/{id}"),
                json!({"base_amplitude": 1.0, "base_octave": -4, "octave_count": 2}),
            );
        }
        for (key, value) in extra {
            r.insert((*key).to_string(), value.clone());
        }
        r
    }

    fn load(resources: &HashMap<String, Value>) -> Result<(MaterialSettings, Palette), LoadError> {
        let mut palette = Palette::new();
        let settings = MaterialSettings::load(resources, SETTINGS, ANCHORS, &mut palette)?;
        Ok((settings, palette))
    }

    /// The context values a rule is tried at.
    #[derive(Debug, Clone, Copy)]
    struct Place {
        pos: [i32; 3],
        biome: u16,
        surface_depth: i32,
        stone_depth_above: i32,
        stone_depth_below: i32,
        water_height: i32,
        gradient: [i32; 2],
    }

    impl Default for Place {
        fn default() -> Self {
            Place {
                pos: [5, 64, -7],
                biome: PLAINS,
                surface_depth: 3,
                stone_depth_above: 1,
                stone_depth_below: 1,
                water_height: i32::MIN,
                gradient: [0, 0],
            }
        }
    }

    /// The block `rule` gives at `place`, by name.
    fn eval(rule: Value, place: Place) -> Option<String> {
        let (settings, palette) = load(&resources(rule, &[])).unwrap();
        let names = BiomeNames::new(vec![
            "minecraft:plains".into(),
            "minecraft:snowy_plains".into(),
        ]);
        let climates = [0.8, 0.0].map(|temperature| Climate {
            temperature,
            frozen: false,
        });
        let material = settings.bind(1, &palette, &names, &climates).unwrap();
        let biomes = ChunkBiomes {
            region: Region::One(Lookup::Unique(BiomeId(place.biome))),
            min_qy: -16,
            max_qy: 79,
            fiddles: RefCell::new(Vec::new()),
        };
        let cell = Cell::new(None);
        let [x, y, z] = place.pos;
        let context = Context {
            system: &material,
            biomes: &biomes,
            terrain: None,
            cell: &cell,
            x,
            z,
            gradient: Cell::new(place.gradient),
            gradient_read: Cell::new(true),
            surface_depth: place.surface_depth,
            surface_secondary: Cell::new(None),
            min_surface_level: Cell::new(None),
            y,
            stone_depth_above: place.stone_depth_above,
            stone_depth_below: place.stone_depth_below,
            water_height: place.water_height,
            biome: Cell::new(None),
            tied: Cell::new(false),
        };
        material
            .apply(&material.rule, &context)
            .map(|s| palette.name(s).to_string())
    }

    fn block(id: &str) -> Value {
        json!({"type": "minecraft:block", "result_state": {"Name": id}})
    }

    /// Whether `condition` holds at `place`.
    fn holds(condition: Value, place: Place) -> bool {
        let rule = json!({
            "type": "minecraft:condition",
            "if_true": condition,
            "then_run": block("minecraft:dirt"),
        });
        eval(rule, place).is_some()
    }

    fn at(y: i32) -> Place {
        Place {
            pos: [5, y, -7],
            ..Place::default()
        }
    }

    #[test]
    fn block_sequence_and_condition_rules() {
        let place = Place::default();
        assert_eq!(
            eval(block("dirt"), place).as_deref(),
            Some("minecraft:dirt")
        );
        let never = json!({"type": "minecraft:hole"});
        let sequence = json!({"type": "minecraft:sequence", "sequence": [
            {"type": "minecraft:condition", "if_true": never, "then_run": block("dirt")},
            block("sand"),
            block("gravel"),
        ]});
        assert_eq!(eval(sequence, place).as_deref(), Some("minecraft:sand"));
        assert!(!holds(never, place));
    }

    #[test]
    fn bandlands_gives_a_clay_band() {
        let band = eval(json!({"type": "minecraft:bandlands"}), Place::default()).unwrap();
        assert!(band.ends_with("terracotta"), "{band}");
    }

    #[test]
    fn ore_vein_rules() {
        let vein = |density: f64, filler_gap: f64| {
            let rule = json!({
                "type": "minecraft:ore_vein",
                "ore_block": {"Name": "minecraft:copper_ore"},
                "raw_ore_block": {"Name": "minecraft:raw_copper_block"},
                "filler_block": {"Name": "minecraft:granite"},
                "raw_ore_chance": 0.0,
                "density": density,
                "richness": 1.0,
                "filler_gap": filler_gap,
            });
            eval(rule, Place::default())
        };
        // Outside the vein, nothing; in it, ore where the gap is negative, else filler.
        assert_eq!(vein(-1.0, -1.0), None);
        assert_eq!(vein(1.0, -1.0).as_deref(), Some("minecraft:copper_ore"));
        assert_eq!(vein(1.0, 1.0).as_deref(), Some("minecraft:granite"));
    }

    #[test]
    fn biome_and_temperature_conditions() {
        let plains = json!({"type": "minecraft:biome", "biome_is": ["minecraft:plains"]});
        let snowy = Place {
            biome: SNOWY,
            ..Place::default()
        };
        assert!(holds(plains.clone(), Place::default()));
        assert!(!holds(plains, snowy));
        let cold = json!({"type": "minecraft:temperature"});
        assert!(holds(cold.clone(), snowy));
        assert!(!holds(cold, Place::default()));
    }

    #[test]
    fn noise_threshold_conditions() {
        let threshold = |min: f64, is_3d: bool| {
            json!({"type": "minecraft:noise_threshold", "noise": "test:noise",
                   "min_threshold": min, "max_threshold": 1e9, "is_3d": is_3d})
        };
        assert!(holds(threshold(-1e9, false), Place::default()));
        assert!(!holds(threshold(1e9, false), Place::default()));
        // In 3D, the noise varies with y.
        let seen: Vec<bool> = (0..64)
            .map(|y| holds(threshold(0.0, true), at(y)))
            .collect();
        assert!(seen.contains(&true) && seen.contains(&false));
    }

    #[test]
    fn vertical_gradient_conditions() {
        let gradient = json!({"type": "minecraft:vertical_gradient", "random_name": "test:gradient",
            "true_at_and_below": {"absolute": 0}, "false_at_and_above": {"absolute": 10}});
        assert!(holds(gradient.clone(), at(0)));
        assert!(!holds(gradient.clone(), at(10)));
        let between: Vec<bool> = (-200..200)
            .map(|x| {
                let place = Place {
                    pos: [x, 5, 0],
                    ..Place::default()
                };
                holds(gradient.clone(), place)
            })
            .collect();
        assert!(between.contains(&true) && between.contains(&false));
    }

    #[test]
    fn y_above_and_water_conditions() {
        let y_above = |multiplier: i32, add: bool| {
            json!({"type": "minecraft:y_above", "anchor": {"absolute": 60},
                   "surface_depth_multiplier": multiplier, "add_stone_depth": add})
        };
        assert!(holds(y_above(0, false), at(60)));
        assert!(!holds(y_above(0, false), at(59)));
        let deep = Place {
            stone_depth_above: 2,
            ..at(58)
        };
        assert!(holds(y_above(0, true), deep));
        // 60 + 3 · 1.
        assert!(holds(y_above(1, false), at(63)));
        assert!(!holds(y_above(1, false), at(62)));
        let water = json!({"type": "minecraft:water", "offset": -1,
            "surface_depth_multiplier": 0, "add_stone_depth": false});
        assert!(holds(water.clone(), at(0)), "no water above");
        let under = |y| Place {
            water_height: 64,
            ..at(y)
        };
        assert!(holds(water.clone(), under(63)));
        assert!(!holds(water, under(62)));
    }

    #[test]
    fn steep_hole_and_preliminary_surface_conditions() {
        let steep = json!({"type": "minecraft:steep"});
        let sloped = |gradient| Place {
            gradient,
            ..Place::default()
        };
        assert!(holds(steep.clone(), sloped([-4, 0])));
        assert!(holds(steep.clone(), sloped([0, 4])));
        assert!(!holds(steep, sloped([-3, 3])));
        let hole = json!({"type": "minecraft:hole"});
        let depth = |surface_depth| Place {
            surface_depth,
            ..Place::default()
        };
        assert!(holds(hole.clone(), depth(0)));
        assert!(!holds(hole, depth(1)));
        // chunk_surface_level 64, plus the surface depth 3, minus 8.
        let above = json!({"type": "minecraft:above_preliminary_surface"});
        assert!(holds(above.clone(), at(59)));
        assert!(!holds(above, at(58)));
    }

    #[test]
    fn not_and_stone_depth_conditions() {
        let not_hole = json!({"type": "minecraft:not", "invert": {"type": "minecraft:hole"}});
        assert!(holds(not_hole, Place::default()));
        let stone_depth = |add: bool, surface_type: &str| {
            json!({"type": "minecraft:stone_depth", "offset": 0, "add_surface_depth": add,
                   "secondary_depth_range": 0, "surface_type": surface_type})
        };
        let depths = |above, below| Place {
            stone_depth_above: above,
            stone_depth_below: below,
            ..Place::default()
        };
        assert!(holds(stone_depth(false, "floor"), depths(1, 9)));
        assert!(!holds(stone_depth(false, "floor"), depths(2, 1)));
        assert!(holds(stone_depth(false, "ceiling"), depths(9, 1)));
        // 1 + the surface depth 3.
        assert!(holds(stone_depth(true, "floor"), depths(4, 9)));
        assert!(!holds(stone_depth(true, "floor"), depths(5, 9)));
    }

    #[test]
    fn rules_and_conditions_by_id() {
        let extra = [
            (
                "worldgen/material_rule/test:top",
                json!({"type": "minecraft:condition", "if_true": "test:never",
                       "then_run": block("dirt")}),
            ),
            (
                "worldgen/material_condition/test:never",
                json!({"type": "minecraft:hole"}),
            ),
        ];
        let (settings, _) = load(&resources(json!("test:top"), &extra)).unwrap();
        assert!(matches!(settings.rule, Rule::Condition(..)));
    }

    #[test]
    fn load_errors_name_the_resource_and_path() {
        let error = |rule: Value, extra: &[(&str, Value)]| {
            load(&resources(rule, extra)).unwrap_err().to_string()
        };
        let unknown = json!({"type": "minecraft:sequence", "sequence": [{"type": "test:nope"}]});
        assert_eq!(
            error(unknown, &[]),
            r#"test:settings: material_rule.sequence.0.type: unsupported material rule type "test:nope""#
        );
        let cycle = [
            ("worldgen/material_rule/test:a", json!("test:b")),
            ("worldgen/material_rule/test:b", json!("test:a")),
        ];
        assert_eq!(
            error(json!("test:a"), &cycle),
            "test:a: reference cycle: test:a -> test:b -> test:a"
        );
    }

    #[test]
    fn surface_rules_have_no_ore_veins() {
        let mut resources = resources(json!(null), &[]);
        let settings = resources
            .get_mut(&format!("{NOISE_SETTINGS}/{SETTINGS}"))
            .unwrap();
        settings["noise"] = json!({"size_horizontal": 1});
        settings["surface_rule"] = json!({
            "type": "minecraft:ore_vein",
            "ore_block": {"Name": "minecraft:copper_ore"},
            "raw_ore_block": {"Name": "minecraft:raw_copper_block"},
            "filler_block": {"Name": "minecraft:granite"},
            "raw_ore_chance": 0.0, "density": 1.0, "richness": 1.0, "filler_gap": 1.0,
        });
        assert_eq!(
            load(&resources).unwrap_err().to_string(),
            r#"test:settings: surface_rule: unsupported rule type in 26.2's surface rules "minecraft:ore_vein""#
        );
    }
}
