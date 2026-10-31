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
use crate::order::PlacedRef;
use crate::region::{Doubt, Region};
use crate::settings::FeatureSettings;

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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    /// The try.
    pub at: Try,
    /// What it did.
    pub outcome: Outcome,
}

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
        for &r in placed {
            let p = settings.placed(r);
            let name =
                p.id.clone()
                    .unwrap_or_else(|| "an inline placed feature".into());
            let at = At::root(&name);
            let feature = field(object(&p.json, &at)?, "feature", &at)?;
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
            let o = object(&json, &at)?;
            let kind = field(o, "type", &at)?
                .as_str()
                .map(full_id)
                .ok_or_else(|| {
                    at.child("type")
                        .error(LoadErrorKind::Shape("a feature type"))
                })?;
            // 26.2 nests the configuration under `config`; 26.3 flattens it.
            let (config, config_at) = match o.get("config") {
                Some(c) => (object(c, &at.child("config"))?, at.child("config")),
                _ => (o, at.clone()),
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
            features.push((r, configured));
        }
        Ok(Placer { features })
    }

    /// Whether `placed`'s feature is one spyglass places.
    pub fn places(&self, placed: PlacedRef) -> bool {
        self.features.iter().any(|(r, _)| *r == placed)
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

    /// Decorates the region's chunk with `decorator`'s placed features: those this placer places
    /// are placed, and each try's outcome returned in decoration order; any other that reaches
    /// its feature puts everything after it in doubt (`docs/specs/features.md#certainty`).
    pub fn decorate(
        &self,
        decorator: &Decorator<'_>,
        region: &mut Region<'_>,
        seed: i64,
    ) -> Vec<Placement> {
        let mut out = Vec::new();
        let mut after: Option<Doubt> = None;
        let mut place = |t: &Try, random: &mut WorldgenRandom, region: &mut Region<'_>| {
            // Once something in the chunk is in doubt, everything after it is: a candidate's
            // blocks may not be the game's.
            region.take_doubt();
            if let Some(why) = after.or((!t.certain).then_some(Doubt::Tie)) {
                region.doubt(why);
            }
            if !t.exact {
                // Where it went isn't known, so neither is what it wrote.
                after.get_or_insert(Doubt::OtherFeature(t.placed));
                return Drawn::Unknown;
            }
            let Some(placed) = self.place(t.placed, region, random, t.pos, seed) else {
                // Not placed here: whatever it wrote is unknown.
                after.get_or_insert(Doubt::OtherFeature(t.placed));
                return Drawn::Unknown;
            };
            let why = region.take_doubt();
            if let Some(why) = why {
                after.get_or_insert(why);
            }
            let outcome = match why {
                Some(why) => Outcome::Candidate { placed, why },
                None if placed => Outcome::Placed,
                None => Outcome::Declined,
            };
            out.push(Placement { at: *t, outcome });
            Drawn::Known
        };
        decorator.decorate(region, &mut place);
        out
    }
}
