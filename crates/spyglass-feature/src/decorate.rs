//! `ChunkGenerator.applyBiomeDecoration`'s tries: the positions each placed feature hands its
//! feature, in decoration order (`docs/specs/features.md#tries`).

use spyglass_biome::BiomeId;
use spyglass_blocks::ChunkGenerator;
use spyglass_data::anchor::AnchorHeights;
use spyglass_data::codec::{At, LoadError, object};
use spyglass_rng::{RandomSource, WorldgenRandom};

use crate::modifier::Modifier;
use crate::order::PlacedRef;
use crate::settings::FeatureSettings;

/// A position a placed feature hands its feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Try {
    /// The placed feature.
    pub placed: PlacedRef,
    /// Its decoration step.
    pub step: usize,
    /// Its index in the step's order, which its seed uses.
    pub index: usize,
    /// The position.
    pub pos: [i32; 3],
    /// Whether the position is the game's whatever decoration did before: false after a
    /// modifier that reads blocks (the position is then where that modifier was reached), or
    /// after an earlier try of the same placed feature whose draws aren't known.
    pub exact: bool,
    /// Whether the biome check is the game's whatever the lookup history: false where the
    /// try's biome is tied and the candidates disagree.
    pub certain: bool,
}

/// What a feature did with the random at a try, from the caller of [`Decorator::tries`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Drawn {
    /// The caller drew what the feature draws there.
    Known,
    /// Not known: the placed feature's later tries aren't exact.
    Unknown,
}

/// One seed's decoration of the placed features asked for.
pub struct Decorator<'a> {
    settings: &'a FeatureSettings,
    generator: &'a ChunkGenerator<'a>,
    seed: i64,
    anchors: AnchorHeights,
    /// The level's lowest section's lowest y: decoration's origin.
    origin_y: i32,
    /// The placed features asked for, with their modifiers.
    wanted: Vec<(PlacedRef, Vec<Modifier>)>,
}

impl<'a> Decorator<'a> {
    /// For `seed`'s `generator`, the placed features `wanted`. `anchors` are the generator's
    /// vertical anchors, and `level_min_y` the level's lowest y.
    pub fn new(
        settings: &'a FeatureSettings,
        generator: &'a ChunkGenerator<'a>,
        seed: i64,
        anchors: AnchorHeights,
        level_min_y: i32,
        wanted: &[PlacedRef],
    ) -> Result<Self, LoadError> {
        let wanted = wanted
            .iter()
            .map(|&r| {
                let placed = settings.placed(r);
                let name = placed
                    .id
                    .clone()
                    .unwrap_or_else(|| "an inline placed feature".into());
                let at = At::root(&name);
                let o = object(&placed.json, &at)?;
                let modifiers = match o.get("placement") {
                    None => Vec::new(),
                    Some(json) => Modifier::parse_list(json, &at.child("placement"))?,
                };
                Ok((r, modifiers))
            })
            .collect::<Result<_, LoadError>>()?;
        Ok(Decorator {
            settings,
            generator,
            seed,
            anchors,
            origin_y: level_min_y.div_euclid(16) * 16,
            wanted,
        })
    }

    /// The tries in chunk `(cx, cz)`, in decoration order. `feature` is called at each try
    /// that reaches its feature, and draws from the random what the feature would.
    pub fn tries(
        &self,
        [cx, cz]: [i32; 2],
        feature: &mut dyn FnMut(&Try, &mut WorldgenRandom) -> Drawn,
    ) -> Vec<Try> {
        let mut random = WorldgenRandom::new(0);
        let (x0, z0) = (cx.wrapping_shl(4), cz.wrapping_shl(4));
        let decoration = random.set_decoration_seed(self.seed, x0, z0);
        let mut out = Vec::new();
        for (step, order) in self.settings.order().iter().enumerate() {
            for (index, placed) in order.iter().enumerate() {
                let Some((_, modifiers)) = self.wanted.iter().find(|(r, _)| r == placed) else {
                    continue;
                };
                random.set_feature_seed(decoration, index as i32, step as i32);
                let mut run = Run {
                    decorator: self,
                    random: &mut random,
                    feature,
                    out: &mut out,
                    placed: *placed,
                    step,
                    index,
                    exact: true,
                    certain: true,
                };
                run.apply(modifiers, [x0, self.origin_y, z0]);
            }
        }
        out
    }

    /// Whether `biome` lists `placed` in any step (`BiomeGenerationSettings.hasFeature`).
    fn lists(&self, biome: BiomeId, placed: PlacedRef) -> bool {
        self.settings
            .lists(biome)
            .iter()
            .any(|list| list.contains(&placed))
    }
}

/// One placed feature's tries in a chunk.
struct Run<'r, 'a> {
    decorator: &'r Decorator<'a>,
    random: &'r mut WorldgenRandom,
    feature: &'r mut dyn FnMut(&Try, &mut WorldgenRandom) -> Drawn,
    out: &'r mut Vec<Try>,
    placed: PlacedRef,
    step: usize,
    index: usize,
    /// False once a try's draws aren't known.
    exact: bool,
    /// False while the position passed a biome check that a tie could change.
    certain: bool,
}

impl Run<'_, '_> {
    /// The rest of the modifiers on `pos`, depth first: each position goes through them all,
    /// and reaches the feature, before the next is made (`FeaturePlacer.place`).
    fn apply(&mut self, modifiers: &[Modifier], pos: [i32; 3]) {
        let Some((modifier, rest)) = modifiers.split_first() else {
            self.reach(pos, true);
            return;
        };
        let [x, y, z] = pos;
        match modifier {
            Modifier::Count(count) => {
                for _ in 0..count.sample(self.random) {
                    self.apply(rest, pos);
                }
            }
            Modifier::Rarity(chance) => {
                // fcmpg/ifge: NaN fails.
                if self.random.next_float() < 1.0 / *chance as f32 {
                    self.apply(rest, pos);
                }
            }
            Modifier::InSquare => {
                let dx = self.random.next_int_bounded(16);
                let dz = self.random.next_int_bounded(16);
                self.apply(rest, [dx.wrapping_add(x), y, dz.wrapping_add(z)]);
            }
            Modifier::HeightRange(height) => {
                let y = height.sample(self.random, self.decorator.anchors);
                self.apply(rest, [x, y, z]);
            }
            Modifier::Offset([ox, oy, oz]) => {
                let dx = ox.sample(self.random);
                let dy = oy.sample(self.random);
                let dz = oz.sample(self.random);
                self.apply(
                    rest,
                    [x.wrapping_add(dx), y.wrapping_add(dy), z.wrapping_add(dz)],
                );
            }
            Modifier::Biome => {
                let lookup = self.decorator.generator.block_biome(pos);
                let first = self.decorator.lists(lookup.biome(), self.placed);
                let agree = lookup
                    .candidates()
                    .iter()
                    .all(|&b| self.decorator.lists(b, self.placed) == first);
                let any = first || !agree;
                if any {
                    let certain = self.certain;
                    self.certain &= agree;
                    self.apply(rest, pos);
                    self.certain = certain;
                }
            }
            Modifier::ReadsBlocks(_) => self.reach(pos, false),
        }
    }

    /// A try reaches the feature at `pos`; `known` if every modifier on the way is.
    fn reach(&mut self, pos: [i32; 3], known: bool) {
        let t = Try {
            placed: self.placed,
            step: self.step,
            index: self.index,
            pos,
            exact: self.exact && known,
            certain: self.certain,
        };
        self.out.push(t);
        let drawn = if known && self.certain {
            (self.feature)(&t, self.random)
        } else {
            Drawn::Unknown
        };
        if drawn == Drawn::Unknown {
            self.exact = false;
        }
    }
}
