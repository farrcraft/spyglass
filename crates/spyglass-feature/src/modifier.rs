//! Placement modifiers: how a placed feature turns a chunk's corner into the positions its
//! feature is tried at (`docs/specs/features.md#modifiers`).

use serde_json::Value;
use spyglass_data::codec::{At, LoadError, LoadErrorKind, field, full_id, int, object};
use spyglass_data::providers::{HeightProvider, IntProvider};
use spyglass_density::Resources;

use crate::blocks::{Blocks, Heightmap};
use crate::predicate::BlockPredicate;

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
    /// `block_predicate_filter`, `environment_scan`, `surface_relative_threshold_filter`, …): what
    /// it gives depends on blocks decoration has changed. `read` is how, when the modifiers were
    /// loaded with the blocks to read; `None` for tries without them, or a modifier not ported.
    ReadsBlocks {
        /// The type id.
        kind: String,
        /// What it does.
        read: Option<Read>,
    },
}

/// The modifiers that read blocks, as placing runs them (`docs/specs/features.md#modifiers`).
/// None draws from the random.
#[derive(Debug, Clone)]
pub enum Read {
    /// `heightmap`: the position moved to the heightmap's first free y, if that's above the
    /// level's bottom.
    Heightmap(Heightmap),
    /// `environment_scan`: up or down from the position, at most `max_steps`, to the first that
    /// passes `target`, while `allowed` holds.
    EnvironmentScan {
        /// Down, or up.
        down: bool,
        /// 1 to 32.
        max_steps: i32,
        /// `target_condition`: where the scan stops.
        target: BlockPredicate,
        /// `allowed_search_condition`: where it may go; `true` by default.
        allowed: BlockPredicate,
    },
    /// `surface_relative_threshold_filter`: the position, if its y is from `min` to `max`
    /// (inclusive) above the heightmap's first free y.
    SurfaceRelativeThreshold {
        /// The heightmap.
        heightmap: Heightmap,
        /// `min_inclusive`, `i32::MIN` by default.
        min: i32,
        /// `max_inclusive`, `i32::MAX` by default.
        max: i32,
    },
    /// `block_predicate_filter`: the position, if it passes.
    Filter(BlockPredicate),
}

/// What loading the modifiers that read blocks needs.
pub struct Reading<'a> {
    /// The bundle's resources, for block tags.
    pub resources: &'a dyn Resources,
    /// The blocks, to load tags into.
    pub blocks: &'a mut Blocks,
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
    pub fn parse_list(
        json: &Value,
        at: &At<'_>,
        mut reading: Option<&mut Reading<'_>>,
    ) -> Result<Vec<Modifier>, LoadError> {
        let Value::Array(items) = json else {
            return Err(at.error(LoadErrorKind::Shape("a list of placement modifiers")));
        };
        items
            .iter()
            .enumerate()
            .map(|(i, m)| Modifier::parse(m, &at.child(&i.to_string()), reading.as_deref_mut()))
            .collect()
    }

    /// One modifier.
    pub fn parse(
        json: &Value,
        at: &At<'_>,
        reading: Option<&mut Reading<'_>>,
    ) -> Result<Modifier, LoadError> {
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
            k if READS_BLOCKS.contains(&k) => {
                let read = match reading {
                    Some(reading) => Read::parse(&kind, o, at, reading)?,
                    None => None,
                };
                Modifier::ReadsBlocks { kind, read }
            }
            _ => {
                return Err(at.child("type").error(LoadErrorKind::Unsupported {
                    what: "placement modifier",
                    name: kind,
                }));
            }
        })
    }
}

impl Read {
    /// A modifier that reads blocks, by its type id; `None` for one not ported.
    fn parse(
        kind: &str,
        o: &serde_json::Map<String, Value>,
        at: &At<'_>,
        reading: &mut Reading<'_>,
    ) -> Result<Option<Read>, LoadError> {
        let heightmap = |at: &At<'_>| -> Result<Heightmap, LoadError> {
            let name = field(o, "heightmap", at)?.as_str().unwrap_or_default();
            Heightmap::parse(name).ok_or_else(|| {
                at.child("heightmap")
                    .error(LoadErrorKind::Shape("a heightmap"))
            })
        };
        let predicate =
            |key: &'static str, reading: &mut Reading<'_>| -> Result<BlockPredicate, LoadError> {
                BlockPredicate::parse(
                    field(o, key, at)?,
                    &at.child(key),
                    reading.resources,
                    reading.blocks,
                )
            };
        Ok(Some(match kind {
            "minecraft:heightmap" => Read::Heightmap(heightmap(at)?),
            "minecraft:environment_scan" => {
                let down = match field(o, "direction_of_search", at)?.as_str() {
                    Some("down") => true,
                    Some("up") => false,
                    _ => {
                        return Err(at
                            .child("direction_of_search")
                            .error(LoadErrorKind::Shape("up or down")));
                    }
                };
                let steps_at = at.child("max_steps");
                let max_steps = int(field(o, "max_steps", at)?, &steps_at)?;
                if !(1..=32).contains(&max_steps) {
                    return Err(steps_at.error(LoadErrorKind::Invalid("max_steps must be 1 to 32")));
                }
                let target = predicate("target_condition", reading)?;
                let allowed = match o.get("allowed_search_condition") {
                    None => BlockPredicate::True,
                    Some(_) => predicate("allowed_search_condition", reading)?,
                };
                Read::EnvironmentScan {
                    down,
                    max_steps,
                    target,
                    allowed,
                }
            }
            "minecraft:surface_relative_threshold_filter" => {
                let bound = |key: &str, default: i32| match o.get(key) {
                    None => Ok(default),
                    Some(v) => int(v, &at.child(key)),
                };
                Read::SurfaceRelativeThreshold {
                    heightmap: heightmap(at)?,
                    min: bound("min_inclusive", i32::MIN)?,
                    max: bound("max_inclusive", i32::MAX)?,
                }
            }
            "minecraft:block_predicate_filter" => Read::Filter(predicate("predicate", reading)?),
            _ => return Ok(None),
        }))
    }
}
