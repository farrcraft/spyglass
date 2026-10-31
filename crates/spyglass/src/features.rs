//! Features: where the target features place in a chunk, each answer certain or a candidate
//! (`docs/specs/locate.md#features`, `docs/specs/features.md`).

use std::collections::HashMap;

use spyglass_biome::BiomeId;
use spyglass_blocks::Chunk;
use spyglass_feature::order::PlacedRef;
use spyglass_feature::structures;
use spyglass_feature::{
    Blocks, Decorator, Doubt, FeatureSettings, NeighbourBounds, Outcome, Placer, Reading, Region,
    Wanted,
};

use crate::{Error, Result, Session, World};

/// A world's features, seed-independent, loaded on first use.
pub(crate) struct FeatureWorld {
    settings: FeatureSettings,
    blocks: Blocks,
    placer: Placer,
    bounds: NeighbourBounds,
    /// The placed features a query can name.
    targets: Vec<PlacedRef>,
    /// Every placed feature of the steps up to the last target's, to place, with their
    /// modifiers read in full.
    placing: Wanted,
    /// Every placed feature, by their modifiers' shapes, for boxes of tries.
    everything: Wanted,
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
        let mut alone = HashMap::new();
        for &t in &targets {
            alone.insert(t, Wanted::load(&settings, &[t], None)?);
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
            alone,
            last_step,
        })
    }
}

impl World {
    /// The world's features, loaded the first time they're asked for.
    pub(crate) fn feature_world(&self) -> Result<&FeatureWorld> {
        if let Some(f) = self.features.get() {
            return Ok(f);
        }
        let f = FeatureWorld::load(self)?;
        Ok(self.features.get_or_init(|| f))
    }

    /// Every feature spyglass places, by placed feature id: the ones a [`FeatureQuery`] can
    /// name.
    pub fn features(&self) -> Result<Vec<&str>> {
        let f = self.feature_world()?;
        Ok(f.targets
            .iter()
            .filter_map(|&r| f.settings.placed(r).id.as_deref())
            .collect())
    }

    /// Resolves a feature argument: a placed feature's id, or a configured feature's, which
    /// names every placed feature of it (`minecraft:monster_room` is both dungeons). One
    /// spyglass doesn't place is an error (`docs/specs/locate.md#features`).
    pub fn feature_query(&self, argument: &str) -> Result<FeatureQuery> {
        let f = self.feature_world()?;
        let id = spyglass_data::codec::full_id(argument);
        let targets: Vec<PlacedRef> = f
            .targets
            .iter()
            .copied()
            .filter(|&r| {
                f.settings.placed(r).id.as_deref() == Some(id.as_str())
                    || f.placer.configured_id(r) == Some(id.as_str())
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
    world: u64,
    argument: String,
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
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FeatureOptions {
    /// Whether each answer holds whatever the neighbours do: about half a chunk's time.
    pub neighbours: bool,
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

/// What a query keeps between chunks: carved chunks and stored biomes.
#[derive(Default)]
struct Cache {
    carved: HashMap<[i32; 2], Chunk>,
    biomes: HashMap<[i32; 2], Vec<BiomeId>>,
}

impl<'w> Session<'w> {
    /// Every feature `query` names that placed, or might have, in chunks `min..=max` (chunk
    /// coordinates, inclusive), chunk by chunk (z, then x), each in decoration order. Places
    /// every chunk whose tries reach a feature the query names, which takes a while each: about
    /// 40 ms with its neighbours' blocks (`docs/specs/locate.md#features`).
    pub fn features_in(
        &self,
        query: &FeatureQuery,
        min: [i32; 2],
        max: [i32; 2],
        options: FeatureOptions,
    ) -> Result<Vec<FoundFeature>> {
        self.check_world(query.world, &query.argument)?;
        let mut cache = Cache::default();
        let mut out = Vec::new();
        for cz in min[1]..=max[1] {
            for cx in min[0]..=max[0] {
                out.extend(self.chunk_features(query, [cx, cz], options, &mut cache)?);
            }
        }
        Ok(out)
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
        let mut cache = Cache::default();
        let centre = [from[0] >> 4, from[2] >> 4];
        let dist = |p: [i32; 3]| {
            let (dx, dz) = (i64::from(p[0] - from[0]), i64::from(p[2] - from[2]));
            dx * dx + dz * dz
        };
        let mut found: Vec<FoundFeature> = Vec::new();
        let mut best: Option<i64> = None;
        for r in 0..=radius {
            // The ring's nearest block is (r - 1) chunks' worth of blocks away.
            if let Some(b) = best {
                let near = i64::from((r - 1).max(0) * 16);
                if near * near > b {
                    break;
                }
            }
            for dz in -r..=r {
                for dx in -r..=r {
                    if dx.abs().max(dz.abs()) != r {
                        continue;
                    }
                    let chunk = [centre[0] + dx, centre[1] + dz];
                    for f in self.chunk_features(query, chunk, options, &mut cache)? {
                        if f.certain {
                            best = Some(best.map_or(dist(f.position), |b| b.min(dist(f.position))));
                        }
                        found.push(f);
                    }
                }
            }
        }
        found.sort_by_key(|f| dist(f.position));
        let complete = best.is_some();
        if let Some(b) = best {
            let end = found
                .iter()
                .position(|f| f.certain && dist(f.position) == b)
                .map_or(found.len(), |i| i + 1);
            found.truncate(end);
        }
        Ok(FeatureLocate {
            hits: found,
            complete,
        })
    }

    /// One chunk's features that `query` names.
    fn chunk_features(
        &self,
        query: &FeatureQuery,
        chunk: [i32; 2],
        options: FeatureOptions,
        cache: &mut Cache,
    ) -> Result<Vec<FoundFeature>> {
        let world = self.world;
        let f = world.feature_world()?;
        let generator = self.chunks()?;
        let terrain = &world.terrain;
        let (anchors, min_y) = (terrain.anchors(), terrain.level_min_y);
        // Only a chunk where a named feature's tries reach it can have one.
        let reaches = query.targets.iter().any(|t| {
            f.alone.get(t).is_some_and(|w| {
                let d = Decorator::of(&f.settings, generator, self.seed, anchors, min_y, w);
                !d.tries(chunk, &mut |_, _| spyglass_feature::Drawn::Unknown)
                    .is_empty()
            })
        });
        if !reaches {
            return Ok(Vec::new());
        }
        let mut chunks = Vec::with_capacity(9);
        for dz in -1..=1 {
            for dx in -1..=1 {
                let key = [chunk[0] + dx, chunk[1] + dz];
                let c = cache
                    .carved
                    .entry(key)
                    .or_insert_with(|| generator.carved_chunk(key[0], key[1]));
                chunks.push(c.clone());
            }
        }
        let mut region = Region::new(&f.blocks, chunk, chunks);
        let mut biomes = |c: [i32; 2]| {
            cache
                .biomes
                .entry(c)
                .or_insert_with(|| generator.chunk_biomes(c[0], c[1]))
                .clone()
        };
        let everything = Decorator::of(
            &f.settings,
            generator,
            self.seed,
            anchors,
            min_y,
            &f.everything,
        );
        let foreign = if options.neighbours {
            f.bounds
                .writes(&everything, &f.settings, &region, &mut biomes)
        } else {
            Vec::new()
        };
        let own = f
            .bounds
            .own_writes(&everything, &f.settings, &region, &mut biomes);
        let pools = world.pools()?;
        let starts = self.starts(true).with_pools(pools);
        let pieces = structures::pieces(
            &starts,
            &world.sets,
            chunk,
            f.last_step,
            region.min_y(),
            region.max_y(),
        );
        let decorator = Decorator::of(
            &f.settings,
            generator,
            self.seed,
            anchors,
            min_y,
            &f.placing,
        );
        let placements =
            f.placer
                .decorate(&decorator, &mut region, self.seed, &foreign, &own, &pieces);
        let name = |r: PlacedRef| f.settings.placed(r).id.clone().unwrap_or_default();
        let reason = |d: Doubt| match d {
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
