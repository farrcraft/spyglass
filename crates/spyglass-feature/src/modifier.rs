//! Placement modifiers: how a placed feature turns a chunk's corner into the positions its
//! feature is tried at (`docs/specs/features.md#modifiers`).

use serde_json::Value;
use spyglass_data::codec::{At, LoadError, LoadErrorKind, field, full_id, int, object};
use spyglass_data::providers::{HeightProvider, IntProvider};

/// A placement modifier.
#[derive(Debug, Clone)]
pub enum Modifier {
    /// `count`: the position, `count` times.
    Count(IntProvider),
    /// `rarity_filter`: the position, if `nextFloat() < 1 / chance`.
    Rarity(i32),
    /// `in_square`: x and z each moved by `nextInt(16)`, x first.
    InSquare,
    /// `height_range`: y from the height provider.
    HeightRange(HeightProvider),
    /// `biome`: the position, if the biome at its block lists the placed feature.
    Biome,
    /// `offset` (26.3 on): the position moved by `x`, `y` and `z`, sampled in that order.
    Offset([IntProvider; 3]),
    /// A modifier that reads the level's blocks or heightmaps (`heightmap`,
    /// `block_predicate_filter`, `environment_scan`, `surface_relative_threshold_filter`, …), by
    /// type id: what it gives depends on blocks decoration has changed.
    ReadsBlocks(String),
}

/// The modifiers that read blocks.
const READS_BLOCKS: [&str; 6] = [
    "minecraft:heightmap",
    "minecraft:block_predicate_filter",
    "minecraft:environment_scan",
    "minecraft:surface_relative_threshold_filter",
    "minecraft:surface_water_depth_filter",
    "minecraft:count_on_every_layer",
];

impl Modifier {
    /// A placed feature's `placement` list.
    pub fn parse_list(json: &Value, at: &At<'_>) -> Result<Vec<Modifier>, LoadError> {
        let Value::Array(items) = json else {
            return Err(at.error(LoadErrorKind::Shape("a list of placement modifiers")));
        };
        items
            .iter()
            .enumerate()
            .map(|(i, m)| Modifier::parse(m, &at.child(&i.to_string())))
            .collect()
    }

    /// One modifier.
    pub fn parse(json: &Value, at: &At<'_>) -> Result<Modifier, LoadError> {
        let o = object(json, at)?;
        let kind = field(o, "type", at)?
            .as_str()
            .map(full_id)
            .ok_or_else(|| at.child("type").error(LoadErrorKind::Shape("a type id")))?;
        Ok(match kind.as_str() {
            "minecraft:count" => {
                let count_at = at.child("count");
                let count = IntProvider::parse(field(o, "count", at)?, &count_at)?;
                if count.min() < 0 || count.max() > 256 {
                    return Err(count_at.error(LoadErrorKind::Invalid("count must be 0 to 256")));
                }
                Modifier::Count(count)
            }
            "minecraft:rarity_filter" => {
                let chance_at = at.child("chance");
                let chance = int(field(o, "chance", at)?, &chance_at)?;
                if chance <= 0 {
                    return Err(chance_at.error(LoadErrorKind::Invalid("chance must be positive")));
                }
                Modifier::Rarity(chance)
            }
            "minecraft:in_square" => Modifier::InSquare,
            "minecraft:height_range" => Modifier::HeightRange(HeightProvider::parse(
                field(o, "height", at)?,
                &at.child("height"),
            )?),
            "minecraft:biome" => Modifier::Biome,
            "minecraft:offset" => {
                let axis =
                    |name: &'static str| IntProvider::parse(field(o, name, at)?, &at.child(name));
                Modifier::Offset([axis("x")?, axis("y")?, axis("z")?])
            }
            k if READS_BLOCKS.contains(&k) => Modifier::ReadsBlocks(kind),
            _ => {
                return Err(at.child("type").error(LoadErrorKind::Unsupported {
                    what: "placement modifier",
                    name: kind,
                }));
            }
        })
    }
}
