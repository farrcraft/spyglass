//! Structure templates as features place them (`StructureTemplate`), with their blocks' states
//! for each rotation (`docs/specs/features.md#templates`).

use std::collections::HashSet;

use serde_json::{Map, Value};
use spyglass_blocks::State;
use spyglass_data::bundle::BundleError;
use spyglass_data::codec::{At, LoadError, LoadErrorKind, field, float, full_id, object};
use spyglass_data::nbt::{self, Tag};
use spyglass_density::Resources;
use spyglass_rng::RandomSource;

use crate::blocks::Blocks;
use crate::region::{Doubt, Region};

/// `Rotation`, in `Rotation.values()`' order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rotation {
    /// `NONE`.
    None,
    /// `CLOCKWISE_90`.
    Clockwise90,
    /// `CLOCKWISE_180`.
    Clockwise180,
    /// `COUNTERCLOCKWISE_90`.
    Counterclockwise90,
}

impl Rotation {
    /// `Rotation.values()[i]`.
    pub const ALL: [Rotation; 4] = [
        Rotation::None,
        Rotation::Clockwise90,
        Rotation::Clockwise180,
        Rotation::Counterclockwise90,
    ];

    /// `StructureTemplate.transform` with mirror none and pivot zero.
    pub fn transform(self, [x, y, z]: [i32; 3]) -> [i32; 3] {
        match self {
            Rotation::None => [x, y, z],
            Rotation::Clockwise90 => [z.wrapping_neg(), y, x],
            Rotation::Clockwise180 => [x.wrapping_neg(), y, z.wrapping_neg()],
            Rotation::Counterclockwise90 => [z, y, x.wrapping_neg()],
        }
    }
}

/// One block of a template.
#[derive(Debug, Clone)]
pub struct TemplateBlock {
    /// Template-local position.
    pub pos: [i32; 3],
    /// Its state under each rotation, in [`Rotation::ALL`]' order.
    pub states: [State; 4],
    /// Whether it has block-entity NBT.
    pub nbt: bool,
}

/// A structure template: its size and each palette's blocks, in the order the game processes
/// them (sorted by y, then x, then z; those with NBT after the rest).
#[derive(Debug, Clone)]
pub struct Template {
    /// `size`, unrotated.
    pub size: [i32; 3],
    /// Each palette's blocks.
    pub palettes: Vec<Vec<TemplateBlock>>,
}

/// Properties that change under rotation, which only `axis` and horizontal `facing` of are
/// ported: a template using another can't be placed.
const ROTATING: [&str; 7] = [
    "rotation",
    "shape",
    "north",
    "east",
    "south",
    "west",
    "orientation",
];

impl Template {
    /// Loads template `id`, adding its states to `blocks`. Missing, an empty template
    /// (`getOrCreate`).
    pub fn load(
        resources: &dyn Resources,
        blocks: &mut Blocks,
        id: &str,
    ) -> Result<Template, LoadError> {
        let at = At::root(id);
        let bytes = match resources.structure_template(id) {
            Ok(bytes) => bytes,
            Err(BundleError::NotFound(_)) => {
                return Ok(Template {
                    size: [0; 3],
                    palettes: Vec::new(),
                });
            }
            Err(e) => return Err(at.error(e.into())),
        };
        let (_, root) = nbt::read_compressed(&bytes)
            .map_err(|_| at.error(LoadErrorKind::Invalid("not gzip-compressed NBT")))?;
        let size = int3(root.get("size"), &at.child("size"))?;
        let list = root
            .get("blocks")
            .and_then(Tag::as_list)
            .ok_or_else(|| at.error(LoadErrorKind::Missing("blocks")))?;
        let palettes: Vec<&[Tag]> = match root.get("palettes").and_then(Tag::as_list) {
            Some(p) => p.iter().filter_map(Tag::as_list).collect(),
            None => vec![
                root.get("palette")
                    .and_then(Tag::as_list)
                    .ok_or_else(|| at.error(LoadErrorKind::Missing("palette")))?,
            ],
        };
        let mut out = Vec::new();
        for palette in palettes {
            let mut states: Vec<[State; 4]> = Vec::new();
            for (i, entry) in palette.iter().enumerate() {
                states.push(rotated_states(
                    blocks,
                    entry,
                    &at.child("palette").child(&i.to_string()),
                )?);
            }
            let mut blocks_of = Vec::new();
            for (i, block) in list.iter().enumerate() {
                let at = at.child("blocks").child(&i.to_string());
                let pos = int3(block.get("pos"), &at.child("pos"))?;
                let state = block.get("state").and_then(Tag::as_int).unwrap_or(0);
                let states = usize::try_from(state)
                    .ok()
                    .and_then(|s| states.get(s))
                    .ok_or_else(|| {
                        at.child("state")
                            .error(LoadErrorKind::Invalid("no such palette entry"))
                    })?;
                blocks_of.push(TemplateBlock {
                    pos,
                    states: *states,
                    nbt: block.get("nbt").is_some(),
                });
            }
            blocks_of.sort_by_key(|b| (b.nbt, b.pos[1], b.pos[0], b.pos[2]));
            out.push(blocks_of);
        }
        Ok(Template {
            size,
            palettes: out,
        })
    }

    /// `getSize(rotation)`.
    pub fn rotated_size(&self, rotation: Rotation) -> [i32; 3] {
        let [x, y, z] = self.size;
        match rotation {
            Rotation::Clockwise90 | Rotation::Counterclockwise90 => [z, y, x],
            _ => [x, y, z],
        }
    }

    /// `getZeroPositionWithTransform(corner, NONE, rotation)`: where the template's own origin
    /// goes for its rotated box's least corner to be at `corner`.
    pub fn zero(&self, [cx, cy, cz]: [i32; 3], rotation: Rotation) -> [i32; 3] {
        // The template's last x and z, which a turn moves to the negative side.
        let (last_x, last_z) = (self.size[0] - 1, self.size[2] - 1);
        match rotation {
            Rotation::None => [cx, cy, cz],
            Rotation::Clockwise90 => [cx.wrapping_add(last_z), cy, cz],
            Rotation::Clockwise180 => [cx.wrapping_add(last_x), cy, cz.wrapping_add(last_z)],
            Rotation::Counterclockwise90 => [cx, cy, cz.wrapping_add(last_x)],
        }
    }

    /// `getBoundingBox(settings, zero)`: the min and max corners of the rotated box at `zero`.
    pub fn bounding_box(&self, zero: [i32; 3], rotation: Rotation) -> ([i32; 3], [i32; 3]) {
        let first = rotation.transform([0; 3]);
        let last = rotation.transform(self.size.map(|s| s - 1));
        let mut min = [0; 3];
        let mut max = [0; 3];
        for i in 0..3 {
            min[i] = first[i].min(last[i]).wrapping_add(zero[i]);
            max[i] = first[i].max(last[i]).wrapping_add(zero[i]);
        }
        (min, max)
    }
}

/// A palette entry's state under each rotation (`BlockState.rotate`).
fn rotated_states(blocks: &mut Blocks, entry: &Tag, at: &At<'_>) -> Result<[State; 4], LoadError> {
    let id = entry
        .get("id")
        .or_else(|| entry.get("Name"))
        .and_then(Tag::as_str)
        .map(full_id)
        .ok_or_else(|| at.error(LoadErrorKind::Missing("id")))?;
    let mut properties = Map::new();
    if let Some(compound) = entry
        .get("properties")
        .or_else(|| entry.get("Properties"))
        .and_then(Tag::as_compound)
    {
        for (k, v) in compound {
            let v = v
                .as_str()
                .ok_or_else(|| at.child(k).error(LoadErrorKind::Shape("a property value")))?;
            properties.insert(k.clone(), Value::from(v));
        }
    }
    if let Some(k) = ROTATING.iter().find(|k| properties.contains_key(**k)) {
        return Err(at.error(LoadErrorKind::Unsupported {
            what: "rotating block property in a feature's template",
            name: format!("{id}[{k}]"),
        }));
    }
    let mut out = [0; 4];
    for (index, rotation) in Rotation::ALL.iter().enumerate() {
        let mut props = properties.clone();
        if let Some(Value::String(axis)) = props.get("axis") {
            let quarter = matches!(
                rotation,
                Rotation::Clockwise90 | Rotation::Counterclockwise90
            );
            let axis = match axis.as_str() {
                "x" if quarter => "z",
                "z" if quarter => "x",
                a => a,
            }
            .to_string();
            props.insert("axis".into(), Value::from(axis));
        }
        if let Some(Value::String(facing)) = props.get("facing") {
            const HORIZONTAL: [&str; 4] = ["north", "east", "south", "west"];
            if let Some(i) = HORIZONTAL.iter().position(|h| h == facing) {
                let turn = match rotation {
                    Rotation::None => 0,
                    Rotation::Clockwise90 => 1,
                    Rotation::Clockwise180 => 2,
                    Rotation::Counterclockwise90 => 3,
                };
                props.insert("facing".into(), Value::from(HORIZONTAL[(i + turn) % 4]));
            }
        }
        out[index] =
            blocks.parse_state(&serde_json::json!({"Name": id, "Properties": props}), at)?;
    }
    Ok(out)
}

/// A structure processor features use.
#[derive(Debug, Clone)]
pub enum Processor {
    /// `block_rot`: keeps a block if `nextFloat() <= integrity`, drawn from the feature's random.
    BlockRot(f32),
    /// `protected_blocks`: drops a block whose target in the level is in the tag.
    Protected(crate::blocks::Tag),
    /// `rule`: the first rule whose input block matches (location and position always true)
    /// replaces the state. The block entity modifiers draw from a random of their own.
    Rule(Vec<(String, State)>),
}

impl Processor {
    /// A `worldgen/processor_list` by id, or written inline (`{"processors": [...]}` or a list).
    pub fn parse_list(
        json: &Value,
        at: &At<'_>,
        resources: &dyn Resources,
        blocks: &mut Blocks,
    ) -> Result<Vec<Processor>, LoadError> {
        let owned;
        let json = match json {
            Value::String(id) => {
                let id = full_id(id);
                owned = resources
                    .resource("worldgen/processor_list", &id)
                    .map_err(|e| At::root(&id).error(e.into()))?;
                &owned
            }
            other => other,
        };
        let list = match json {
            Value::Object(o) => o.get("processors").unwrap_or(&Value::Null),
            other => other,
        };
        let Value::Array(items) = list else {
            return Err(at.error(LoadErrorKind::Shape("a list of processors")));
        };
        items
            .iter()
            .enumerate()
            .map(|(i, p)| Processor::parse(p, &at.child(&i.to_string()), resources, blocks))
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
        Ok(match kind.as_str() {
            "minecraft:block_rot" => {
                if o.contains_key("rottable_blocks") {
                    return Err(at.error(LoadErrorKind::Unsupported {
                        what: "block_rot field",
                        name: "rottable_blocks".into(),
                    }));
                }
                Processor::BlockRot(float(field(o, "integrity", at)?, &at.child("integrity"))?)
            }
            "minecraft:protected_blocks" => {
                let tag = field(o, "value", at)?
                    .as_str()
                    .and_then(|s| s.strip_prefix('#'))
                    .ok_or_else(|| at.child("value").error(LoadErrorKind::Shape("a #tag")))?;
                Processor::Protected(blocks.tag(resources, &full_id(tag))?)
            }
            "minecraft:rule" => {
                let rules_at = at.child("rules");
                let Value::Array(rules) = field(o, "rules", at)? else {
                    return Err(rules_at.error(LoadErrorKind::Shape("a list of rules")));
                };
                let mut out = Vec::new();
                for (i, rule) in rules.iter().enumerate() {
                    let at = rules_at.child(&i.to_string());
                    let rule = object(rule, &at)?;
                    let kind_of = |key: &'static str| -> Result<String, LoadError> {
                        let predicate = object(field(rule, key, &at)?, &at.child(key))?;
                        Ok(field(predicate, "predicate_type", &at.child(key))?
                            .as_str()
                            .map(full_id)
                            .unwrap_or_default())
                    };
                    let always = |k: &str| k == "minecraft:always_true";
                    if kind_of("input_predicate")? != "minecraft:block_match"
                        || !always(&kind_of("location_predicate")?)
                        || (rule.contains_key("position_predicate")
                            && !always(&kind_of("position_predicate")?))
                    {
                        return Err(at.error(LoadErrorKind::Unsupported {
                            what: "processor rule",
                            name: "other than block_match and always_true".into(),
                        }));
                    }
                    let input = object(
                        field(rule, "input_predicate", &at)?,
                        &at.child("input_predicate"),
                    )?;
                    let block = field(input, "block", &at.child("input_predicate"))?
                        .as_str()
                        .map(full_id)
                        .unwrap_or_default();
                    let output = blocks.parse_state(
                        field(rule, "output_state", &at)?,
                        &at.child("output_state"),
                    )?;
                    out.push((block, output));
                }
                Processor::Rule(out)
            }
            _ => {
                return Err(at
                    .child("processor_type")
                    .error(LoadErrorKind::Unsupported {
                        what: "structure processor",
                        name: kind,
                    }));
            }
        })
    }
}

/// `StructureTemplate.placeInWorld` as features call it (mirror none, pivot zero, the feature's
/// random, no clipping that matters): a palette drawn, every block processed, then written. The
/// neighbours' shape updates aren't ported: where a placed block touches one that could change,
/// the region is put in doubt. Whether any block was placed.
pub fn place_in_world(
    region: &mut Region<'_>,
    template: &Template,
    zero: [i32; 3],
    rotation: Rotation,
    processors: &[Processor],
    random: &mut impl RandomSource,
) -> bool {
    if template.palettes.is_empty() {
        return false;
    }
    let palette =
        &template.palettes[random.next_int_bounded(template.palettes.len() as i32) as usize];
    let rotation_index = Rotation::ALL
        .iter()
        .position(|&x| x == rotation)
        .unwrap_or(0);
    let blocks = region.blocks();
    let mut placed: Vec<([i32; 3], State)> = Vec::with_capacity(palette.len());
    for block in palette {
        let offset = rotation.transform(block.pos);
        let world_pos = [
            zero[0].wrapping_add(offset[0]),
            zero[1].wrapping_add(offset[1]),
            zero[2].wrapping_add(offset[2]),
        ];
        // Processors test the unrotated state; the rotated one is written.
        let mut state = Some(block.states[rotation_index]);
        for processor in processors {
            let Some(kept) = state else { break };
            state = match processor {
                Processor::BlockRot(integrity) => {
                    (random.next_float() <= *integrity).then_some(kept)
                }
                Processor::Protected(tag) => {
                    (!blocks.is(region.get(world_pos), *tag)).then_some(kept)
                }
                Processor::Rule(rules) => {
                    let id = blocks.palette().block_id(block.states[0]);
                    Some(
                        rules
                            .iter()
                            .find(|(input, _)| input == id)
                            .map_or(kept, |(_, out)| *out),
                    )
                }
            };
        }
        if let Some(state) = state {
            placed.push((world_pos, state));
        }
    }
    if placed.is_empty() {
        return false;
    }
    // Waterlogging (`APPLY_WATERLOGGING`) isn't ported: a block that can hold water, placed
    // where water was or beside a source the template didn't place, is in doubt.
    let mut waterlog = false;
    for &(pos, state) in &placed {
        if blocks.waterloggable(state) && blocks.behaviour(region.get(pos)).fluid.is_some() {
            waterlog = true;
        }
    }
    for &(pos, state) in &placed {
        region.set(pos, state);
    }
    let sources: HashSet<[i32; 3]> = placed
        .iter()
        .filter(|(_, state)| blocks.behaviour(*state).fluid.is_some_and(|f| f.source))
        .map(|(pos, _)| *pos)
        .collect();
    for &(pos, state) in &placed {
        if !blocks.waterloggable(state) {
            continue;
        }
        for step in [[0, 1, 0], [0, 0, -1], [1, 0, 0], [0, 0, 1], [-1, 0, 0]] {
            let neighbour = [
                pos[0].wrapping_add(step[0]),
                pos[1].wrapping_add(step[1]),
                pos[2].wrapping_add(step[2]),
            ];
            let fluid = blocks.behaviour(region.get(neighbour)).fluid;
            if fluid.is_some_and(|f| f.source) && !sources.contains(&neighbour) {
                waterlog = true;
            }
        }
    }
    if waterlog {
        region.doubt(Doubt::Waterlogging);
    }
    // updateShapeAtEdge: each placed block's neighbours outside the template are told.
    let in_template: HashSet<[i32; 3]> = placed.iter().map(|(pos, _)| *pos).collect();
    for &(pos, _) in &placed {
        for step in [
            [0, -1, 0],
            [0, 1, 0],
            [0, 0, -1],
            [0, 0, 1],
            [-1, 0, 0],
            [1, 0, 0],
        ] {
            let neighbour = [
                pos[0].wrapping_add(step[0]),
                pos[1].wrapping_add(step[1]),
                pos[2].wrapping_add(step[2]),
            ];
            if in_template.contains(&neighbour) || region.outside_build_height(neighbour[1]) {
                continue;
            }
            let behaviour = blocks.behaviour(region.get(neighbour));
            // A fluid's update only schedules a tick.
            if behaviour.updates_shape && !behaviour.liquid {
                region.doubt(Doubt::ShapeUpdate);
            }
        }
    }
    true
}

fn int3(tag: Option<&Tag>, at: &At<'_>) -> Result<[i32; 3], LoadError> {
    let list = tag
        .and_then(Tag::as_list)
        .ok_or_else(|| at.error(LoadErrorKind::Shape("three integers")))?;
    if list.len() != 3 {
        return Err(at.error(LoadErrorKind::Shape("three integers")));
    }
    let mut out = [0; 3];
    for (i, t) in list.iter().enumerate() {
        out[i] = t
            .as_int()
            .ok_or_else(|| at.error(LoadErrorKind::Shape("three integers")))?;
    }
    Ok(out)
}
