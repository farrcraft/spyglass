//! The target features themselves: whether a try places, and the blocks it writes
//! (`docs/specs/features.md#placing`).

mod block_blob;
mod dripstone;
mod fossil;
mod geode;
mod lake;
mod monster_room;
pub mod template;
mod well;

use serde_json::Value;
use spyglass_data::codec::{At, LoadError, LoadErrorKind, field, full_id, object};
use spyglass_density::Resources;
use spyglass_rng::WorldgenRandom;

use crate::blocks::Blocks;
use crate::decorate::{Decorator, Drawn, Try};
use crate::neighbours::Foreign;
use crate::order::PlacedRef;
use crate::outline::Outline;
use crate::reach::Changes;
use crate::region::{Container, Doubt, Region};
use crate::settings::FeatureSettings;
use crate::structures::Piece;

pub use block_blob::BlockBlob;
pub use dripstone::LargeDripstone;
pub use fossil::Fossil;
pub use geode::Geode;
pub use lake::Lake;
pub use monster_room::MonsterRoom;
pub use well::{DesertWell, Overlay};

/// Configured features, from 26.3.
const FEATURE: &str = "worldgen/feature";
/// Configured features, in 26.2.
const CONFIGURED_FEATURE: &str = "worldgen/configured_feature";

/// What a target feature did at a try.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// It placed, certainly.
    Placed,
    /// It declined, certainly.
    Declined,
    /// spyglass's answer, which something it can't know could change.
    Candidate {
        /// Whether it placed on the blocks spyglass has.
        placed: bool,
        /// Why it isn't certain.
        why: Doubt,
    },
}

/// A target feature's try in a chunk and what it did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placement {
    /// The try.
    pub at: Try,
    /// What it did, for the chunk decorated before its neighbours (certain on its own).
    pub outcome: Outcome,
    /// Why neighbours decorated first could change it, if they could; `None` where it holds
    /// whatever the neighbours do (`docs/specs/features.md#certainty`).
    pub neighbours: Option<Doubt>,
    /// The containers it placed with loot tables, in the order it placed them: as certain as
    /// `outcome` (`docs/specs/loot.md#containers`).
    pub containers: Vec<Container>,
}

/// A box an earlier feature could write, for [`MonsterRoom::declines_on`], and whether it could
/// write air there.
#[derive(Debug, Clone, Copy)]
pub struct Written {
    /// The least corner.
    pub min: [i32; 3],
    /// The greatest corner.
    pub max: [i32; 3],
    /// Whether it could write air.
    pub air: bool,
}

impl Written {
    /// Whether `pos` is in the box.
    pub fn holds(&self, pos: [i32; 3]) -> bool {
        (0..3).all(|i| (self.min[i]..=self.max[i]).contains(&pos[i]))
    }
}

/// Offsets a try reads, around its position: the least, the greatest, and whether y is any y.
#[derive(Debug, Clone, Copy)]
struct ReadBox([i32; 3], [i32; 3], bool);

/// A configured feature spyglass places.
#[derive(Debug, Clone)]
pub enum Configured {
    /// `lake`.
    Lake(Lake),
    /// `geode`.
    Geode(Box<Geode>),
    /// `monster_room`.
    MonsterRoom(MonsterRoom),
    /// `fossil`.
    Fossil(Box<Fossil>),
    /// 26.2's `desert_well`.
    DesertWell(DesertWell),
    /// `overlay` of `template` features: 26.3's desert well.
    Overlay(Overlay),
    /// `block_blob`: the forest rock.
    BlockBlob(BlockBlob),
    /// `large_dripstone`; 26.4's `large_speleothem`.
    LargeDripstone(Box<LargeDripstone>),
}

/// The configured features of the placed features asked for.
#[derive(Debug, Clone)]
pub struct Placer {
    features: Vec<(PlacedRef, Configured)>,
    /// Each one's configured feature's id, if it names one.
    configured: Vec<(PlacedRef, Option<String>)>,
}

impl Placer {
    /// Loads the configured feature of each of `placed`, adding the states they write to
    /// `blocks`. A placed feature whose feature isn't ported is left out.
    pub fn load(
        resources: &dyn Resources,
        settings: &FeatureSettings,
        blocks: &mut Blocks,
        placed: &[PlacedRef],
    ) -> Result<Placer, LoadError> {
        let mut features = Vec::new();
        let mut configured_ids = Vec::new();
        for &placed_ref in placed {
            let placed_feature = settings.placed(placed_ref);
            let name = placed_feature
                .id
                .clone()
                .unwrap_or_else(|| "an inline placed feature".into());
            let at = At::root(&name);
            let feature = field(object(&placed_feature.json, &at)?, "feature", &at)?;
            let named = feature.as_str().map(full_id);
            let (json, id) = match feature {
                Value::String(id) => {
                    let id = full_id(id);
                    // 26.3 keeps configured features in `worldgen/feature`, 26.2 in
                    // `worldgen/configured_feature`.
                    let json = resources
                        .resource(FEATURE, &id)
                        .or_else(|_| resources.resource(CONFIGURED_FEATURE, &id))
                        .map_err(|e| At::root(&id).error(e.into()))?;
                    (json, id)
                }
                inline => (inline.clone(), format!("{name}'s feature")),
            };
            let at = At::root(&id);
            let fields = object(&json, &at)?;
            let kind = field(fields, "type", &at)?
                .as_str()
                .map(full_id)
                .ok_or_else(|| {
                    at.child("type")
                        .error(LoadErrorKind::Shape("a feature type"))
                })?;
            // 26.2 nests the configuration under `config`; 26.3 flattens it.
            let (config, config_at) = match fields.get("config") {
                Some(nested) => (object(nested, &at.child("config"))?, at.child("config")),
                _ => (fields, at.clone()),
            };
            let configured = match kind.as_str() {
                "minecraft:lake" => {
                    Configured::Lake(Lake::load(config, &config_at, resources, blocks)?)
                }
                "minecraft:geode" => Configured::Geode(Box::new(Geode::load(
                    config, &config_at, resources, blocks,
                )?)),
                "minecraft:monster_room" => {
                    Configured::MonsterRoom(MonsterRoom::load(resources, blocks)?)
                }
                "minecraft:fossil" => Configured::Fossil(Box::new(Fossil::load(
                    config, &config_at, resources, blocks,
                )?)),
                "minecraft:desert_well" => Configured::DesertWell(DesertWell::load(blocks)?),
                "minecraft:overlay" => {
                    Configured::Overlay(Overlay::load(config, &config_at, resources, blocks)?)
                }
                "minecraft:block_blob" => {
                    Configured::BlockBlob(BlockBlob::load(config, &config_at, resources, blocks)?)
                }
                "minecraft:large_dripstone" | "minecraft:large_speleothem" => {
                    Configured::LargeDripstone(Box::new(LargeDripstone::load(
                        config, &config_at, resources, blocks,
                    )?))
                }
                _ => continue,
            };
            features.push((placed_ref, configured));
            configured_ids.push((placed_ref, named));
        }
        Ok(Placer {
            features,
            configured: configured_ids,
        })
    }

    /// The boxes a placed feature's try can read, around its position, and whether a block
    /// turning into another solid one (an ore) can change its answer. Whole columns where y is
    /// any y; `None` for one not placed here.
    fn reads(&self, placed: PlacedRef) -> Option<(&'static [ReadBox], bool)> {
        let (_, feature) = self.features.iter().find(|(r, _)| *r == placed)?;
        Some(match feature {
            // The lake's box, and the scan down to it.
            Configured::Lake(_) => (
                &[
                    ReadBox([-8, -4, -8], [7, 3, 7], false),
                    ReadBox([0, 0, 0], [0, 0, 0], true),
                ],
                true,
            ),
            Configured::Geode(_) => (&[ReadBox([-16; 3], [16; 3], false)], true),
            Configured::MonsterRoom(_) => (&[ReadBox([-4, -2, -4], [4, 4, 4], false)], false),
            // Its footprint's heightmap is terrain's; its y anywhere below.
            Configured::Fossil(_) => (&[ReadBox([-7, 0, -7], [7, 0, 7], true)], false),
            // The heightmap's column, and the well's box.
            Configured::DesertWell(_) | Configured::Overlay(_) => {
                (&[ReadBox([-3, 0, -3], [3, 0, 3], true)], true)
            }
            Configured::BlockBlob(_) => (&[ReadBox([-5, 0, -5], [5, 0, 5], true)], true),
            Configured::LargeDripstone(_) => (&[ReadBox([-32, 0, -32], [32, 0, 32], true)], true),
        })
    }

    /// Whether `placed` is one a query can ask for: a target feature (`docs/specs/features.md`),
    /// not one placed only so the targets after it are exact (the forest rock, large dripstone).
    pub fn is_target(&self, placed: PlacedRef) -> bool {
        self.features.iter().any(|(r, f)| {
            *r == placed && !matches!(f, Configured::BlockBlob(_) | Configured::LargeDripstone(_))
        })
    }

    /// The id of `placed`'s configured feature, if it names one rather than writing it inline.
    pub fn configured_id(&self, placed: PlacedRef) -> Option<&str> {
        self.configured
            .iter()
            .find(|(r, _)| *r == placed)
            .and_then(|(_, id)| id.as_deref())
    }

    /// Whether `placed`'s feature is one spyglass places.
    pub fn places(&self, placed: PlacedRef) -> bool {
        self.features.iter().any(|(r, _)| *r == placed)
    }

    /// The loot table of the containers `placed`'s feature puts down, if it puts any: dungeons'
    /// (`docs/specs/loot.md#containers`).
    pub fn container_table(&self, placed: PlacedRef) -> Option<&'static str> {
        self.features.iter().find_map(|(r, f)| match f {
            Configured::MonsterRoom(_) if *r == placed => Some(monster_room::LOOT_TABLE),
            _ => None,
        })
    }

    /// Whether `placed`'s feature could write air: true unless it writes only one state that
    /// isn't, or isn't ported.
    pub fn writes_air(&self, placed: PlacedRef, blocks: &Blocks) -> bool {
        match self.features.iter().find(|(r, _)| *r == placed) {
            Some((_, Configured::BlockBlob(blob))) => blocks.is_air(blob.state()),
            Some((_, Configured::LargeDripstone(dripstone))) => blocks.is_air(dripstone.state()),
            _ => true,
        }
    }

    /// Places `placed`'s feature at `pos` for world `seed`: whether it placed, or `None` if it
    /// isn't ported.
    pub fn place(
        &self,
        placed: PlacedRef,
        region: &mut Region<'_>,
        random: &mut WorldgenRandom,
        pos: [i32; 3],
        seed: i64,
    ) -> Option<bool> {
        let (_, feature) = self.features.iter().find(|(r, _)| *r == placed)?;
        Some(match feature {
            Configured::Lake(lake) => lake.place(region, random, pos),
            Configured::Geode(geode) => geode.place(region, random, pos, seed),
            Configured::MonsterRoom(room) => room.place(region, random, pos),
            Configured::Fossil(fossil) => fossil.place(region, random, pos),
            Configured::DesertWell(well) => well.place(region, random, pos),
            Configured::Overlay(overlay) => overlay.place(region, random, pos),
            Configured::BlockBlob(blob) => blob.place(region, random, pos),
            Configured::LargeDripstone(dripstone) => dripstone.place(region, random, pos),
        })
    }

    /// Whether no try of `decorator`'s placed features places in chunk `chunk` on its own,
    /// surely, from the terrain in outline (`docs/specs/features.md#ruling-out`): each try is
    /// one whose feature declines on the terrain alone, with what the features this placer
    /// places could write earlier (`writes`, [`crate::NeighbourBounds::outline_writes`]) taken
    /// as any block, and as air where they could write air. False where that isn't known: a
    /// try whose feature can't be ruled out this way, or that isn't exact and certain.
    pub fn none_place(
        &self,
        decorator: &Decorator<'_>,
        blocks: &Blocks,
        outline: &mut Outline<'_>,
        chunk: [i32; 2],
        writes: &[Foreign],
    ) -> bool {
        let mut ruled_out = true;
        let mut rule_out = |t: &Try, random: &mut WorldgenRandom| {
            let feature = self.features.iter().find(|(r, _)| *r == t.placed);
            let Some((_, Configured::MonsterRoom(room))) = feature else {
                ruled_out = false;
                return Drawn::Unknown;
            };
            let earlier: Vec<Written> = writes
                .iter()
                .filter(|f| f.order < (t.step, t.index))
                .map(|f| Written {
                    min: f.min,
                    max: f.max,
                    air: self.writes_air(f.placed, blocks),
                })
                .collect();
            if !ruled_out || !room.declines_on(outline, &earlier, random, t.pos) {
                ruled_out = false;
                return Drawn::Unknown;
            }
            Drawn::Known
        };
        let tries = decorator.tries(chunk, &mut rule_out);
        ruled_out && tries.iter().all(|t| t.exact && t.certain)
    }

    /// Decorates the region's chunk with `decorator`'s placed features: those this placer places
    /// are placed, and each try's outcome returned in decoration order
    /// (`docs/specs/features.md#certainty`).
    ///
    /// A try is a candidate where something spyglass doesn't place exactly could have written
    /// what it reads: what the chunk's own placed features could write wherever their tries
    /// are (`own`, [`crate::NeighbourBounds::own_writes`]), counted for those spyglass doesn't
    /// place, and for those it places from their first candidate on. A placed feature's tries
    /// after its first candidate are candidates too: its draws aren't known. Whether an answer
    /// holds whatever the neighbours do adds what they could write if decorated first
    /// (`foreign`, [`crate::NeighbourBounds::writes`]; ADR 0056).
    pub fn decorate(
        &self,
        decorator: &Decorator<'_>,
        region: &mut Region<'_>,
        seed: i64,
        foreign: &[Foreign],
        own: &[Foreign],
        structures: &[Piece],
    ) -> Vec<Placement> {
        self.decorate_noting(
            decorator,
            region,
            seed,
            foreign,
            own,
            structures,
            &mut Vec::new(),
        )
    }

    /// As [`decorate`](Self::decorate), noting in `unsure` the placed features whose writes in
    /// the chunk aren't known on its own, with why: those this placer doesn't place, and those
    /// it does from their first candidate on. What reads the region after them is in doubt
    /// where they could have written.
    #[allow(
        clippy::too_many_arguments,
        reason = "the decoration's inputs, passed through"
    )]
    pub fn decorate_noting(
        &self,
        decorator: &Decorator<'_>,
        region: &mut Region<'_>,
        seed: i64,
        foreign: &[Foreign],
        own: &[Foreign],
        structures: &[Piece],
        unsure: &mut Vec<(PlacedRef, Doubt)>,
    ) -> Vec<Placement> {
        let [cx, cz] = region.center();
        let chunk = ([cx * 16, cz * 16], [cx * 16 + 15, cz * 16 + 15]);
        let mut out = Vec::new();
        // The placed features whose writes aren't known, with why: on the chunk's own, and
        // whatever the neighbours do.
        unsure.clear();
        let mut unsure_anyway: Vec<(PlacedRef, Doubt)> = Vec::new();
        let mut place = |attempt: &Try, random: &mut WorldgenRandom, region: &mut Region<'_>| {
            region.take_doubt();
            region.take_containers();
            let ours = self.reads(attempt.placed);
            if ours.is_none() || !attempt.exact {
                // Not placed here, or where isn't known: what it writes isn't.
                let why = Doubt::OtherFeature(attempt.placed);
                for list in [&mut *unsure, &mut unsure_anyway] {
                    if !list.iter().any(|(r, _)| *r == attempt.placed) {
                        list.push((attempt.placed, why));
                    }
                }
                return Drawn::Unknown;
            }
            let (boxes, identity) = ours.unwrap_or((&[], true));
            let reads = |f: &Foreign| {
                (identity || f.changes == Changes::Any)
                    && boxes.iter().any(|ReadBox(lo, hi, column)| {
                        (0..3).all(|i| {
                            let (least, greatest) = if i == 1 && *column {
                                (i32::MIN, i32::MAX)
                            } else {
                                (
                                    attempt.pos[i].saturating_add(lo[i]),
                                    attempt.pos[i].saturating_add(hi[i]),
                                )
                            };
                            least <= f.max[i] && f.min[i] <= greatest
                        })
                    })
            };
            // Its own earlier draws, or an earlier unsure feature's writes it reads.
            let doubt = |unsure: &[(PlacedRef, Doubt)]| {
                let find = |r: PlacedRef| unsure.iter().find(|(u, _)| *u == r).map(|(_, d)| *d);
                find(attempt.placed).or_else(|| {
                    own.iter()
                        .filter(|f| f.order < (attempt.step, attempt.index) && reads(f))
                        .find_map(|f| find(f.placed))
                })
            };
            // A structure placed before it, in this chunk; or in the region, if neighbours went
            // first.
            let piece = |in_chunk: bool| {
                structures
                    .iter()
                    .filter(|p| p.step <= attempt.step)
                    .find(|p| {
                        let mut min = p.min;
                        let mut max = p.max;
                        if in_chunk {
                            min[0] = min[0].max(chunk.0[0]);
                            min[2] = min[2].max(chunk.0[1]);
                            max[0] = max[0].min(chunk.1[0]);
                            max[2] = max[2].min(chunk.1[1]);
                        }
                        min[0] <= max[0]
                            && min[2] <= max[2]
                            && reads(&Foreign {
                                min,
                                max,
                                changes: Changes::Any,
                                placed: attempt.placed,
                                order: (0, 0),
                            })
                    })
                    .map(|p| Doubt::Structure(p.structure))
            };
            let tie = (!attempt.certain).then_some(Doubt::Tie);
            let why = tie.or_else(|| piece(true)).or_else(|| doubt(unsure));
            let anyway = foreign
                .iter()
                .find(|f| reads(f))
                .map(|f| Doubt::Neighbours(f.placed))
                .or_else(|| piece(false))
                .or_else(|| doubt(&unsure_anyway));
            if let Some(why) = why {
                region.doubt(why);
            }
            let Some(placed) = self.place(attempt.placed, region, random, attempt.pos, seed) else {
                return Drawn::Unknown;
            };
            let why = region.take_doubt().or(why);
            let anyway = why.or(anyway);
            for (list, doubt) in [(&mut *unsure, why), (&mut unsure_anyway, anyway)] {
                if let Some(doubt) = doubt
                    && !list.iter().any(|(r, _)| *r == attempt.placed)
                {
                    list.push((attempt.placed, doubt));
                }
            }
            let outcome = match why {
                Some(why) => Outcome::Candidate { placed, why },
                None if placed => Outcome::Placed,
                None => Outcome::Declined,
            };
            out.push(Placement {
                at: *attempt,
                outcome,
                neighbours: anyway,
                containers: region.take_containers(),
            });
            Drawn::Known
        };
        decorator.decorate(region, &mut place);
        out
    }
}
