//! The loot containers jigsaw pieces place, with their tables and loot seeds
//! (`docs/specs/loot.md#containers`).
//!
//! A piece places its template's blocks with block-entity NBT in the template's order (y, then x,
//! then z, whatever the rotation), each inside the chunk after passing the element's processors.
//! Each that's then a container draws `nextLong()` from the structure's random for its loot seed,
//! with a table or not. Processors draw from randoms of their own, seeded from the block's
//! position, so within a piece the seeds are the structure random's only draws. A feature element
//! draws from the same random, and what it draws isn't known: every seed after one isn't either.

use std::collections::HashMap;

use serde_json::Value;
use spyglass_data::codec::{At, LoadError, LoadErrorKind, field, float, full_id, object};
use spyglass_density::Resources;
use spyglass_rng::{LegacyRandomSource, RandomSource, WorldgenRandom, block_seed};
use spyglass_structure::{
    BoundingBox, JigsawPools, Layout, NbtBlock, PoolElement, Processors, Projection, Rotation,
    Template,
};

use crate::blocks::{Blocks, Tag};
use crate::region::{Doubt, Region};

/// Why a structure's container isn't certain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContainerDoubt {
    /// The start itself might not be there: its biome or another structure of its set.
    Start,
    /// Several starts of the structure reach the chunk, and the order they place in isn't known.
    StartOrder,
    /// A feature element placed earlier in the chunk drew from the same random: this placed
    /// feature, `None` for one written inline.
    FeatureElement(Option<String>),
    /// A processor whose effect on a container spyglass can't decide: its type.
    Processor(String),
    /// A block a processor reads could have been written by something spyglass doesn't place.
    Read(Doubt),
}

/// Why something placed before a structure's pieces, which spyglass doesn't place, could have
/// written within a box (least corner, greatest corner), for the structure by index; `None`
/// where nothing could have.
pub type Reads<'a> = dyn Fn([i32; 3], [i32; 3], usize) -> Option<Doubt> + 'a;

/// A container a structure's piece placed in the chunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructureContainer {
    /// The structure, by index in its `StructureSets`.
    pub structure: usize,
    /// Where.
    pub pos: [i32; 3],
    /// The block, e.g. `minecraft:chest`.
    pub block: String,
    /// The loot table's id.
    pub table: String,
    /// The loot seed; `None` where an earlier draw from the random isn't known.
    pub seed: Option<i64>,
    /// Why it isn't certain; `None` where it is, seed included.
    pub doubt: Option<ContainerDoubt>,
}

/// A structure processor, as far as what it does to a container.
#[derive(Debug, Clone)]
enum Processor {
    /// `block_ignore`: drops these blocks.
    Ignore(Vec<String>),
    /// `block_rot`: keeps a block not in `rottable` when that's given, and otherwise one whose
    /// position's random's `nextFloat()` is at most `integrity`.
    Rot {
        integrity: f32,
        rottable: Option<Rottable>,
    },
    /// `protected_blocks`: drops a block where the level's block is in the tag.
    Protected(Tag),
    /// `rule`: the first rule that holds replaces the block.
    Rule(Vec<Rule>),
    /// `nop`, `jigsaw_replacement`, `block_age`, `blackstone_replace`: none changes a container.
    Unchanged,
    /// Any other: what it does to a container isn't known. Its type.
    Unknown(String),
}

/// `block_rot`'s `rottable_blocks`.
#[derive(Debug, Clone)]
enum Rottable {
    Tag(Tag),
    Blocks(Vec<String>),
}

/// A `rule` processor's rule (`ProcessorRule`).
#[derive(Debug, Clone)]
struct Rule {
    input: Predicate,
    /// Whether the location predicate is `always_true`: any other reads the level.
    location_always: bool,
    /// Whether the position predicate is absent or `always_true`.
    position_always: bool,
    /// The output state's block.
    output: String,
    /// Whether the block entity modifier keeps the block's NBT (`passthrough`, the default).
    passthrough: bool,
}

/// A rule's input predicate (`RuleTest`), on the block being placed.
#[derive(Debug, Clone)]
enum Predicate {
    /// `always_true`.
    Always,
    /// `block_match`.
    Block(String),
    /// `blockstate_match`: this block, with properties spyglass doesn't compare.
    State(String),
    /// `random_block_match`: the block, then `nextFloat() < probability`.
    RandomBlock(String, f32),
    /// `random_blockstate_match`: this block, with properties and a chance.
    RandomState(String),
    /// `tag_match`.
    Tag(Tag),
    /// Any other.
    Other,
}

/// What processing did to a block with NBT.
enum Processed {
    /// Placed as this block.
    Placed(String),
    /// Dropped: not placed.
    Dropped,
    /// Not known.
    Unknown(ContainerDoubt),
}

/// The processor lists of a dimension's jigsaw pools, for finding their pieces' containers.
#[derive(Debug, Clone)]
pub struct StructureContainers {
    processors: HashMap<Processors, Vec<Processor>>,
    /// `#features_cannot_replace`, which ruined portals' chests don't replace.
    pub(crate) cannot_replace: Tag,
}

impl StructureContainers {
    /// Parses every single element's processors in `pools`, adding the tags they test to
    /// `blocks`. The bundle's block report must say which blocks are loot containers.
    pub fn load(
        resources: &dyn Resources,
        blocks: &mut Blocks,
        pools: &JigsawPools,
    ) -> Result<Self, LoadError> {
        if blocks.loot_container("minecraft:chest").is_none() {
            return Err(LoadError::new(
                "reports/block_behaviour.json",
                "loot_container",
                LoadErrorKind::Invalid(
                    "a block report without loot containers; extract the bundle again with this \
                     version of spyglass",
                ),
            ));
        }
        let mut out = StructureContainers {
            processors: HashMap::new(),
            cannot_replace: blocks.tag(resources, "minecraft:features_cannot_replace")?,
        };
        let mut stack: Vec<&PoolElement> = pools.elements().collect();
        while let Some(element) = stack.pop() {
            match element {
                PoolElement::Single(_, _, processors) => {
                    if !out.processors.contains_key(processors) {
                        let list = parse_list(processors, resources, blocks)?;
                        out.processors.insert(processors.clone(), list);
                    }
                }
                PoolElement::List(elements, _) => stack.extend(elements),
                PoolElement::Constrained(element, _) => stack.push(element),
                PoolElement::Feature(..) | PoolElement::Empty => {}
            }
        }
        Ok(out)
    }

    /// The containers a jigsaw start's pieces place in the region's chunk, appended to `out`,
    /// drawing their seeds from `random`, the structure's random for the chunk. `unknown` is
    /// why the random's draws aren't known, if they aren't; a feature element sets it.
    /// `reads` says why a block a processor reads (`protected_blocks`) might not be the
    /// region's, if it might not.
    #[allow(
        clippy::too_many_arguments,
        reason = "the decoration's state, passed through"
    )]
    pub fn jigsaw(
        &self,
        blocks: &Blocks,
        region: &mut Region<'_>,
        layout: &Layout,
        structure: usize,
        random: &mut WorldgenRandom,
        unknown: &mut Option<ContainerDoubt>,
        reads: &Reads<'_>,
        out: &mut Vec<StructureContainer>,
    ) {
        let [cx, cz] = region.center();
        // `ChunkGenerator.getWritableArea`: the chunk, from a block above the bottom.
        let chunk = BoundingBox {
            min: [cx * 16, region.min_y() + 1, cz * 16],
            max: [cx * 16 + 15, region.max_y(), cz * 16 + 15],
        };
        let mut place = Place {
            containers: self,
            blocks,
            region,
            chunk,
            structure,
            random,
            unknown,
            reads,
            out,
        };
        for (bbox, piece) in layout.pieces.iter().zip(&layout.placed) {
            if bbox.intersects(&chunk) {
                place.element(&piece.element, None, piece.pos, piece.rotation);
            }
        }
    }
}

/// One start's placing in a chunk.
struct Place<'a, 'r, 'b> {
    containers: &'a StructureContainers,
    blocks: &'a Blocks,
    region: &'a mut Region<'r>,
    chunk: BoundingBox,
    structure: usize,
    random: &'a mut WorldgenRandom,
    unknown: &'a mut Option<ContainerDoubt>,
    reads: &'b Reads<'b>,
    out: &'a mut Vec<StructureContainer>,
}

impl Place<'_, '_, '_> {
    /// `StructurePoolElement.place`: whether it placed. A list's members take its projection
    /// (`ListPoolElement.setProjectionOnEachElement`).
    fn element(
        &mut self,
        element: &PoolElement,
        projection: Option<Projection>,
        pos: [i32; 3],
        rotation: Rotation,
    ) -> bool {
        match element {
            PoolElement::Single(template, own, processors) => {
                let projection = projection.unwrap_or(*own);
                self.template(template, projection, processors, pos, rotation)
            }
            // Each in order, until one doesn't place.
            PoolElement::List(elements, own) => elements
                .iter()
                .all(|e| self.element(e, Some(projection.unwrap_or(*own)), pos, rotation)),
            PoolElement::Feature(_, _, id) => {
                self.unknown
                    .get_or_insert(ContainerDoubt::FeatureElement(id.clone()));
                true
            }
            PoolElement::Empty => true,
            PoolElement::Constrained(element, _) => {
                self.element(element, projection, pos, rotation)
            }
        }
    }

    /// `StructureTemplate.placeInWorld` for the blocks that draw: whether it placed.
    fn template(
        &mut self,
        template: &Template,
        projection: Projection,
        processors: &Processors,
        pos: [i32; 3],
        rotation: Rotation,
    ) -> bool {
        let Some(palette) = template.palette_at(pos) else {
            return false;
        };
        if template.size.contains(&0) {
            return false;
        }
        let list = &self.containers.processors[processors];
        for block in template.nbt_blocks(palette) {
            let offset = rotation.transform(block.pos);
            let world = [0, 1, 2].map(|i| pos[i].wrapping_add(offset[i]));
            // Blocks outside the chunk are skipped before processing, and never placed.
            if !self.chunk.contains(world) {
                continue;
            }
            let processed = if projection == Projection::TerrainMatching {
                // GravityProcessor: the block's height follows the surface.
                Processed::Unknown(ContainerDoubt::Processor("minecraft:gravity".into()))
            } else {
                self.process(list, block, world)
            };
            let (block_id, doubt) = match processed {
                Processed::Placed(id) => (id, None),
                Processed::Dropped => continue,
                // A block that might not be placed, or not as a container, might not draw.
                Processed::Unknown(why) => {
                    if self.blocks.loot_container(&block.block) == Some(true) {
                        self.unknown.get_or_insert(why.clone());
                        if let Some(table) = &block.loot_table {
                            self.out.push(StructureContainer {
                                structure: self.structure,
                                pos: world,
                                block: block.block.clone(),
                                table: table.clone(),
                                seed: None,
                                doubt: Some(why),
                            });
                        }
                    }
                    continue;
                }
            };
            if self.blocks.loot_container(&block_id) != Some(true) {
                continue;
            }
            // `RandomizableContainer`: a seed whether or not the NBT names a table.
            let seed = self.random.next_long();
            if let Some(table) = &block.loot_table {
                let doubt = doubt.or_else(|| self.unknown.clone());
                self.out.push(StructureContainer {
                    structure: self.structure,
                    pos: world,
                    block: block_id,
                    table: table.clone(),
                    seed: self.unknown.is_none().then_some(seed),
                    doubt,
                });
            }
        }
        true
    }

    /// The element's processors on one block with NBT at `world` (`processBlockInfos`).
    fn process(&mut self, list: &[Processor], block: &NbtBlock, world: [i32; 3]) -> Processed {
        let mut id = block.block.clone();
        for processor in list {
            match processor {
                Processor::Ignore(ignored) => {
                    if ignored.contains(&id) {
                        return Processed::Dropped;
                    }
                }
                Processor::Rot {
                    integrity,
                    rottable,
                } => {
                    // Tested on the template's own block.
                    let exempt = match rottable {
                        None => false,
                        Some(Rottable::Tag(tag)) => !self.blocks.block_in(&block.block, *tag),
                        Some(Rottable::Blocks(list)) => !list.contains(&block.block),
                    };
                    let mut random =
                        LegacyRandomSource::new(block_seed(world[0], world[1], world[2]));
                    if !exempt && random.next_float() > *integrity {
                        return Processed::Dropped;
                    }
                }
                Processor::Protected(tag) => {
                    if let Some(why) = (self.reads)(world, world, self.structure) {
                        return Processed::Unknown(ContainerDoubt::Read(why));
                    }
                    self.region.take_doubt();
                    let here = self.region.get(world);
                    if let Some(why) = self.region.take_doubt() {
                        return Processed::Unknown(ContainerDoubt::Read(why));
                    }
                    if self.blocks.is(here, *tag) {
                        return Processed::Dropped;
                    }
                }
                Processor::Rule(rules) => {
                    let mut random =
                        LegacyRandomSource::new(block_seed(world[0], world[1], world[2]));
                    for rule in rules {
                        let input = match &rule.input {
                            Predicate::Always => Some(true),
                            Predicate::Block(b) => Some(*b == id),
                            Predicate::RandomBlock(b, chance) => {
                                Some(*b == id && random.next_float() < *chance)
                            }
                            Predicate::State(b) | Predicate::RandomState(b) => {
                                (*b != id).then_some(false)
                            }
                            Predicate::Tag(tag) => Some(self.blocks.block_in(&id, *tag)),
                            Predicate::Other => None,
                        };
                        match input {
                            Some(false) => continue,
                            None => {
                                return Processed::Unknown(ContainerDoubt::Processor(
                                    "minecraft:rule".into(),
                                ));
                            }
                            Some(true) => {}
                        }
                        if !rule.location_always || !rule.position_always || !rule.passthrough {
                            return Processed::Unknown(ContainerDoubt::Processor(
                                "minecraft:rule".into(),
                            ));
                        }
                        id.clone_from(&rule.output);
                        break;
                    }
                }
                Processor::Unchanged => {}
                Processor::Unknown(kind) => {
                    return Processed::Unknown(ContainerDoubt::Processor(kind.clone()));
                }
            }
        }
        Processed::Placed(id)
    }
}

/// A processor list's `processors`, as [`Processors`] holds them.
fn parse_list(
    processors: &Processors,
    resources: &dyn Resources,
    blocks: &mut Blocks,
) -> Result<Vec<Processor>, LoadError> {
    let at = At::root("processors");
    let json: Value = serde_json::from_str(processors)
        .map_err(|_| at.error(LoadErrorKind::Shape("a list of processors")))?;
    let Value::Array(items) = json else {
        return Err(at.error(LoadErrorKind::Shape("a list of processors")));
    };
    items
        .iter()
        .enumerate()
        .map(|(i, p)| parse(p, &at.child(&i.to_string()), resources, blocks))
        .collect()
}

fn parse(
    json: &Value,
    at: &At<'_>,
    resources: &dyn Resources,
    blocks: &mut Blocks,
) -> Result<Processor, LoadError> {
    let o = object(json, at)?;
    let kind = field(o, "processor_type", at)?
        .as_str()
        .map(full_id)
        .unwrap_or_default();
    let tag = |value: &Value, blocks: &mut Blocks, at: &At<'_>| -> Result<Tag, LoadError> {
        let id = value
            .as_str()
            .and_then(|s| s.strip_prefix('#'))
            .ok_or_else(|| at.error(LoadErrorKind::Shape("a #tag")))?;
        blocks.tag(resources, &full_id(id))
    };
    Ok(match kind.as_str() {
        "minecraft:block_ignore" => {
            let Value::Array(states) = field(o, "blocks", at)? else {
                return Err(at
                    .child("blocks")
                    .error(LoadErrorKind::Shape("a list of states")));
            };
            Processor::Ignore(states.iter().filter_map(state_block).collect())
        }
        "minecraft:block_rot" => Processor::Rot {
            integrity: float(field(o, "integrity", at)?, &at.child("integrity"))?,
            rottable: match o.get("rottable_blocks") {
                None => None,
                Some(Value::String(s)) if s.starts_with('#') => Some(Rottable::Tag(tag(
                    &Value::String(s.clone()),
                    blocks,
                    &at.child("rottable_blocks"),
                )?)),
                Some(Value::String(s)) => Some(Rottable::Blocks(vec![full_id(s)])),
                Some(Value::Array(list)) => Some(Rottable::Blocks(
                    list.iter().filter_map(Value::as_str).map(full_id).collect(),
                )),
                Some(_) => {
                    return Err(at
                        .child("rottable_blocks")
                        .error(LoadErrorKind::Shape("blocks or a #tag")));
                }
            },
        },
        "minecraft:protected_blocks" => {
            Processor::Protected(tag(field(o, "value", at)?, blocks, &at.child("value"))?)
        }
        "minecraft:rule" => {
            let rules_at = at.child("rules");
            let Value::Array(rules) = field(o, "rules", at)? else {
                return Err(rules_at.error(LoadErrorKind::Shape("a list of rules")));
            };
            let mut out = Vec::with_capacity(rules.len());
            for (i, rule) in rules.iter().enumerate() {
                let at = rules_at.child(&i.to_string());
                let rule = object(rule, &at)?;
                let kind_of = |key: &str| {
                    rule.get(key)
                        .and_then(|p| p.get("predicate_type"))
                        .and_then(Value::as_str)
                        .map(full_id)
                };
                let input_json = field(rule, "input_predicate", &at)?;
                let block_of = |key: &str| {
                    input_json
                        .get(key)
                        .and_then(|b| b.as_str().map(full_id).or_else(|| state_block(b)))
                        .unwrap_or_default()
                };
                let input = match kind_of("input_predicate").as_deref() {
                    Some("minecraft:always_true") => Predicate::Always,
                    Some("minecraft:block_match") => Predicate::Block(block_of("block")),
                    Some("minecraft:blockstate_match") => Predicate::State(block_of("block_state")),
                    Some("minecraft:random_block_match") => Predicate::RandomBlock(
                        block_of("block"),
                        float(
                            field(object(input_json, &at)?, "probability", &at)?,
                            &at.child("probability"),
                        )?,
                    ),
                    Some("minecraft:random_blockstate_match") => {
                        Predicate::RandomState(block_of("block_state"))
                    }
                    Some("minecraft:tag_match") => Predicate::Tag(tag(
                        &Value::String(format!(
                            "#{}",
                            input_json
                                .get("tag")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                        )),
                        blocks,
                        &at.child("input_predicate"),
                    )?),
                    _ => Predicate::Other,
                };
                let always = |key: &str| kind_of(key).is_none_or(|k| k == "minecraft:always_true");
                let modifier = rule
                    .get("block_entity_modifier")
                    .and_then(|m| m.get("type"))
                    .and_then(Value::as_str)
                    .map(full_id);
                out.push(Rule {
                    input,
                    location_always: always("location_predicate"),
                    position_always: always("position_predicate"),
                    output: state_block(field(rule, "output_state", &at)?).unwrap_or_default(),
                    passthrough: modifier.is_none_or(|m| m == "minecraft:passthrough"),
                });
            }
            Processor::Rule(out)
        }
        "minecraft:nop"
        | "minecraft:jigsaw_replacement"
        | "minecraft:block_age"
        | "minecraft:blackstone_replace" => Processor::Unchanged,
        _ => Processor::Unknown(kind),
    })
}

/// A block state's block in JSON: 26.2's `{"Name"}`, 26.3's `{"id"}`, or an id alone.
fn state_block(json: &Value) -> Option<String> {
    match json {
        Value::String(id) => Some(full_id(id)),
        Value::Object(o) => o
            .get("Name")
            .or_else(|| o.get("id"))
            .and_then(Value::as_str)
            .map(full_id),
        _ => None,
    }
}
