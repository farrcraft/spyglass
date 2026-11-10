//! Placement modifiers: how a placed feature turns a chunk's corner into the positions its
//! feature is tried at (`docs/specs/features.md#modifiers`).

use serde_json::Value;
use spyglass_data::codec::{At, LoadError, LoadErrorKind, double, field, full_id, int, object};
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
    /// `noise_based_count`: the position `ceil((noise(x / factor, z / factor) + offset) ·
    /// ratio)` times, from the biome info noise. No draws.
    NoiseBasedCount {
        /// `noise_to_count_ratio`.
        ratio: i32,
        /// `noise_factor`.
        factor: f64,
        /// `noise_offset`, default 0.
        offset: f64,
    },
    /// `noise_threshold_count`: the position `below_noise` times where the biome info noise at
    /// `(x / 200, z / 200)` is below `noise_level`, else `above_noise` (NaN too). No draws.
    NoiseThresholdCount {
        /// `noise_level`.
        level: f64,
        /// `below_noise`.
        below: i32,
        /// `above_noise`.
        above: i32,
    },
    /// `height_range`: y from the height provider.
    HeightRange(HeightProvider),
    /// `biome`: the position, if the biome at its block lists the placed feature.
    Biome,
    /// `fixed_placement`: each of `positions` in the chunk of the position, in order, in place
    /// of it. No draws.
    Fixed(Vec<[i32; 3]>),
    /// `offset` (26.3 on), or 26.2's `random_offset` (`xz_spread` for x and z, `y_spread`):
    /// the position moved by `x`, `y` and `z`, sampled in that order.
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
        /// Its shape, its block predicates left as `true`: where it can move a position, for
        /// boxes of tries (`Decorator::try_boxes`). Loaded without the blocks too.
        shape: Option<Read>,
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
    /// `surface_water_depth_filter`: the position, if `WORLD_SURFACE` less `OCEAN_FLOOR` (the
    /// live heightmaps' first free y) is at most `max`.
    SurfaceWaterDepth {
        /// `max_water_depth`.
        max: i32,
    },
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
            "minecraft:noise_based_count" => {
                let num = |k: &'static str| double(field(o, k, at)?, &at.child(k));
                Modifier::NoiseBasedCount {
                    ratio: int(
                        field(o, "noise_to_count_ratio", at)?,
                        &at.child("noise_to_count_ratio"),
                    )?,
                    factor: num("noise_factor")?,
                    offset: match o.get("noise_offset") {
                        None => 0.0,
                        Some(v) => double(v, &at.child("noise_offset"))?,
                    },
                }
            }
            "minecraft:noise_threshold_count" => Modifier::NoiseThresholdCount {
                level: double(field(o, "noise_level", at)?, &at.child("noise_level"))?,
                below: int(field(o, "below_noise", at)?, &at.child("below_noise"))?,
                above: int(field(o, "above_noise", at)?, &at.child("above_noise"))?,
            },
            "minecraft:height_range" => Modifier::HeightRange(HeightProvider::parse(
                field(o, "height", at)?,
                &at.child("height"),
            )?),
            "minecraft:biome" => Modifier::Biome,
            "minecraft:fixed_placement" => {
                let positions_at = at.child("positions");
                let list = field(o, "positions", at)?
                    .as_array()
                    .ok_or_else(|| positions_at.error(LoadErrorKind::Invalid("expected a list")))?;
                let positions = list
                    .iter()
                    .enumerate()
                    .map(|(i, p)| {
                        let p_at = positions_at.child(&format!("[{i}]"));
                        let xyz = p.as_array().filter(|a| a.len() == 3).ok_or_else(|| {
                            p_at.error(LoadErrorKind::Invalid("expected [x, y, z]"))
                        })?;
                        Ok([
                            int(&xyz[0], &p_at.child("[0]"))?,
                            int(&xyz[1], &p_at.child("[1]"))?,
                            int(&xyz[2], &p_at.child("[2]"))?,
                        ])
                    })
                    .collect::<Result<Vec<_>, LoadError>>()?;
                Modifier::Fixed(positions)
            }
            "minecraft:random_offset" => {
                // 26.2's: x, y, then z, the spread across sampled for each of x and z.
                let xz = IntProvider::parse(field(o, "xz_spread", at)?, &at.child("xz_spread"))?;
                let y = IntProvider::parse(field(o, "y_spread", at)?, &at.child("y_spread"))?;
                Modifier::Offset([xz.clone(), y, xz])
            }
            "minecraft:offset" => {
                let axis =
                    |name: &'static str| IntProvider::parse(field(o, name, at)?, &at.child(name));
                Modifier::Offset([axis("x")?, axis("y")?, axis("z")?])
            }
            k if READS_BLOCKS.contains(&k) => {
                let read = match reading {
                    Some(reading) => Read::parse(&kind, o, at, Some(reading))?,
                    None => None,
                };
                let shape = Read::parse(&kind, o, at, None).ok().flatten();
                Modifier::ReadsBlocks { kind, read, shape }
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
        mut reading: Option<&mut Reading<'_>>,
    ) -> Result<Option<Read>, LoadError> {
        let heightmap = |at: &At<'_>| -> Result<Heightmap, LoadError> {
            let name = field(o, "heightmap", at)?.as_str().unwrap_or_default();
            Heightmap::parse(name).ok_or_else(|| {
                at.child("heightmap")
                    .error(LoadErrorKind::Shape("a heightmap"))
            })
        };
        // Without the blocks, every predicate is `true`: the shape alone.
        let predicate = |key: &'static str,
                         reading: &mut Option<&mut Reading<'_>>|
         -> Result<BlockPredicate, LoadError> {
            let json = field(o, key, at)?;
            match reading {
                Some(r) => BlockPredicate::parse(json, &at.child(key), r.resources, r.blocks),
                None => Ok(BlockPredicate::True),
            }
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
                let target = predicate("target_condition", &mut reading)?;
                let allowed = match o.get("allowed_search_condition") {
                    None => BlockPredicate::True,
                    Some(_) => predicate("allowed_search_condition", &mut reading)?,
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
            "minecraft:block_predicate_filter" => {
                Read::Filter(predicate("predicate", &mut reading)?)
            }
            "minecraft:surface_water_depth_filter" => Read::SurfaceWaterDepth {
                max: int(
                    field(o, "max_water_depth", at)?,
                    &at.child("max_water_depth"),
                )?,
            },
            _ => return Ok(None),
        }))
    }
}
