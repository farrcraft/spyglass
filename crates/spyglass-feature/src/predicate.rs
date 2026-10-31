//! Block predicates (`BlockPredicate`): tests of the level's blocks at a position, for placement
//! filters and feature configurations (`docs/specs/features.md#block-predicates`).

use serde_json::Value;
use spyglass_blocks::State;
use spyglass_data::codec::{At, LoadError, LoadErrorKind, field, full_id, int, object};
use spyglass_density::Resources;

use crate::blocks::{Blocks, Heightmap, Tag};
use crate::region::Region;

/// A block predicate.
#[derive(Debug, Clone)]
pub enum BlockPredicate {
    /// `true`.
    True,
    /// `matching_blocks`: the block at the offset is one of these ids.
    MatchingBlocks([i32; 3], Vec<String>),
    /// `matching_block_tag`: the block at the offset is in the tag.
    MatchingBlockTag([i32; 3], Tag),
    /// `not`.
    Not(Box<BlockPredicate>),
    /// `all_of`: every one, in order, stopping at the first that fails.
    AllOf(Vec<BlockPredicate>),
    /// `any_of`: any, in order, stopping at the first that passes.
    AnyOf(Vec<BlockPredicate>),
    /// `inside_world_bounds`: the offset position's y is within the level.
    InsideWorldBounds([i32; 3]),
    /// `volume_match` (26.3 on): every position from `min` to `max`, both inclusive, x outer, then
    /// z, then y, passes `match`.
    VolumeMatch {
        /// The lowest offset.
        min: [i32; 3],
        /// The highest offset.
        max: [i32; 3],
        /// `match`.
        test: Box<BlockPredicate>,
    },
    /// `below_heightmap` (26.4-snapshot-6 on): y is below the heightmap's first free y.
    BelowHeightmap(Heightmap),
}

impl BlockPredicate {
    /// Parses one, loading the tags it names into `blocks`.
    pub fn parse(
        json: &Value,
        at: &At<'_>,
        resources: &dyn Resources,
        blocks: &mut Blocks,
    ) -> Result<BlockPredicate, LoadError> {
        let o = object(json, at)?;
        let kind = field(o, "type", at)?
            .as_str()
            .map(full_id)
            .ok_or_else(|| at.child("type").error(LoadErrorKind::Shape("a type id")))?;
        let offset = |key: &str| -> Result<[i32; 3], LoadError> {
            match o.get(key) {
                None => Ok([0; 3]),
                Some(v) => vec3(v, &at.child(key)),
            }
        };
        let list =
            |key: &'static str, blocks: &mut Blocks| -> Result<Vec<BlockPredicate>, LoadError> {
                let at = at.child(key);
                let Value::Array(items) = field(o, key, &at)? else {
                    return Err(at.error(LoadErrorKind::Shape("a list of block predicates")));
                };
                items
                    .iter()
                    .enumerate()
                    .map(|(i, p)| {
                        BlockPredicate::parse(p, &at.child(&i.to_string()), resources, blocks)
                    })
                    .collect()
            };
        Ok(match kind.as_str() {
            "minecraft:true" => BlockPredicate::True,
            "minecraft:matching_blocks" => {
                let ids = match field(o, "blocks", at)? {
                    Value::String(s) if s.starts_with('#') => {
                        spyglass_terrain::tag(resources, "tags/block", &s[1..])?
                    }
                    Value::String(s) => vec![full_id(s)],
                    Value::Array(items) => items
                        .iter()
                        .map(|v| {
                            v.as_str().map(full_id).ok_or_else(|| {
                                at.child("blocks").error(LoadErrorKind::Shape("a block id"))
                            })
                        })
                        .collect::<Result<_, _>>()?,
                    _ => return Err(at.child("blocks").error(LoadErrorKind::Shape("block ids"))),
                };
                BlockPredicate::MatchingBlocks(offset("offset")?, ids)
            }
            "minecraft:matching_block_tag" => {
                let tag = field(o, "tag", at)?
                    .as_str()
                    .map(full_id)
                    .ok_or_else(|| at.child("tag").error(LoadErrorKind::Shape("a tag id")))?;
                BlockPredicate::MatchingBlockTag(offset("offset")?, blocks.tag(resources, &tag)?)
            }
            "minecraft:not" => BlockPredicate::Not(Box::new(BlockPredicate::parse(
                field(o, "predicate", at)?,
                &at.child("predicate"),
                resources,
                blocks,
            )?)),
            "minecraft:all_of" => BlockPredicate::AllOf(list("predicates", blocks)?),
            "minecraft:any_of" => BlockPredicate::AnyOf(list("predicates", blocks)?),
            "minecraft:inside_world_bounds" => BlockPredicate::InsideWorldBounds(offset("offset")?),
            "minecraft:volume_match" => {
                let min = vec3(field(o, "min", at)?, &at.child("min"))?;
                let max = vec3(field(o, "max", at)?, &at.child("max"))?;
                if (0..3).any(|i| min[i] > max[i]) {
                    return Err(at.error(LoadErrorKind::Invalid(
                        "min bound cannot be larger than max bound",
                    )));
                }
                let test = BlockPredicate::parse(
                    field(o, "match", at)?,
                    &at.child("match"),
                    resources,
                    blocks,
                )?;
                BlockPredicate::VolumeMatch {
                    min,
                    max,
                    test: Box::new(test),
                }
            }
            "minecraft:below_heightmap" => {
                let name = field(o, "heightmap", at)?.as_str().unwrap_or_default();
                let h = Heightmap::parse(name).ok_or_else(|| {
                    at.child("heightmap")
                        .error(LoadErrorKind::Shape("a heightmap"))
                })?;
                BlockPredicate::BelowHeightmap(h)
            }
            _ => {
                return Err(at.child("type").error(LoadErrorKind::Unsupported {
                    what: "block predicate",
                    name: kind,
                }));
            }
        })
    }

    /// Tests the level at `pos`.
    pub fn test(&self, region: &mut Region<'_>, pos: [i32; 3]) -> bool {
        match self {
            BlockPredicate::True => true,
            BlockPredicate::MatchingBlocks(offset, ids) => {
                let s = region.get(add(pos, *offset));
                let id = region.blocks().palette().block_id(s);
                ids.iter().any(|b| b == id)
            }
            BlockPredicate::MatchingBlockTag(offset, tag) => {
                let s = region.get(add(pos, *offset));
                region.blocks().is(s, *tag)
            }
            BlockPredicate::Not(p) => !p.test(region, pos),
            BlockPredicate::AllOf(ps) => ps.iter().all(|p| p.test(region, pos)),
            BlockPredicate::AnyOf(ps) => ps.iter().any(|p| p.test(region, pos)),
            BlockPredicate::InsideWorldBounds(offset) => {
                !region.outside_build_height(pos[1].wrapping_add(offset[1]))
            }
            BlockPredicate::VolumeMatch { min, max, test } => {
                for dx in min[0]..=max[0] {
                    for dz in min[2]..=max[2] {
                        for dy in min[1]..=max[1] {
                            if !test.test(region, add(pos, [dx, dy, dz])) {
                                return false;
                            }
                        }
                    }
                }
                true
            }
            BlockPredicate::BelowHeightmap(h) => pos[1] < region.height(*h, pos[0], pos[2]),
        }
    }

    /// Tests a state alone, for the predicates a feature applies to a block it has in hand
    /// (`matching_block_tag`, `matching_blocks`, `not`, `all_of`, `any_of`, `true`, at offset
    /// 0). `None` for one that needs the level.
    pub fn test_state(&self, blocks: &Blocks, s: State) -> Option<bool> {
        Some(match self {
            BlockPredicate::True => true,
            BlockPredicate::MatchingBlocks([0, 0, 0], ids) => {
                let id = blocks.palette().block_id(s);
                ids.iter().any(|b| b == id)
            }
            BlockPredicate::MatchingBlockTag([0, 0, 0], tag) => blocks.is(s, *tag),
            BlockPredicate::Not(p) => !p.test_state(blocks, s)?,
            BlockPredicate::AllOf(ps) => {
                for p in ps {
                    if !p.test_state(blocks, s)? {
                        return Some(false);
                    }
                }
                true
            }
            BlockPredicate::AnyOf(ps) => {
                for p in ps {
                    if p.test_state(blocks, s)? {
                        return Some(true);
                    }
                }
                false
            }
            _ => return None,
        })
    }
}

fn add(a: [i32; 3], b: [i32; 3]) -> [i32; 3] {
    [
        a[0].wrapping_add(b[0]),
        a[1].wrapping_add(b[1]),
        a[2].wrapping_add(b[2]),
    ]
}

/// `Vec3i.offsetCodec(16)`: three ints, each from −16 to 16.
fn vec3(json: &Value, at: &At<'_>) -> Result<[i32; 3], LoadError> {
    let Value::Array(items) = json else {
        return Err(at.error(LoadErrorKind::Shape("three integers")));
    };
    if items.len() != 3 {
        return Err(at.error(LoadErrorKind::Shape("three integers")));
    }
    let mut out = [0; 3];
    for (i, v) in items.iter().enumerate() {
        let n = int(v, &at.child(&i.to_string()))?;
        if !(-16..=16).contains(&n) {
            return Err(at.error(LoadErrorKind::Invalid("an offset component past 16")));
        }
        out[i] = n;
    }
    Ok(out)
}
