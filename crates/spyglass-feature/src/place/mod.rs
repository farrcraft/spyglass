//! The target features themselves: whether a try places, and the blocks it writes
//! (`docs/specs/features.md#placing`).

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
use crate::order::PlacedRef;
use crate::region::Region;
use crate::settings::FeatureSettings;

pub use fossil::Fossil;
pub use geode::Geode;
pub use lake::Lake;
pub use monster_room::MonsterRoom;
pub use well::{DesertWell, Overlay};

/// Configured features, from 26.3.
const FEATURE: &str = "worldgen/feature";
/// Configured features, in 26.2.
const CONFIGURED_FEATURE: &str = "worldgen/configured_feature";

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
        })
    }
}
