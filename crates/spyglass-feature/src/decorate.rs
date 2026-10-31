//! `ChunkGenerator.applyBiomeDecoration`'s tries: the positions each placed feature hands its
//! feature, in decoration order (`docs/specs/features.md#tries`), with or without the blocks.

use spyglass_biome::BiomeId;
use spyglass_blocks::ChunkGenerator;
use spyglass_data::anchor::AnchorHeights;
use spyglass_data::codec::{At, LoadError, object};
use spyglass_rng::{RandomSource, WorldgenRandom};

use crate::modifier::{Modifier, Read, Reading};
use crate::order::PlacedRef;
use crate::region::Region;
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
    /// modifier that reads blocks without the blocks (the position is then where that modifier
    /// was reached), or after an earlier try of the same placed feature whose draws aren't
    /// known.
    pub exact: bool,
    /// Whether the try is the game's whatever the lookup history: false where the try's biome
    /// is tied and the candidates disagree, or a modifier read a block that depends on a tie.
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

/// The feature at each try: with the region when decorating on blocks.
type Feature<'f, 'b> = dyn FnMut(&Try, &mut WorldgenRandom, Option<&mut Region<'b>>) -> Drawn + 'f;

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
    /// vertical anchors, and `level_min_y` the level's lowest y. With `reading`, the modifiers
    /// that read blocks are loaded, for [`Decorator::decorate`].
    pub fn new(
        settings: &'a FeatureSettings,
        generator: &'a ChunkGenerator<'a>,
        seed: i64,
        anchors: AnchorHeights,
        level_min_y: i32,
        wanted: &[PlacedRef],
    ) -> Result<Self, LoadError> {
        Self::with_reading(
            settings,
            generator,
            seed,
            anchors,
            level_min_y,
            wanted,
            None,
        )
    }

    /// As [`Decorator::new`], loading the modifiers that read blocks with `reading`.
    pub fn with_reading(
        settings: &'a FeatureSettings,
        generator: &'a ChunkGenerator<'a>,
        seed: i64,
        anchors: AnchorHeights,
        level_min_y: i32,
        wanted: &[PlacedRef],
        mut reading: Option<&mut Reading<'_>>,
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
                    Some(json) => {
                        Modifier::parse_list(json, &at.child("placement"), reading.as_deref_mut())?
                    }
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

    /// The tries in chunk `(cx, cz)`, in decoration order, without the blocks. `feature` is
    /// called at each exact, certain try, and draws from the random what the feature would.
    pub fn tries(
        &self,
        chunk: [i32; 2],
        feature: &mut dyn FnMut(&Try, &mut WorldgenRandom) -> Drawn,
    ) -> Vec<Try> {
        let mut call =
            |t: &Try, random: &mut WorldgenRandom, _: Option<&mut Region<'_>>| feature(t, random);
        self.run(chunk, None, &mut call)
    }

    /// The tries in the region's chunk, in decoration order, on its blocks: the modifiers that
    /// read blocks read the region as each feature leaves it. `feature` is called at every try
    /// with the region, exact or not, and places what it does.
    pub fn decorate<'b>(
        &self,
        region: &mut Region<'b>,
        feature: &mut dyn FnMut(&Try, &mut WorldgenRandom, &mut Region<'b>) -> Drawn,
    ) -> Vec<Try> {
        let chunk = region.center();
        let mut call =
            |t: &Try, random: &mut WorldgenRandom, region: Option<&mut Region<'b>>| match region {
                Some(region) => feature(t, random, region),
                None => Drawn::Unknown,
            };
        self.run(chunk, Some(region), &mut call)
    }

    fn run<'b>(
        &self,
        [cx, cz]: [i32; 2],
        mut region: Option<&mut Region<'b>>,
        feature: &mut Feature<'_, 'b>,
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
                    region: region.as_deref_mut(),
                    feature: &mut *feature,
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
struct Run<'r, 'a, 'f, 'b> {
    decorator: &'r Decorator<'a>,
    random: &'r mut WorldgenRandom,
    region: Option<&'r mut Region<'b>>,
    feature: &'r mut Feature<'f, 'b>,
    out: &'r mut Vec<Try>,
    placed: PlacedRef,
    step: usize,
    index: usize,
    /// False once a try's draws aren't known.
    exact: bool,
    /// False while the position passed a check that a tie could change.
    certain: bool,
}

impl Run<'_, '_, '_, '_> {
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
                    self.with_certain(agree, |run| run.apply(rest, pos));
                }
            }
            Modifier::ReadsBlocks {
                read: Some(read), ..
            } if self.region.is_some() => self.read(read, rest, pos),
            Modifier::ReadsBlocks { .. } => self.reach(pos, false),
        }
    }

    /// `f` with certainty lowered unless `certain`, then restored.
    fn with_certain(&mut self, certain: bool, f: impl FnOnce(&mut Self)) {
        let before = self.certain;
        self.certain &= certain;
        f(self);
        self.certain = before;
    }

    /// A modifier that reads blocks, on the region (`docs/specs/features.md#modifiers`).
    fn read(&mut self, read: &Read, rest: &[Modifier], pos @ [x, y, z]: [i32; 3]) {
        let region = self
            .region
            .as_deref_mut()
            .expect("reading needs the region");
        let doubt_before = region.take_doubt();
        let next = match read {
            Read::Heightmap(h) => {
                let h = region.height(*h, x, z);
                (h > region.min_y()).then_some([x, h, z])
            }
            Read::EnvironmentScan {
                down,
                max_steps,
                target,
                allowed,
            } => environment_scan(region, pos, *down, *max_steps, target, allowed),
            Read::SurfaceRelativeThreshold {
                heightmap,
                min,
                max,
            } => {
                let h = i64::from(region.height(*heightmap, x, z));
                let (lo, hi) = (h + i64::from(*min), h + i64::from(*max));
                (lo <= i64::from(y) && i64::from(y) <= hi).then_some(pos)
            }
            Read::Filter(p) => p.test(region, pos).then_some(pos),
        };
        let doubt = region.take_doubt();
        if let Some(why) = doubt_before {
            region.doubt(why);
        }
        // A position a tied block could have moved or kept out is a candidate try; one it
        // kept out is lost here, which only the caller's doubt about the chunk covers.
        if let Some(next) = next {
            self.with_certain(doubt.is_none(), |run| run.apply(rest, next));
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
        // On the blocks, every try reaches the feature, which knows from `exact` whether its
        // position is the game's.
        let drawn = match self.region.as_deref_mut() {
            Some(region) => (self.feature)(&t, self.random, Some(region)),
            None if known && self.certain => (self.feature)(&t, self.random, None),
            None => Drawn::Unknown,
        };
        if drawn == Drawn::Unknown {
            self.exact = false;
        }
    }
}

/// `EnvironmentScanPlacement`: from `pos`, the first position passing `target`, testing at most
/// `max_steps + 1`, while `allowed` holds and within the build height.
fn environment_scan(
    region: &mut Region<'_>,
    pos: [i32; 3],
    down: bool,
    max_steps: i32,
    target: &crate::predicate::BlockPredicate,
    allowed: &crate::predicate::BlockPredicate,
) -> Option<[i32; 3]> {
    let mut p = pos;
    if !allowed.test(region, p) {
        return None;
    }
    for _ in 0..max_steps {
        if target.test(region, p) {
            return Some(p);
        }
        p[1] = if down {
            p[1].wrapping_sub(1)
        } else {
            p[1].wrapping_add(1)
        };
        if region.outside_build_height(p[1]) {
            return None;
        }
        if !allowed.test(region, p) {
            break;
        }
    }
    target.test(region, p).then_some(p)
}
