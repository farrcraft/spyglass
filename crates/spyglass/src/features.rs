//! Features: where the target features place in a chunk, each answer certain or a candidate
//! (`docs/specs/locate.md#features`, `docs/specs/features.md`).

use std::collections::HashMap;
use std::ops::ControlFlow;

use spyglass_biome::BiomeId;
use spyglass_blocks::Chunk;
use spyglass_feature::order::PlacedRef;
use spyglass_feature::outline::Outline;
use spyglass_feature::structures;
use spyglass_feature::{
    Blocks, Decorator, Doubt, FeatureSettings, NeighbourBounds, Outcome, Placer, Reading, Region,
    Wanted,
};

use crate::{Error, Result, Session, World};

/// A world's features, seed-independent, loaded on first use.
pub(crate) struct FeatureWorld {
    /// The placed and configured features, and each decoration step's order of them.
    settings: FeatureSettings,
    /// The blocks' behaviour, for placing on them.
    blocks: Blocks,
    /// What places the features spyglass places.
    placer: Placer,
    /// How far features' writes reach, for what earlier features and neighbours could write.
    bounds: NeighbourBounds,
    /// The placed features a query can name.
    targets: Vec<PlacedRef>,
    /// Every placed feature of the steps up to the last target's, to place, with their
    /// modifiers read in full.
    placing: Wanted,
    /// Every placed feature, by their modifiers' shapes, for boxes of tries.
    everything: Wanted,
    /// The placed features of the steps up to the last target's, by their modifiers' shapes:
    /// the ones that can write before a target.
    early: Wanted,
    /// Those of them spyglass places, whose writes the region holds.
    early_placed: Wanted,
    /// Each target placed feature on its own, for whether a chunk has tries of it.
    alone: HashMap<PlacedRef, Wanted>,
    /// The last decoration step a target is in.
    last_step: usize,
}

impl FeatureWorld {
    fn load(world: &World) -> Result<FeatureWorld> {
        let bundle = &world.bundle;
        let v1_21 = bundle
            .resource("worldgen/configured_feature", "minecraft:monster_room")
            .is_ok();
        let settings = FeatureSettings::load(bundle, world.source.biome_names(), v1_21)?;
        let mut blocks = Blocks::new(bundle, world.block_settings()?.palette())?;
        let all: Vec<PlacedRef> = settings.order().iter().flatten().copied().collect();
        let placer = Placer::load(bundle, &settings, &mut blocks, &all)?;
        let targets: Vec<PlacedRef> = all
            .iter()
            .copied()
            .filter(|&r| placer.is_target(r))
            .collect();
        let last_step = settings
            .order()
            .iter()
            .rposition(|step| step.iter().any(|r| targets.contains(r)))
            .unwrap_or(0);
        let early: Vec<PlacedRef> = settings.order()
            [..=last_step.min(settings.order().len().saturating_sub(1))]
            .iter()
            .flatten()
            .copied()
            .collect();
        let mut reading = Reading {
            resources: bundle,
            blocks: &mut blocks,
        };
        let placing = Wanted::load(&settings, &early, Some(&mut reading))?;
        let everything = Wanted::load(&settings, &all, None)?;
        let placed_early: Vec<PlacedRef> = early
            .iter()
            .copied()
            .filter(|&r| placer.places(r))
            .collect();
        let early_placed = Wanted::load(&settings, &placed_early, None)?;
        let early = Wanted::load(&settings, &early, None)?;
        let mut alone = HashMap::new();
        for &target in &targets {
            alone.insert(target, Wanted::load(&settings, &[target], None)?);
        }
        let bounds = NeighbourBounds::load(bundle, &settings)?;
        Ok(FeatureWorld {
            settings,
            blocks,
            placer,
            bounds,
            targets,
            placing,
            everything,
            early,
            early_placed,
            alone,
            last_step,
        })
    }
}

impl World {
    /// The world's features, loaded the first time they're asked for.
    pub(crate) fn feature_world(&self) -> Result<&FeatureWorld> {
        if let Some(features) = self.features.get() {
            return Ok(features);
        }
        let features = FeatureWorld::load(self)?;
        Ok(self.features.get_or_init(|| features))
    }

    /// Every feature spyglass places, by placed feature id: the ones a [`FeatureQuery`] can
    /// name.
    pub fn features(&self) -> Result<Vec<&str>> {
        let features = self.feature_world()?;
        Ok(features
            .targets
            .iter()
            .filter_map(|&r| features.settings.placed(r).id.as_deref())
            .collect())
    }

    /// Resolves a feature argument: a placed feature's id, or a configured feature's, which
    /// names every placed feature of it (`minecraft:monster_room` is both dungeons). One
    /// spyglass doesn't place is an error (`docs/specs/locate.md#features`).
    pub fn feature_query(&self, argument: &str) -> Result<FeatureQuery> {
        let features = self.feature_world()?;
        let id = spyglass_data::codec::full_id(argument);
        let targets: Vec<PlacedRef> = features
            .targets
            .iter()
            .copied()
            .filter(|&r| {
                features.settings.placed(r).id.as_deref() == Some(id.as_str())
                    || features.placer.configured_id(r) == Some(id.as_str())
            })
            .collect();
        if targets.is_empty() {
            let known = self.exists("worldgen/placed_feature", &id)
                || self.exists("worldgen/feature", &id)
                || self.exists("worldgen/configured_feature", &id);
            return Err(if known {
                Error::NotPlaced(argument.to_string())
            } else {
                Error::Unknown {
                    what: "feature",
                    id: argument.to_string(),
                }
            });
        }
        Ok(FeatureQuery {
            world: self.id,
            argument: argument.to_string(),
            targets,
        })
    }
}

/// A feature argument resolved by [`World::feature_query`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureQuery {
    /// The world it was resolved in, so it isn't asked of another.
    world: u64,
    /// The argument as given.
    argument: String,
    /// The placed features it names.
    targets: Vec<PlacedRef>,
}

impl FeatureQuery {
    /// The argument as given, e.g. `minecraft:monster_room`.
    pub fn argument(&self) -> &str {
        &self.argument
    }
}

/// A feature found: a try of a placed feature a query names that placed, or might have.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct FoundFeature {
    /// The placed feature, e.g. `minecraft:monster_room_deep`.
    pub feature: String,
    /// The position the game calls the feature with.
    pub position: [i32; 3],
    /// Whether it's certain, for the chunk decorated before its neighbours: it placed, and
    /// nothing in its chunk spyglass can't know could change that.
    pub certain: bool,
    /// Whether it placed on spyglass's blocks. A candidate that didn't may still in the game.
    pub likely: bool,
    /// Why it's a candidate, if it is.
    pub reason: Option<String>,
    /// Whether neighbours decorated first could change it (ADR 0056), if that was asked for.
    pub neighbours: Neighbours,
}

/// Whether a feature holds whatever its chunk's neighbours do.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Neighbours {
    /// Not asked for ([`FeatureOptions::neighbours`]).
    NotChecked,
    /// It holds whatever the neighbours do.
    Hold,
    /// Neighbours decorated first could change it, and why.
    Could(String),
}

/// What a feature query works out besides where features are.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct FeatureOptions {
    /// Whether each answer holds whatever the neighbours do: about half a chunk's time.
    pub neighbours: bool,
    /// Whether to answer the candidates that didn't place on spyglass's blocks (`likely`
    /// false) too. Without them, a chunk is placed only where a target could place on its own,
    /// which rules most chunks out from the terrain alone
    /// (`docs/specs/features.md#ruling-out`).
    pub unlikely: bool,
}

/// The answer to [`Session::nearest_feature`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct FeatureLocate {
    /// Every feature that may be the nearest, nearest first by horizontal distance, ending at
    /// the first certain one; empty if none is within the radius.
    pub hits: Vec<FoundFeature>,
    /// Whether it ends at a certain one.
    pub complete: bool,
}

/// Each chunk's features, by the placed features and options asked for: what one seed's
/// queries share, so a chunk is placed once however many ask for it.
#[derive(Default)]
pub(crate) struct Placings(HashMap<(Vec<PlacedRef>, FeatureOptions, [i32; 2]), Vec<FoundFeature>>);

/// What a query keeps between chunks: carved chunks, stored biomes and the terrain in
/// outline.
struct Cache<'g> {
    /// Carved chunks, by chunk coordinates: each is a neighbour of up to 9 placed chunks.
    carved: HashMap<[i32; 2], Chunk>,
    /// Each chunk's stored biomes, for what earlier features and neighbours could write.
    biomes: HashMap<[i32; 2], Vec<BiomeId>>,
    /// The terrain in outline, for ruling chunks out: none when the unlikely candidates are
    /// asked for (then no chunk is ruled out), or where the outline can't be drawn.
    outline: Option<Outline<'g>>,
}

impl<'g> Cache<'g> {
    fn new(generator: &'g spyglass_blocks::ChunkGenerator<'g>, options: FeatureOptions) -> Self {
        Cache {
            carved: HashMap::new(),
            biomes: HashMap::new(),
            outline: if options.unlikely {
                None
            } else {
                Outline::new(generator)
            },
        }
    }
}

impl<'w> Session<'w> {
    /// Every feature `query` names that placed, or might have, in chunks `min..=max` (chunk
    /// coordinates, inclusive), chunk by chunk (z, then x), each in decoration order. Places
    /// every chunk whose tries reach a feature the query names, which takes a while each: about
    /// 30 ms with its neighbours' blocks, unless the options rule it out from the terrain
    /// (`docs/specs/locate.md#features`).
    pub fn features_in(
        &self,
        query: &FeatureQuery,
        min: [i32; 2],
        max: [i32; 2],
        options: FeatureOptions,
    ) -> Result<Vec<FoundFeature>> {
        let mut out = Vec::new();
        let placings = &mut Placings::default();
        self.each_feature(
            query,
            min,
            max,
            |_| true,
            options,
            placings,
            |f| {
                out.push(f);
                ControlFlow::Continue(())
            },
        )?;
        Ok(out)
    }

    /// [`features_in`](Self::features_in) over the chunks `wanted` keeps, each feature handed
    /// to `visit` as it's found, until it breaks. Chunks already in `placings` aren't placed
    /// again. Keeps the carved chunks of three rows at a time.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn each_feature(
        &self,
        query: &FeatureQuery,
        min: [i32; 2],
        max: [i32; 2],
        wanted: impl Fn([i32; 2]) -> bool,
        options: FeatureOptions,
        placings: &mut Placings,
        mut visit: impl FnMut(FoundFeature) -> ControlFlow<()>,
    ) -> Result<()> {
        self.check_world(query.world, &query.argument)?;
        let mut cache = None;
        for cz in min[1]..=max[1] {
            // The rows before this one's neighbours aren't read again.
            if let Some(Cache { carved, biomes, .. }) = &mut cache {
                carved.retain(|c, _| c[1] >= cz - 1);
                biomes.retain(|c, _| c[1] >= cz - 1);
            }
            for cx in (min[0]..=max[0]).filter(|&cx| wanted([cx, cz])) {
                let key = (query.targets.clone(), options, [cx, cz]);
                let found = match placings.0.get(&key) {
                    Some(found) => found.clone(),
                    None => {
                        if cache.is_none() {
                            cache = Some(Cache::new(self.chunks()?, options));
                        }
                        let cache = cache.as_mut().expect("made above");
                        let found = self.chunk_features(query, [cx, cz], options, cache)?;
                        placings.0.insert(key, found.clone());
                        found
                    }
                };
                for f in found {
                    if visit(f).is_break() {
                        return Ok(());
                    }
                }
            }
        }
        Ok(())
    }

    /// The feature `query` names nearest to `from`, horizontally, within `radius` chunks:
    /// every candidate nearer than the nearest certain one, then that one.
    pub fn nearest_feature(
        &self,
        query: &FeatureQuery,
        from: [i32; 3],
        radius: i32,
        options: FeatureOptions,
    ) -> Result<FeatureLocate> {
        self.check_world(query.world, &query.argument)?;
        let mut cache = Cache::new(self.chunks()?, options);
        let centre = [from[0] >> 4, from[2] >> 4];
        let distance_sqr = |p: [i32; 3]| {
            let (dx, dz) = (i64::from(p[0] - from[0]), i64::from(p[2] - from[2]));
            dx * dx + dz * dz
        };
        // Square rings of chunks around `from`'s, outwards. Every feature found is kept, and
        // the nearest certain one's squared distance bounds the search: once a whole ring is
        // further than it, no ring after can hold anything nearer, so the search stops.
        let mut found: Vec<FoundFeature> = Vec::new();
        let mut nearest_certain: Option<i64> = None;
        for ring in 0..=radius {
            // The ring's nearest block is (ring - 1) chunks' worth of blocks away.
            if let Some(nearest) = nearest_certain {
                let ring_near = i64::from((ring - 1).max(0) * 16);
                if ring_near * ring_near > nearest {
                    break;
                }
            }
            for dz in -ring..=ring {
                for dx in -ring..=ring {
                    if dx.abs().max(dz.abs()) != ring {
                        continue;
                    }
                    let chunk = [centre[0] + dx, centre[1] + dz];
                    for feature in self.chunk_features(query, chunk, options, &mut cache)? {
                        if feature.certain {
                            nearest_certain = Some(
                                nearest_certain.map_or(distance_sqr(feature.position), |nearest| {
                                    nearest.min(distance_sqr(feature.position))
                                }),
                            );
                        }
                        found.push(feature);
                    }
                }
            }
        }
        // Nearest first, ending at the nearest certain one: the candidates before it may be
        // the nearest, and nothing after it can be.
        found.sort_by_key(|f| distance_sqr(f.position));
        let complete = nearest_certain.is_some();
        if let Some(nearest) = nearest_certain {
            let end = found
                .iter()
                .position(|f| f.certain && distance_sqr(f.position) == nearest)
                .map_or(found.len(), |i| i + 1);
            found.truncate(end);
        }
        Ok(FeatureLocate {
            hits: found,
            complete,
        })
    }

    /// One chunk's features that `query` names.
    ///
    /// Placing a chunk is costly (its 8 neighbours carved, then every feature up to the last
    /// target's step decorated), so cheaper tests rule it out first. A chunk none of the named
    /// features has a try in has none. Unless the unlikely candidates are asked for, a chunk
    /// where the terrain in outline shows every try of every named feature declines has none
    /// either (`docs/specs/features.md#ruling-out`). Otherwise the chunk is decorated on its
    /// blocks and its neighbours', with what earlier features, the neighbours and structure
    /// pieces could write putting tries in doubt, and the named features' tries are answered.
    fn chunk_features(
        &self,
        query: &FeatureQuery,
        chunk: [i32; 2],
        options: FeatureOptions,
        cache: &mut Cache<'_>,
    ) -> Result<Vec<FoundFeature>> {
        let world = self.world;
        let features = world.feature_world()?;
        let generator = self.chunks()?;
        let terrain = &world.terrain;
        let (anchors, min_y) = (terrain.anchors(), terrain.level_min_y);
        // Only a chunk where a named feature's tries reach it can have one.
        let reaches = query.targets.iter().any(|target| {
            features.alone.get(target).is_some_and(|alone| {
                let decorator = Decorator::of(
                    &features.settings,
                    generator,
                    self.seed,
                    anchors,
                    min_y,
                    alone,
                );
                !decorator
                    .tries(chunk, &mut |_, _| spyglass_feature::Drawn::Unknown)
                    .is_empty()
            })
        });
        if !reaches {
            return Ok(Vec::new());
        }
        let Cache {
            carved,
            biomes: stored,
            outline,
        } = cache;
        let mut biomes = |chunk: [i32; 2]| {
            stored
                .entry(chunk)
                .or_insert_with(|| generator.chunk_biomes(chunk[0], chunk[1]))
                .clone()
        };
        // Nothing could place on its own, from the terrain in outline: no likely answer.
        if let Some(outline) = outline {
            let (level_min, level_top) = generator.level();
            let placed = Decorator::of(
                &features.settings,
                generator,
                self.seed,
                anchors,
                min_y,
                &features.early_placed,
            );
            // A target's own writes count only where one of its tries isn't ruled out.
            let writes: Vec<_> = features
                .bounds
                .outline_writes(
                    &placed,
                    &features.settings,
                    chunk,
                    [level_min, level_top - 1],
                    &mut biomes,
                )
                .into_iter()
                .filter(|write| !query.targets.contains(&write.placed))
                .collect();
            let ruled_out = query.targets.iter().all(|target| {
                features.alone.get(target).is_some_and(|alone| {
                    let decorator = Decorator::of(
                        &features.settings,
                        generator,
                        self.seed,
                        anchors,
                        min_y,
                        alone,
                    );
                    features.placer.none_place(
                        &decorator,
                        &features.blocks,
                        outline,
                        chunk,
                        &writes,
                    )
                })
            });
            if ruled_out {
                return Ok(Vec::new());
            }
        }
        // The chunk and its 8 neighbours, x fastest, as the region takes them.
        let mut chunks = Vec::with_capacity(9);
        for dz in -1..=1 {
            for dx in -1..=1 {
                let key = [chunk[0] + dx, chunk[1] + dz];
                let neighbour = carved
                    .entry(key)
                    .or_insert_with(|| generator.carved_chunk(key[0], key[1]));
                chunks.push(neighbour.clone());
            }
        }
        let mut region = Region::new(&features.blocks, chunk, chunks);
        let everything = Decorator::of(
            &features.settings,
            generator,
            self.seed,
            anchors,
            min_y,
            &features.everything,
        );
        // What the neighbours, decorated first, could write into this chunk.
        let foreign = if options.neighbours {
            features
                .bounds
                .writes(&everything, &features.settings, &region, &mut biomes)
        } else {
            Vec::new()
        };
        // What this chunk's own earlier features could write: those spyglass doesn't place,
        // and those it places once it isn't sure of them.
        let early = Decorator::of(
            &features.settings,
            generator,
            self.seed,
            anchors,
            min_y,
            &features.early,
        );
        let own = features
            .bounds
            .own_writes(&early, &features.settings, &region, &mut biomes);
        let pools = world.pools()?;
        let starts = self.starts(true).with_pools(pools);
        let pieces = structures::pieces(
            &starts,
            &world.sets,
            chunk,
            features.last_step,
            region.min_y(),
            region.max_y(),
        );
        let decorator = Decorator::of(
            &features.settings,
            generator,
            self.seed,
            anchors,
            min_y,
            &features.placing,
        );
        let placements =
            features
                .placer
                .decorate(&decorator, &mut region, self.seed, &foreign, &own, &pieces);
        let name = |r: PlacedRef| features.settings.placed(r).id.clone().unwrap_or_default();
        let reason = |doubt: Doubt| match doubt {
            Doubt::Tie => "a block it reads depends on a tied biome".to_string(),
            Doubt::OutsideRegion => "it reads past its chunk's neighbours".to_string(),
            Doubt::ShapeUpdate => "a block beside it could change shape".to_string(),
            Doubt::Waterlogging => "a block it places could fill with water".to_string(),
            Doubt::OtherFeature(r) => format!("{} could change what it reads", name(r)),
            Doubt::Neighbours(r) => format!("a neighbour's {}, decorated first", name(r)),
            Doubt::Structure(i) => format!("{}'s pieces", world.sets.structures()[i].id),
        };
        Ok(placements
            .into_iter()
            .filter(|p| query.targets.contains(&p.at.placed))
            .filter_map(|p| {
                let (certain, likely, why) = match p.outcome {
                    Outcome::Placed => (true, true, None),
                    Outcome::Declined => return None,
                    Outcome::Candidate { placed: false, .. } if !options.unlikely => return None,
                    Outcome::Candidate { placed, why } => (false, placed, Some(why)),
                };
                Some(FoundFeature {
                    feature: name(p.at.placed),
                    position: p.at.pos,
                    certain,
                    likely,
                    reason: why.map(reason),
                    neighbours: match (options.neighbours, p.neighbours) {
                        (false, _) => Neighbours::NotChecked,
                        (true, None) => Neighbours::Hold,
                        (true, Some(d)) => Neighbours::Could(reason(d)),
                    },
                })
            })
            .collect())
    }
}
