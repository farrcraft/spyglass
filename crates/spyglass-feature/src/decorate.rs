//! `ChunkGenerator.applyBiomeDecoration`'s tries: the positions each placed feature hands its
//! feature, in decoration order (`docs/specs/features.md#tries`), with or without the blocks.

use spyglass_biome::BiomeId;
use spyglass_blocks::ChunkGenerator;
use spyglass_data::anchor::AnchorHeights;
use spyglass_data::codec::{At, LoadError, object};
use spyglass_rng::{RandomSource, WorldgenRandom};

use crate::blocks::Heightmap;
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
    wanted: std::borrow::Cow<'a, [(PlacedRef, Vec<Modifier>)]>,
}

/// Placed features with their modifiers loaded, seed-independent: what a [`Decorator`] runs.
#[derive(Debug, Clone)]
pub struct Wanted(Vec<(PlacedRef, Vec<Modifier>)>);

impl Wanted {
    /// The modifiers of each of `wanted`. With `reading`, the modifiers that read blocks are
    /// loaded in full, for [`Decorator::decorate`]; without, only their shape.
    pub fn load(
        settings: &FeatureSettings,
        wanted: &[PlacedRef],
        mut reading: Option<&mut Reading<'_>>,
    ) -> Result<Wanted, LoadError> {
        wanted
            .iter()
            .map(|&placed_ref| {
                let placed = settings.placed(placed_ref);
                let name = placed
                    .id
                    .clone()
                    .unwrap_or_else(|| "an inline placed feature".into());
                let at = At::root(&name);
                let fields = object(&placed.json, &at)?;
                let modifiers = match fields.get("placement") {
                    None => Vec::new(),
                    Some(json) => {
                        Modifier::parse_list(json, &at.child("placement"), reading.as_deref_mut())?
                    }
                };
                Ok((placed_ref, modifiers))
            })
            .collect::<Result<_, LoadError>>()
            .map(Wanted)
    }

    /// The placed features, in the order asked for.
    pub fn placed(&self) -> impl Iterator<Item = PlacedRef> + '_ {
        self.0.iter().map(|(r, _)| *r)
    }
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
        reading: Option<&mut Reading<'_>>,
    ) -> Result<Self, LoadError> {
        let wanted = Wanted::load(settings, wanted, reading)?;
        Ok(Decorator {
            settings,
            generator,
            seed,
            anchors,
            origin_y: level_min_y.div_euclid(16) * 16,
            wanted: std::borrow::Cow::Owned(wanted.0),
        })
    }

    /// For `seed`'s `generator`, placed features loaded once ([`Wanted::load`]).
    pub fn of(
        settings: &'a FeatureSettings,
        generator: &'a ChunkGenerator<'a>,
        seed: i64,
        anchors: AnchorHeights,
        level_min_y: i32,
        wanted: &'a Wanted,
    ) -> Self {
        Decorator {
            settings,
            generator,
            seed,
            anchors,
            origin_y: level_min_y.div_euclid(16) * 16,
            wanted: std::borrow::Cow::Borrowed(&wanted.0),
        }
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
        let (corner_x, corner_z) = (cx.wrapping_shl(4), cz.wrapping_shl(4));
        let decoration = random.set_decoration_seed(self.seed, corner_x, corner_z);
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
                run.apply(modifiers, [corner_x, self.origin_y, corner_z]);
            }
        }
        out
    }

    /// How many times a noise count gives the position at `(x, z)`; `None` for another
    /// modifier.
    fn noise_count(&self, modifier: &Modifier, x: i32, z: i32) -> Option<i32> {
        let (x, z) = (f64::from(x), f64::from(z));
        Some(match *modifier {
            Modifier::NoiseBasedCount {
                ratio,
                factor,
                offset,
            } => {
                let v = self.generator.biome_info(x / factor, z / factor);
                ((v + offset) * f64::from(ratio)).ceil() as i32
            }
            Modifier::NoiseThresholdCount {
                level,
                below,
                above,
            } => {
                // dcmpg/ifge: NaN counts as above.
                if self.generator.biome_info(x / 200.0, z / 200.0) < level {
                    below
                } else {
                    above
                }
            }
            _ => return None,
        })
    }

    /// Whether `biome` lists `placed` in any step (`BiomeGenerationSettings.hasFeature`).
    fn lists(&self, biome: BiomeId, placed: PlacedRef) -> bool {
        self.settings
            .lists(biome)
            .iter()
            .any(|list| list.contains(&placed))
    }
}

/// Where a try can be when the blocks aren't known: a box of positions (`docs/specs/features.md#neighbours`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TryBox {
    /// The placed feature.
    pub placed: PlacedRef,
    /// Its decoration step.
    pub step: usize,
    /// Its index in the step's order.
    pub index: usize,
    /// The least corner.
    pub min: [i32; 3],
    /// The greatest corner.
    pub max: [i32; 3],
}

/// What boxes of tries need to know about a chunk they don't have the blocks of.
pub trait Ground {
    /// Whether `placed` runs in the chunk: a biome stored in it or its neighbours lists it.
    fn runs(&self, placed: PlacedRef) -> bool;
    /// The least and greatest first free y `heightmap` can give over the columns from `lo`
    /// to `hi` (x and z) when the chunk is decorated, whatever decorated before.
    fn heights(&self, heightmap: Heightmap, lo: [i32; 2], hi: [i32; 2]) -> (i32, i32);
}

impl Decorator<'_> {
    /// Every place a try of the placed features asked for can be in chunk `chunk`, whatever the
    /// blocks: the tries are run with each coordinate a range. Draws stay known until a try
    /// reaches its feature, which draws what its blocks decide; after that, each modifier
    /// gives every value it can. A modifier that reads blocks gives the range it can.
    pub fn try_boxes(
        &self,
        [cx, cz]: [i32; 2],
        min_y: i32,
        max_y: i32,
        ground: &dyn Ground,
    ) -> Vec<TryBox> {
        let mut random = WorldgenRandom::new(0);
        let (corner_x, corner_z) = (cx.wrapping_shl(4), cz.wrapping_shl(4));
        let decoration = random.set_decoration_seed(self.seed, corner_x, corner_z);
        let mut out = Vec::new();
        for (step, order) in self.settings.order().iter().enumerate() {
            for (index, placed) in order.iter().enumerate() {
                let Some((_, modifiers)) = self.wanted.iter().find(|(r, _)| r == placed) else {
                    continue;
                };
                if !ground.runs(*placed) {
                    continue;
                }
                random.set_feature_seed(decoration, index as i32, step as i32);
                let mut run = Boxes {
                    ground,
                    decorator: self,
                    random: &mut random,
                    known: true,
                    out: &mut out,
                    placed: *placed,
                    step,
                    index,
                    min_y,
                    max_y,
                };
                let origin = [corner_x, self.origin_y, corner_z];
                run.apply(modifiers, origin, origin);
            }
        }
        out.dedup();
        out
    }
}

/// One placed feature's try boxes in a chunk.
struct Boxes<'r, 'a> {
    decorator: &'r Decorator<'a>,
    ground: &'r dyn Ground,
    random: &'r mut WorldgenRandom,
    /// Whether the random's draws are still the game's.
    known: bool,
    out: &'r mut Vec<TryBox>,
    placed: PlacedRef,
    step: usize,
    index: usize,
    min_y: i32,
    max_y: i32,
}

impl Boxes<'_, '_> {
    /// The rest of the modifiers on the box from `lo` to `hi`. While the draws are known, each
    /// modifier draws as the game does and moves the box by what it drew; once they aren't, it
    /// grows the box by everything the modifier could do, and one copy stands for all of a
    /// count's.
    fn apply(&mut self, modifiers: &[Modifier], lo: [i32; 3], hi: [i32; 3]) {
        let Some((modifier, rest)) = modifiers.split_first() else {
            let try_box = TryBox {
                placed: self.placed,
                step: self.step,
                index: self.index,
                min: lo,
                max: hi,
            };
            if self.out.last() != Some(&try_box) {
                self.out.push(try_box);
            }
            // The feature draws what its blocks decide.
            self.known = false;
            return;
        };
        let add = |v: [i32; 3], d: [i32; 3]| {
            [
                v[0].saturating_add(d[0]),
                v[1].saturating_add(d[1]),
                v[2].saturating_add(d[2]),
            ]
        };
        match modifier {
            Modifier::Count(count) => {
                if self.known {
                    let copies = count.sample(self.random);
                    for _ in 0..copies {
                        let known_before = self.known;
                        self.apply(rest, lo, hi);
                        // A copy that began unknown gave every box; the rest give the same.
                        if !known_before {
                            break;
                        }
                    }
                } else if count.max() > 0 {
                    self.apply(rest, lo, hi);
                }
            }
            Modifier::Rarity(chance) => {
                if !self.known || self.random.next_float() < 1.0 / *chance as f32 {
                    self.apply(rest, lo, hi);
                }
            }
            Modifier::NoiseBasedCount { .. } | Modifier::NoiseThresholdCount { .. } => {
                // No draws: a single position's count is known, a range's could be anything.
                let copies = if lo == hi {
                    self.decorator
                        .noise_count(modifier, lo[0], lo[2])
                        .unwrap_or(0)
                } else {
                    1
                };
                for _ in 0..copies {
                    let known_before = self.known;
                    self.apply(rest, lo, hi);
                    if !known_before {
                        break;
                    }
                }
            }
            Modifier::InSquare => {
                if self.known {
                    let dx = self.random.next_int_bounded(16);
                    let dz = self.random.next_int_bounded(16);
                    self.apply(rest, add(lo, [dx, 0, dz]), add(hi, [dx, 0, dz]));
                } else {
                    self.apply(rest, lo, add(hi, [15, 0, 15]));
                }
            }
            Modifier::HeightRange(height) => {
                let (least_y, greatest_y) = if self.known {
                    let y = height.sample(self.random, self.decorator.anchors);
                    (y, y)
                } else {
                    height.range(self.decorator.anchors)
                };
                self.apply(rest, [lo[0], least_y, lo[2]], [hi[0], greatest_y, hi[2]]);
            }
            Modifier::Offset(providers) => {
                if self.known {
                    let offset = [
                        providers[0].sample(self.random),
                        providers[1].sample(self.random),
                        providers[2].sample(self.random),
                    ];
                    self.apply(rest, add(lo, offset), add(hi, offset));
                } else {
                    let least = [providers[0].min(), providers[1].min(), providers[2].min()];
                    let greatest = [providers[0].max(), providers[1].max(), providers[2].max()];
                    self.apply(rest, add(lo, least), add(hi, greatest));
                }
            }
            Modifier::Biome => {
                // A single column can be ruled out, at every height it can be.
                if lo[0] == hi[0] && lo[2] == hi[2] && hi[1] - lo[1] <= 512 {
                    let any = (lo[1].max(self.min_y)..=hi[1].min(self.max_y)).any(|y| {
                        let lookup = self.decorator.generator.block_biome([lo[0], y, lo[2]]);
                        std::iter::once(lookup.biome())
                            .chain(lookup.candidates().iter().copied())
                            .any(|b| self.decorator.lists(b, self.placed))
                    });
                    if !any {
                        return;
                    }
                }
                self.apply(rest, lo, hi);
            }
            Modifier::ReadsBlocks { shape, .. } => match shape {
                Some(Read::EnvironmentScan {
                    down, max_steps, ..
                }) => {
                    let (least_y, greatest_y) = if *down {
                        (lo[1].saturating_sub(*max_steps), hi[1])
                    } else {
                        (lo[1], hi[1].saturating_add(*max_steps))
                    };
                    self.apply(rest, [lo[0], least_y, lo[2]], [hi[0], greatest_y, hi[2]]);
                }
                Some(
                    Read::SurfaceRelativeThreshold { .. }
                    | Read::Filter(_)
                    | Read::SurfaceWaterDepth { .. },
                ) => {
                    self.apply(rest, lo, hi);
                }
                Some(Read::Heightmap(heightmap)) => {
                    let (least_y, greatest_y) =
                        self.ground
                            .heights(*heightmap, [lo[0], lo[2]], [hi[0], hi[2]]);
                    // HeightmapPlacement: nothing at the level's bottom.
                    if greatest_y <= self.min_y {
                        return;
                    }
                    self.apply(rest, [lo[0], least_y, lo[2]], [hi[0], greatest_y, hi[2]]);
                }
                // A modifier not ported: anywhere in the column.
                _ => self.apply(rest, [lo[0], self.min_y, lo[2]], [hi[0], self.max_y, hi[2]]),
            },
        }
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
            Modifier::NoiseBasedCount { .. } | Modifier::NoiseThresholdCount { .. } => {
                let copies = self.decorator.noise_count(modifier, x, z).unwrap_or(0);
                for _ in 0..copies {
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
            Read::Heightmap(heightmap) => {
                let height = region.height(*heightmap, x, z);
                (height > region.min_y()).then_some([x, height, z])
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
                let surface = i64::from(region.height(*heightmap, x, z));
                let (lo, hi) = (surface + i64::from(*min), surface + i64::from(*max));
                (lo <= i64::from(y) && i64::from(y) <= hi).then_some(pos)
            }
            Read::Filter(predicate) => predicate.test(region, pos).then_some(pos),
            Read::SurfaceWaterDepth { max } => {
                let depth = region
                    .height(Heightmap::WorldSurface, x, z)
                    .wrapping_sub(region.height(Heightmap::OceanFloor, x, z));
                (depth <= *max).then_some(pos)
            }
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
        let found = Try {
            placed: self.placed,
            step: self.step,
            index: self.index,
            pos,
            exact: self.exact && known,
            certain: self.certain,
        };
        self.out.push(found);
        // On the blocks, every try reaches the feature, which knows from `exact` whether its
        // position is the game's.
        let drawn = match self.region.as_deref_mut() {
            Some(region) => (self.feature)(&found, self.random, Some(region)),
            None if known && self.certain => (self.feature)(&found, self.random, None),
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
