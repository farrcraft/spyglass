//! The flower bone meal grows on grass at a block (`docs/specs/flowers.md`): each biome's bone
//! meal features, the block state providers vanilla's flower features use, and the odds of
//! each answer.
//!
//! The noise providers are seeded by their own `seed` field, never the world's, so what they
//! pick at a block is the same in every world; only the biome depends on the seed.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use serde_json::{Map, Value};
use spyglass_biome::BiomeId;
use spyglass_blocks::state::BlockDefaults;
use spyglass_data::codec::{
    At, LoadError, LoadErrorKind, block_state, field, float, full_id, int, object,
};
use spyglass_data::providers::Weighted;
use spyglass_density::{Resources, parse_noise};
use spyglass_noise::mth::clamped_lerp;
use spyglass_noise::{NoiseStack, NormalNoise, NormalNoiseParameters, v1_21};
use spyglass_rng::{LegacyRandomSource, RandomSource};

use crate::FeatureSettings;

/// Configured features, from 26.3.
const FEATURE: &str = "worldgen/feature";
/// Configured features, in 26.2.
const CONFIGURED_FEATURE: &str = "worldgen/configured_feature";
/// Placed features.
const PLACED_FEATURE: &str = "worldgen/placed_feature";
/// Block state providers by id, from 26.3.
const BLOCK_STATE_PROVIDER: &str = "worldgen/block_state_provider";
/// Configured-feature tags, from 26.3.
const FEATURE_TAGS: &str = "tags/worldgen/feature";
/// Configured-feature tags, in 26.2.
const CONFIGURED_FEATURE_TAGS: &str = "tags/worldgen/configured_feature";
/// The tag bone meal's features come from (`FeatureTags.CAN_SPAWN_FROM_BONE_MEAL`).
const BONE_MEAL_TAG: &str = "minecraft:can_spawn_from_bone_meal";

/// Nested features deeper than this are refused, as a cycle would be.
const MAX_DEPTH: u32 = 32;

/// `nextFloat()` draws a multiple of 2⁻²⁴: this many values.
const FLOAT_STEPS: u128 = 1 << 24;

/// A block state a flower feature can place, by its index in [`Flowers::state_name`].
pub type FlowerState = u16;

/// A chance as an exact fraction, in lowest terms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Chance {
    /// The numerator.
    pub numerator: u128,
    /// The denominator, positive.
    pub denominator: u128,
}

impl Chance {
    /// Certain.
    pub const ONE: Chance = Chance {
        numerator: 1,
        denominator: 1,
    };

    /// Impossible.
    pub fn new_zero() -> Chance {
        Chance::new(0, 1)
    }

    /// The sum of two chances.
    pub fn plus(self, other: Chance) -> Chance {
        self.add(other)
    }

    /// `numerator / denominator`, reduced.
    pub fn new(numerator: u128, denominator: u128) -> Chance {
        let g = gcd(numerator, denominator).max(1);
        Chance {
            numerator: numerator / g,
            denominator: denominator / g,
        }
    }

    // The arithmetic is checked: vanilla's chances are far inside u128, and a data pack whose
    // weights overflowed it would otherwise get a wrong chance (ADR 0075).
    fn add(self, other: Chance) -> Chance {
        const OVERFLOW: &str = "a chance's fraction overflowed u128";
        let g = gcd(self.denominator, other.denominator);
        let denominator = (self.denominator / g)
            .checked_mul(other.denominator)
            .expect(OVERFLOW);
        let left = self
            .numerator
            .checked_mul(denominator / self.denominator)
            .expect(OVERFLOW);
        let right = other
            .numerator
            .checked_mul(denominator / other.denominator)
            .expect(OVERFLOW);
        Chance::new(left.checked_add(right).expect(OVERFLOW), denominator)
    }

    fn times(self, other: Chance) -> Chance {
        const OVERFLOW: &str = "a chance's fraction overflowed u128";
        Chance::new(
            self.numerator.checked_mul(other.numerator).expect(OVERFLOW),
            self.denominator
                .checked_mul(other.denominator)
                .expect(OVERFLOW),
        )
    }

    /// Whether it's 1.
    pub fn is_certain(self) -> bool {
        self.numerator == self.denominator
    }

    /// As a float, for display and sorting.
    #[expect(
        clippy::cast_precision_loss,
        reason = "an approximation, for display; the fraction is exact"
    )]
    pub fn as_f64(self) -> f64 {
        self.numerator as f64 / self.denominator as f64
    }
}

fn gcd(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

/// One flower a block can grow, and its chance given that bone meal makes a flower there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FlowerOdds {
    /// The state.
    pub state: FlowerState,
    /// Its chance.
    pub chance: Chance,
}

/// A provider's noise, in its version's form. The form also decides the arithmetic around it:
/// 26.2's providers work in `double`, 26.3's in `float`, where its noise returns a `float`.
#[derive(Debug, Clone)]
enum ProviderNoise {
    /// 26.2's `NormalNoise`, whose `getValue` is a `double`.
    V1_21(v1_21::NormalNoise),
    /// 26.3's, whose `get` is a `float`.
    V26_3(NoiseStack),
}

/// A noise's value at a block, in its version's type.
#[derive(Debug, Clone, Copy)]
enum NoiseValue {
    Double(f64),
    Float(f32),
}

impl NoiseValue {
    fn widened(self) -> f64 {
        match self {
            NoiseValue::Double(v) => v,
            NoiseValue::Float(v) => f64::from(v),
        }
    }

    /// `NoiseProvider.getRandomState(list, value)`: the index into a list of `len` states,
    /// from `(1 + value) / 2` clamped to `[0, 0.9999]`.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss,
        reason = "Java's i2f, f2i and d2i: the index is within the list"
    )]
    fn index(self, len: usize) -> usize {
        match self {
            NoiseValue::Double(v) => {
                let t = clamp_f64((1.0 + v) / 2.0, 0.0, 0.9999);
                (t * len as f64) as i32 as usize
            }
            NoiseValue::Float(v) => {
                let t = clamp_f32((1.0f32 + v) / 2.0f32, 0.0, 0.9999f32);
                (t * len as f32) as i32 as usize
            }
        }
    }
}

/// `Mth.clamp(double, double, double)`: `min` below it, else `Math.min(value, max)`. They
/// differ only for NaN, which no noise gives.
fn clamp_f64(value: f64, min: f64, max: f64) -> f64 {
    if value < min {
        min
    } else if value > max {
        max
    } else {
        value
    }
}

/// `Mth.clamp(float, float, float)` (26.3) and `Math.clamp` (26.4), which agree but for NaN.
fn clamp_f32(value: f32, min: f32, max: f32) -> f32 {
    if value < min {
        min
    } else if value > max {
        max
    } else {
        value
    }
}

impl ProviderNoise {
    /// `NormalNoise.create(new WorldgenRandom(new LegacyRandomSource(seed)), parameters)`. The
    /// noise forks positional factories from the source, which a `WorldgenRandom` hands to its
    /// inner source.
    fn create(
        parameters: &NormalNoiseParameters,
        seed: i64,
        at: &At<'_>,
    ) -> Result<Self, LoadError> {
        Ok(match parameters {
            NormalNoiseParameters::V1_21(p) => ProviderNoise::V1_21(v1_21::NormalNoise::create(
                &mut LegacyRandomSource::new(seed),
                p,
            )),
            NormalNoiseParameters::V26_3(p) => {
                let noise = NormalNoise::from_parameters(p)
                    .map_err(|e| at.error(LoadErrorKind::Invalid(e.reason())))?;
                ProviderNoise::V26_3(noise.create(&mut LegacyRandomSource::new(seed)))
            }
        })
    }

    /// `NoiseBasedStateProvider.getNoiseValue(pos, scale)`: the noise at each coordinate times
    /// `scale`, multiplied in `double`.
    fn at(&self, [x, y, z]: [i32; 3], scale: f32) -> NoiseValue {
        let scale = f64::from(scale);
        let (x, y, z) = (
            f64::from(x) * scale,
            f64::from(y) * scale,
            f64::from(z) * scale,
        );
        match self {
            ProviderNoise::V1_21(n) => NoiseValue::Double(n.get_value(x, y, z)),
            ProviderNoise::V26_3(n) => NoiseValue::Float(n.get(x, y, z)),
        }
    }

    /// `DualNoiseProvider.getSlowNoiseValue(pos)`: the same, but each coordinate is multiplied
    /// by the scale in `float`, then widened.
    #[expect(
        clippy::cast_precision_loss,
        reason = "Java's i2f: the game multiplies in float"
    )]
    fn slow_at(&self, [x, y, z]: [i32; 3], scale: f32) -> NoiseValue {
        let c = |v: i32| f64::from(v as f32 * scale);
        let (x, y, z) = (c(x), c(y), c(z));
        match self {
            ProviderNoise::V1_21(n) => NoiseValue::Double(n.get_value(x, y, z)),
            ProviderNoise::V26_3(n) => NoiseValue::Float(n.get(x, y, z)),
        }
    }
}

/// A block state provider (`BlockStateProvider`), of the kinds vanilla's flower features use.
#[derive(Debug, Clone)]
enum Provider {
    /// One state (26.3's bare state, 26.2's `simple_state_provider`).
    Single(FlowerState),
    /// `weighted`: a state by weight.
    Weighted(Weighted<FlowerState>),
    /// `noise`: a state from the list by the noise at the block.
    Noise {
        noise: ProviderNoise,
        scale: f32,
        states: Vec<FlowerState>,
    },
    /// `noise_threshold`: a random low state below the threshold, else a random high state
    /// with `high_chance`, else the default.
    Threshold {
        noise: ProviderNoise,
        scale: f32,
        threshold: f32,
        high_chance: f32,
        default: FlowerState,
        low: Vec<FlowerState>,
        high: Vec<FlowerState>,
    },
    /// `dual_noise`: the slow noise picks how many states, and which, from `states`; the main
    /// noise picks among those.
    Dual {
        noise: ProviderNoise,
        scale: f32,
        slow_noise: ProviderNoise,
        slow_scale: f32,
        /// `variety`: the fewest and most states the slow noise picks.
        variety: (i32, i32),
        states: Vec<FlowerState>,
    },
}

impl Provider {
    /// The state a provider that never draws picks at `pos`: a single state, `noise` and
    /// `dual_noise`. `None` for the others.
    fn by_position(&self, pos: [i32; 3]) -> Option<FlowerState> {
        match self {
            Provider::Single(s) => Some(*s),
            Provider::Noise {
                noise,
                scale,
                states,
            } => Some(states[noise.at(pos, *scale).index(states.len())]),
            Provider::Dual { noise, scale, .. } => {
                let picked = self.dual_states(pos);
                Some(picked[noise.at(pos, *scale).index(picked.len())])
            }
            Provider::Weighted(_) | Provider::Threshold { .. } => None,
        }
    }

    /// `getState(level, random, pos)`, with the game's draws in its order.
    fn state(&self, pos: [i32; 3], random: &mut impl RandomSource) -> FlowerState {
        match self {
            Provider::Weighted(w) => *w.pick(random),
            Provider::Threshold {
                noise,
                scale,
                threshold,
                high_chance,
                default,
                low,
                high,
            } => {
                if noise.at(pos, *scale).widened() < f64::from(*threshold) {
                    *get_random(low, random)
                } else if random.next_float() < *high_chance {
                    *get_random(high, random)
                } else {
                    *default
                }
            }
            Provider::Single(_) | Provider::Noise { .. } | Provider::Dual { .. } => {
                self.by_position(pos).expect("these pick by position")
            }
        }
    }

    /// `dual_noise`'s states at `pos`, from the slow noise: how many by `clampedMap` of its
    /// value, and each by its value at a far-off offset.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "Java's d2i: the count is within the variety"
    )]
    fn dual_states(&self, pos: [i32; 3]) -> Vec<FlowerState> {
        let Provider::Dual {
            slow_noise,
            slow_scale,
            variety: (min, max),
            states,
            ..
        } = self
        else {
            unreachable!("only dual_noise has slow states")
        };
        let slow = slow_noise.slow_at(pos, *slow_scale).widened();
        // Mth.clampedMap(slow, -1, 1, min, max + 1): clampedLerp(inverseLerp(...)).
        let t = (slow - -1.0) / (1.0 - -1.0);
        let count = clamped_lerp(t, f64::from(*min), f64::from(*max + 1)) as i32;
        (0..count)
            .map(|j| {
                let offset = [
                    pos[0].wrapping_add(j.wrapping_mul(54545)),
                    pos[1],
                    pos[2].wrapping_add(j.wrapping_mul(34234)),
                ];
                states[slow_noise.slow_at(offset, *slow_scale).index(states.len())]
            })
            .collect()
    }

    /// Each state's chance at `pos`, summed over repeats.
    fn odds(&self, pos: [i32; 3]) -> BTreeMap<FlowerState, Chance> {
        let mut odds = BTreeMap::new();
        let mut add = |state: FlowerState, chance: Chance| {
            let entry = odds.entry(state).or_insert(Chance::new(0, 1));
            *entry = entry.add(chance);
        };
        let uniform =
            |list: &[FlowerState], times: Chance, add: &mut dyn FnMut(FlowerState, Chance)| {
                for &s in list {
                    add(s, times.times(Chance::new(1, list.len() as u128)));
                }
            };
        match self {
            Provider::Single(_) | Provider::Noise { .. } | Provider::Dual { .. } => {
                add(
                    self.by_position(pos).expect("these pick by position"),
                    Chance::ONE,
                );
            }
            Provider::Weighted(w) => {
                let total = u128::try_from(w.total()).expect("a positive total");
                for (&s, weight) in w.weighted_entries() {
                    let weight = u128::try_from(weight).expect("weights are at least 0");
                    if weight > 0 {
                        add(s, Chance::new(weight, total));
                    }
                }
            }
            Provider::Threshold {
                noise,
                scale,
                threshold,
                high_chance,
                default,
                low,
                high,
            } => {
                if noise.at(pos, *scale).widened() < f64::from(*threshold) {
                    uniform(low, Chance::ONE, &mut add);
                } else {
                    let high_steps = float_steps_below(*high_chance);
                    let high_odds = Chance::new(high_steps, FLOAT_STEPS);
                    uniform(high, high_odds, &mut add);
                    if high_steps < FLOAT_STEPS {
                        add(*default, Chance::new(FLOAT_STEPS - high_steps, FLOAT_STEPS));
                    }
                }
            }
        }
        odds
    }
}

/// How many of `nextFloat()`'s values, the multiples of 2⁻²⁴ in `[0, 1)`, are below `chance`:
/// `⌈chance · 2²⁴⌉`, which is exact in `f64`, between 0 and 2²⁴.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the product is an exact integer-or-fraction between 0 and 2^24"
)]
fn float_steps_below(chance: f32) -> u128 {
    let scaled = f64::from(chance) * 16_777_216.0;
    if scaled <= 0.0 {
        return 0;
    }
    let whole = scaled as u128;
    let steps = if (whole as f64) < scaled {
        whole + 1
    } else {
        whole
    };
    steps.min(FLOAT_STEPS)
}

/// `Util.getRandom(list, random)`: `list.get(nextInt(list.size()))`.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "lists are short; nextInt's bound is the list's int size"
)]
fn get_random<'a, T>(list: &'a [T], random: &mut impl RandomSource) -> &'a T {
    &list[random.next_int_bounded(list.len() as i32) as usize]
}

/// A feature in the bone meal tag that a biome can pick.
#[derive(Debug)]
pub struct BoneMealFeature {
    /// Its id, e.g. `minecraft:flower_plain`.
    pub id: String,
    /// Its provider's type, e.g. `minecraft:noise_threshold`, for saying why.
    pub provider_type: String,
    /// What it places, or why spyglass can't say.
    provider: Result<Provider, Arc<LoadError>>,
}

impl BoneMealFeature {
    /// Whether what it places depends on where, not only on chance.
    pub fn picks_by_position(&self) -> bool {
        matches!(
            self.provider,
            Ok(Provider::Noise { .. } | Provider::Threshold { .. } | Provider::Dual { .. })
        )
    }

    /// Why spyglass can't answer for it, if it can't.
    pub fn error(&self) -> Option<&Arc<LoadError>> {
        self.provider.as_ref().err()
    }
}

/// Every biome's bone meal features, and the states they place.
#[derive(Debug)]
pub struct Flowers {
    /// The states the features can place, by [`FlowerState`].
    states: Vec<String>,
    /// The features some biome can pick, each once.
    features: Vec<BoneMealFeature>,
    /// By biome id: the biome's bone meal features, as indexes into `features`, in the game's
    /// order and with its repeats.
    by_biome: Vec<Vec<usize>>,
}

impl Flowers {
    /// Each biome's bone meal features (`BiomeGenerationSettings.getBoneMealFeatures`): the
    /// registered features in the bone meal tag that its placed features reach.
    pub fn load<R: Resources + ?Sized>(
        resources: &R,
        settings: &FeatureSettings,
    ) -> Result<Flowers, LoadError> {
        let tag: Vec<String> = match spyglass_density::tag(resources, FEATURE_TAGS, BONE_MEAL_TAG) {
            Ok(tag) => tag,
            Err(_) => spyglass_density::tag(resources, CONFIGURED_FEATURE_TAGS, BONE_MEAL_TAG)?,
        };
        let defaults = BlockDefaults::load(resources)?;
        let mut loader = Loader {
            resources,
            defaults: &defaults,
            states: Vec::new(),
            state_index: HashMap::new(),
        };
        let mut features: Vec<BoneMealFeature> = Vec::new();
        let mut feature_index: HashMap<String, usize> = HashMap::new();
        // A placed feature's reachable bone meal features, found once.
        let mut by_placed: HashMap<usize, Vec<usize>> = HashMap::new();
        let mut by_biome = Vec::with_capacity(settings.biome_count());
        for biome in 0..settings.biome_count() {
            let id = BiomeId(u16::try_from(biome).expect("biome ids fit a u16"));
            let mut list = Vec::new();
            for &placed in settings.lists(id).iter().flatten() {
                if let std::collections::hash_map::Entry::Vacant(slot) = by_placed.entry(placed) {
                    let json = &settings.placed(placed).json;
                    let at = At::root(PLACED_FEATURE);
                    let mut reached = Vec::new();
                    reachable(resources, json, &at, &mut reached, 0)?;
                    let mut found = Vec::new();
                    for feature in reached {
                        if !tag.contains(&feature) {
                            continue;
                        }
                        let index = match feature_index.get(&feature) {
                            Some(&i) => i,
                            None => {
                                let loaded = loader.feature(&feature);
                                features.push(loaded);
                                feature_index.insert(feature, features.len() - 1);
                                features.len() - 1
                            }
                        };
                        found.push(index);
                    }
                    slot.insert(found);
                }
                list.extend_from_slice(&by_placed[&placed]);
            }
            by_biome.push(list);
        }
        Ok(Flowers {
            states: loader.states,
            features,
            by_biome,
        })
    }

    /// A state's name: its block id, then its non-default properties in brackets.
    pub fn state_name(&self, state: FlowerState) -> &str {
        &self.states[usize::from(state)]
    }

    /// Every state a feature can place.
    pub fn states(&self) -> impl Iterator<Item = (FlowerState, &str)> {
        self.states
            .iter()
            .enumerate()
            .map(|(i, s)| (FlowerState::try_from(i).expect("few states"), s.as_str()))
    }

    /// The bone meal features some biome can pick.
    pub fn features(&self) -> &[BoneMealFeature] {
        &self.features
    }

    /// A biome's bone meal features, as indexes into [`Flowers::features`], in the game's
    /// order and with its repeats: bone meal picks one of these uniformly.
    pub fn biome_features(&self, biome: BiomeId) -> &[usize] {
        &self.by_biome[usize::from(biome.0)]
    }

    /// The states a feature can place anywhere, for whether a biome can grow a flower at all.
    pub fn feature_states(&self, feature: usize) -> Vec<FlowerState> {
        match &self.features[feature].provider {
            Err(_) => Vec::new(),
            Ok(Provider::Single(s)) => vec![*s],
            Ok(Provider::Weighted(w)) => w.entries().copied().collect(),
            Ok(Provider::Noise { states, .. } | Provider::Dual { states, .. }) => states.clone(),
            Ok(Provider::Threshold {
                default, low, high, ..
            }) => {
                let mut all = vec![*default];
                all.extend(low);
                all.extend(high);
                all
            }
        }
    }

    /// `getState` of feature `feature`'s provider at `pos`, with `random`'s draws in the game's
    /// order, or why spyglass can't say. The oracle compares this draw for draw.
    pub fn draw(
        &self,
        feature: usize,
        pos: [i32; 3],
        random: &mut impl RandomSource,
    ) -> Result<FlowerState, Arc<LoadError>> {
        let provider = self.features[feature]
            .provider
            .as_ref()
            .map_err(Arc::clone)?;
        Ok(provider.state(pos, random))
    }

    /// Feature `feature`'s provider's odds at `pos`, on its own.
    pub fn provider_odds(
        &self,
        feature: usize,
        pos: [i32; 3],
    ) -> Result<Vec<FlowerOdds>, Arc<LoadError>> {
        let provider = self.features[feature]
            .provider
            .as_ref()
            .map_err(Arc::clone)?;
        Ok(provider
            .odds(pos)
            .into_iter()
            .map(|(state, chance)| FlowerOdds { state, chance })
            .collect())
    }

    /// The flowers bone meal can grow at `pos` in `biome`, with their chances given that it
    /// makes a flower: each of the biome's features with chance `1/n`, times its own odds.
    /// Empty where the biome has no bone meal features. The error of the first feature
    /// spyglass can't answer for, if any.
    pub fn odds(&self, biome: BiomeId, pos: [i32; 3]) -> Result<Vec<FlowerOdds>, Arc<LoadError>> {
        let list = self.biome_features(biome);
        let mut total: BTreeMap<FlowerState, Chance> = BTreeMap::new();
        let pick = Chance::new(1, list.len().max(1) as u128);
        for &feature in list {
            let provider = self.features[feature]
                .provider
                .as_ref()
                .map_err(Arc::clone)?;
            for (state, chance) in provider.odds(pos) {
                let entry = total.entry(state).or_insert(Chance::new(0, 1));
                *entry = entry.add(pick.times(chance));
            }
        }
        Ok(total
            .into_iter()
            .map(|(state, chance)| FlowerOdds { state, chance })
            .collect())
    }
}

/// The configured features `PlacedFeature.getFeatures` streams for a placed feature, by id, in
/// order: its feature, then the features nested in that feature's configuration, depth first.
/// Inline features have no id and are left out, though what they nest is not.
fn reachable<R: Resources + ?Sized>(
    resources: &R,
    placed: &Value,
    at: &At<'_>,
    out: &mut Vec<String>,
    depth: u32,
) -> Result<(), LoadError> {
    if depth > MAX_DEPTH {
        return Err(at.error(LoadErrorKind::Invalid("features nested too deep")));
    }
    let owned;
    let placed = match placed {
        Value::String(id) => {
            let id = full_id(id);
            owned = resources
                .resource(PLACED_FEATURE, &id)
                .map_err(|e| At::root(&id).error(e.into()))?;
            &owned
        }
        other => other,
    };
    let fields = object(placed, at)?;
    let feature = field(fields, "feature", at)?;
    let owned;
    let feature = match feature {
        Value::String(id) => {
            let id = full_id(id);
            owned = configured(resources, &id)?;
            out.push(id);
            &owned
        }
        other => other,
    };
    let at = at.child("feature");
    let fields = object(feature, &at)?;
    let kind = fields
        .get("type")
        .and_then(Value::as_str)
        .map(full_id)
        .unwrap_or_default();
    // 26.2 nests the configuration under `config`.
    let config: &Map<String, Value> = match fields.get("config") {
        Some(Value::Object(config)) => config,
        _ => fields,
    };
    let mut nested = |json: &Value, at: &At<'_>| reachable(resources, json, at, out, depth + 1);
    // The features each selector's `getSubFeatures` streams, in its order.
    match kind.strip_prefix("minecraft:").unwrap_or(&kind) {
        "random_selector" => {
            for (i, entry) in list(config, "features").iter().enumerate() {
                let at = at.child("features").child(&i.to_string());
                nested(field(object(entry, &at)?, "feature", &at)?, &at)?;
            }
            nested(field(config, "default", &at)?, &at.child("default"))?;
        }
        "random_boolean_selector" => {
            nested(
                field(config, "feature_true", &at)?,
                &at.child("feature_true"),
            )?;
            nested(
                field(config, "feature_false", &at)?,
                &at.child("feature_false"),
            )?;
        }
        "simple_random_selector" | "sequence" | "overlay" => {
            for (i, entry) in list(config, "features").iter().enumerate() {
                nested(entry, &at.child("features").child(&i.to_string()))?;
            }
        }
        "weighted_random_selector" => {
            for (i, entry) in list(config, "features").iter().enumerate() {
                let at = at.child("features").child(&i.to_string());
                nested(field(object(entry, &at)?, "data", &at)?, &at)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// A field that's a list, or one entry on its own (a holder set's single id), or nothing.
fn list<'v>(config: &'v Map<String, Value>, key: &str) -> Vec<&'v Value> {
    match config.get(key) {
        Some(Value::Array(items)) => items.iter().collect(),
        Some(one) => vec![one],
        None => Vec::new(),
    }
}

/// A configured feature's JSON, from 26.3's registry or 26.2's.
fn configured<R: Resources + ?Sized>(resources: &R, id: &str) -> Result<Value, LoadError> {
    resources
        .resource(FEATURE, id)
        .or_else(|_| resources.resource(CONFIGURED_FEATURE, id))
        .map_err(|e| At::root(id).error(e.into()))
}

/// Reads flower features' providers, naming each state once.
struct Loader<'a, R: ?Sized> {
    resources: &'a R,
    defaults: &'a BlockDefaults,
    states: Vec<String>,
    state_index: HashMap<String, FlowerState>,
}

impl<R: Resources + ?Sized> Loader<'_, R> {
    /// A feature in the bone meal tag. Only `simple_block` features place a state; any other,
    /// or a provider spyglass doesn't read, is kept with its error.
    fn feature(&mut self, id: &str) -> BoneMealFeature {
        let mut provider_type = String::new();
        let provider = (|| {
            let json = configured(self.resources, id)?;
            let at = At::root(id);
            let fields = object(&json, &at)?;
            let kind = fields.get("type").and_then(Value::as_str).map(full_id);
            if kind.as_deref() != Some("minecraft:simple_block") {
                return Err(at.child("type").error(LoadErrorKind::Unsupported {
                    what: "bone meal feature",
                    name: kind.unwrap_or_default(),
                }));
            }
            let config: &Map<String, Value> = match fields.get("config") {
                Some(Value::Object(config)) => config,
                _ => fields,
            };
            let at = at.child("to_place");
            self.provider(field(config, "to_place", &at)?, &at, &mut provider_type)
        })();
        BoneMealFeature {
            id: id.to_string(),
            provider_type,
            provider: provider.map_err(Arc::new),
        }
    }

    /// A block state provider: 26.3's registry id, a bare state, or one with a `type`.
    fn provider(
        &mut self,
        json: &Value,
        at: &At<'_>,
        kind_out: &mut String,
    ) -> Result<Provider, LoadError> {
        let owned;
        let json = match json {
            // A provider's holder: an id in 26.3's registry.
            Value::String(id) => {
                let id = full_id(id);
                owned = self
                    .resources
                    .resource(BLOCK_STATE_PROVIDER, &id)
                    .map_err(|e| At::root(&id).error(e.into()))?;
                &owned
            }
            other => other,
        };
        let o = object(json, at)?;
        let Some(kind) = o.get("type").and_then(Value::as_str).map(full_id) else {
            *kind_out = "a single state".to_string();
            return Ok(Provider::Single(self.state(json, at)?));
        };
        *kind_out = kind.clone();
        // 26.2's types end in `_provider`.
        let name = kind.strip_prefix("minecraft:").unwrap_or(&kind);
        let name = name.strip_suffix("_provider").unwrap_or(name);
        Ok(match name {
            "simple_state" => {
                Provider::Single(self.state(field(o, "state", at)?, &at.child("state"))?)
            }
            "weighted" | "weighted_state" => {
                Provider::Weighted(Weighted::parse_field(o, "entries", at, |v, at| {
                    self.state(v, at)
                })?)
            }
            "noise" => {
                let (noise, scale) = self.noise(o, "noise", "scale", at)?;
                Provider::Noise {
                    noise,
                    scale,
                    states: self.states_in(o, "states", at)?,
                }
            }
            "noise_threshold" => {
                let (noise, scale) = self.noise(o, "noise", "scale", at)?;
                Provider::Threshold {
                    noise,
                    scale,
                    threshold: float_in(o, "threshold", -1.0, 1.0, at)?,
                    high_chance: float_in(o, "high_chance", 0.0, 1.0, at)?,
                    default: self
                        .state(field(o, "default_state", at)?, &at.child("default_state"))?,
                    low: self.states_in(o, "low_states", at)?,
                    high: self.states_in(o, "high_states", at)?,
                }
            }
            "dual_noise" => {
                let (noise, scale) = self.noise(o, "noise", "scale", at)?;
                let (slow_noise, slow_scale) = self.noise(o, "slow_noise", "slow_scale", at)?;
                Provider::Dual {
                    noise,
                    scale,
                    slow_noise,
                    slow_scale,
                    variety: variety(field(o, "variety", at)?, &at.child("variety"))?,
                    states: self.states_in(o, "states", at)?,
                }
            }
            _ => {
                return Err(at.child("type").error(LoadErrorKind::Unsupported {
                    what: "block state provider",
                    name: kind,
                }));
            }
        })
    }

    /// A noise and its scale, seeded from the provider's own `seed`.
    fn noise(
        &self,
        o: &Map<String, Value>,
        noise_key: &'static str,
        scale_key: &'static str,
        at: &At<'_>,
    ) -> Result<(ProviderNoise, f32), LoadError> {
        let seed_at = at.child("seed");
        let seed = field(o, "seed", at)?
            .as_i64()
            .ok_or_else(|| seed_at.error(LoadErrorKind::Shape("a long")))?;
        let noise_at = at.child(noise_key);
        let parameters = parse_noise(field(o, noise_key, at)?, &noise_at)?;
        let scale_at = at.child(scale_key);
        let scale = float(field(o, scale_key, at)?, &scale_at)?;
        // ExtraCodecs.POSITIVE_FLOAT.
        if scale.is_nan() || scale <= 0.0 {
            return Err(scale_at.error(LoadErrorKind::Invalid("the scale must be above 0")));
        }
        Ok((ProviderNoise::create(&parameters, seed, &noise_at)?, scale))
    }

    /// A non-empty list of states.
    fn states_in(
        &mut self,
        o: &Map<String, Value>,
        key: &'static str,
        at: &At<'_>,
    ) -> Result<Vec<FlowerState>, LoadError> {
        let at = at.child(key);
        let items = field(o, key, &at)?
            .as_array()
            .ok_or_else(|| at.error(LoadErrorKind::Shape("a list of block states")))?;
        if items.is_empty() {
            return Err(at.error(LoadErrorKind::Invalid("the list must not be empty")));
        }
        items
            .iter()
            .enumerate()
            .map(|(i, item)| self.state(item, &at.child(&i.to_string())))
            .collect()
    }

    /// A block state, named as the game's block files name it.
    fn state(&mut self, json: &Value, at: &At<'_>) -> Result<FlowerState, LoadError> {
        let empty = Map::new();
        let (id, properties) = block_state(json, at)?;
        let name = self.defaults.name(&id, properties.unwrap_or(&empty), at)?;
        if let Some(&s) = self.state_index.get(&name) {
            return Ok(s);
        }
        let s = FlowerState::try_from(self.states.len())
            .map_err(|_| at.error(LoadErrorKind::Invalid("too many flower states")))?;
        self.states.push(name.clone());
        self.state_index.insert(name, s);
        Ok(s)
    }
}

/// A float field within `[min, max]` (`Codec.floatRange`).
fn float_in(
    o: &Map<String, Value>,
    key: &'static str,
    min: f32,
    max: f32,
    at: &At<'_>,
) -> Result<f32, LoadError> {
    let at = at.child(key);
    let value = float(field(o, key, &at)?, &at)?;
    if !(min..=max).contains(&value) {
        return Err(at.error(LoadErrorKind::OutOfRange {
            value: value.to_string(),
            min: min.to_string(),
            max: max.to_string(),
        }));
    }
    Ok(value)
}

/// `variety`: `InclusiveRange.codec(INT, 1, 64)`, a `[min, max]` pair with `min ≤ max`, both
/// within 1 to 64.
fn variety(json: &Value, at: &At<'_>) -> Result<(i32, i32), LoadError> {
    let pair = json
        .as_array()
        .filter(|a| a.len() == 2)
        .ok_or_else(|| at.error(LoadErrorKind::Shape("a [min, max] pair")))?;
    let min = int(&pair[0], &at.child("0"))?;
    let max = int(&pair[1], &at.child("1"))?;
    if !(1..=64).contains(&min) || !(1..=64).contains(&max) || min > max {
        return Err(at.error(LoadErrorKind::Invalid(
            "the variety must be min ≤ max, both from 1 to 64",
        )));
    }
    Ok((min, max))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chances_add_and_multiply_exactly() {
        let third = Chance::new(1, 3);
        let sixth = Chance::new(1, 6);
        assert_eq!(third.add(sixth), Chance::new(1, 2));
        assert_eq!(third.times(Chance::new(3, 4)), Chance::new(1, 4));
        assert!(third.add(third).add(third).is_certain());
        assert_eq!(
            Chance::new(4, 8),
            Chance {
                numerator: 1,
                denominator: 2
            }
        );
    }

    #[test]
    fn float_steps_count_next_floats_below_a_chance() {
        assert_eq!(float_steps_below(0.0), 0);
        assert_eq!(float_steps_below(1.0), FLOAT_STEPS);
        // 0.5 is exactly 2^23 steps: next_float < 0.5 for k < 2^23.
        assert_eq!(float_steps_below(0.5), 1 << 23);
        // The plains' 0.33333334f is 5592405.5 steps, so 5592406 values are below it.
        assert_eq!(float_steps_below(0.333_333_34), 5_592_406);
    }

    #[test]
    fn noise_picks_clamp_into_the_list() {
        assert_eq!(NoiseValue::Double(-2.0).index(4), 0);
        assert_eq!(NoiseValue::Double(2.0).index(4), 3);
        assert_eq!(NoiseValue::Float(0.0).index(4), 2);
        assert_eq!(NoiseValue::Float(0.999).index(11), 10);
    }
}
