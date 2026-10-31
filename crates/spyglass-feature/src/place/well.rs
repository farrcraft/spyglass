//! The desert well (`docs/specs/features.md#desert-well`): 26.2's `DesertWellFeature`, and from
//! 26.3 an `overlay` of `template` features, which the data says.

use serde_json::{Map, Value};
use spyglass_blocks::State;
use spyglass_data::codec::{At, LoadError, LoadErrorKind, field, full_id, int, object};
use spyglass_data::providers::IntProvider;
use spyglass_density::Resources;
use spyglass_rng::{RandomSource, WorldgenRandom};

use super::template::{Processor, Rotation, Template, place_in_world};
use crate::blocks::Blocks;
use crate::region::Region;

/// 26.2's `DesertWellFeature`.
#[derive(Debug, Clone)]
pub struct DesertWell {
    sand: State,
    slab: State,
    sandstone: State,
    water: State,
    suspicious_sand: State,
}

/// `Direction.Plane.HORIZONTAL`: north, east, south, west.
const HORIZONTAL: [[i32; 3]; 4] = [[0, 0, -1], [1, 0, 0], [0, 0, 1], [-1, 0, 0]];

impl DesertWell {
    /// Its blocks; the configured feature has no fields.
    pub fn load(blocks: &mut Blocks) -> Result<DesertWell, LoadError> {
        Ok(DesertWell {
            sand: blocks.state("minecraft:sand")?,
            slab: blocks.state("minecraft:sandstone_slab")?,
            sandstone: blocks.state("minecraft:sandstone")?,
            water: blocks.state("minecraft:water")?,
            suspicious_sand: blocks.state("minecraft:suspicious_sand")?,
        })
    }

    /// `DesertWellFeature.place` at the heightmap's position: whether it placed.
    pub fn place(
        &self,
        region: &mut Region<'_>,
        random: &mut WorldgenRandom,
        origin: [i32; 3],
    ) -> bool {
        let blocks = region.blocks();
        let mut pos = add(origin, [0, 1, 0]);
        while blocks.is_air(region.get(pos)) && pos[1] > region.min_y() + 2 {
            pos = add(pos, [0, -1, 0]);
        }
        if region.get(pos) != self.sand {
            return false;
        }
        for x in -2..=2 {
            for z in -2..=2 {
                if blocks.is_air(region.get(add(pos, [x, -1, z])))
                    && blocks.is_air(region.get(add(pos, [x, -2, z])))
                {
                    return false;
                }
            }
        }
        for y in -2..=0 {
            for x in -2..=2 {
                for z in -2..=2 {
                    region.set(add(pos, [x, y, z]), self.sandstone);
                }
            }
        }
        region.set(pos, self.water);
        for d in HORIZONTAL {
            region.set(add(pos, d), self.water);
        }
        let below = add(pos, [0, -1, 0]);
        region.set(below, self.sand);
        for d in HORIZONTAL {
            region.set(add(below, d), self.sand);
        }
        for x in -2..=2 {
            for z in -2..=2 {
                if x == -2 || x == 2 || z == -2 || z == 2 {
                    region.set(add(pos, [x, 1, z]), self.sandstone);
                }
            }
        }
        for d in [[2, 1, 0], [-2, 1, 0], [0, 1, 2], [0, 1, -2]] {
            region.set(add(pos, d), self.slab);
        }
        for x in -1..=1 {
            for z in -1..=1 {
                let s = if x == 0 && z == 0 {
                    self.sandstone
                } else {
                    self.slab
                };
                region.set(add(pos, [x, 4, z]), s);
            }
        }
        for y in 1..=3 {
            for d in [[-1, y, -1], [-1, y, 1], [1, y, -1], [1, y, 1]] {
                region.set(add(pos, d), self.sandstone);
            }
        }
        // The centre, east, south, west, north; the loot seed is the position's.
        let cells = [[0, 0, 0], [1, 0, 0], [0, 0, 1], [-1, 0, 0], [0, 0, -1]];
        for depth in [1, 2] {
            let c = cells[random.next_int_bounded(5) as usize];
            region.set(add(pos, [c[0], -depth, c[2]]), self.suspicious_sand);
        }
        true
    }
}

/// A child's placement modifier in an `overlay`.
#[derive(Debug, Clone)]
enum Child {
    /// `offset`.
    Offset([IntProvider; 3]),
    /// `randomly_selected`: one of the modifiers, by `nextInt(n)`.
    RandomlySelected(Vec<Child>),
}

impl Child {
    fn parse(json: &Value, at: &At<'_>) -> Result<Child, LoadError> {
        let o = object(json, at)?;
        let kind = field(o, "type", at)?
            .as_str()
            .map(full_id)
            .unwrap_or_default();
        Ok(match kind.as_str() {
            "minecraft:offset" => {
                let axis = |k: &'static str| IntProvider::parse(field(o, k, at)?, &at.child(k));
                Child::Offset([axis("x")?, axis("y")?, axis("z")?])
            }
            "minecraft:randomly_selected" => {
                let p_at = at.child("placements");
                let Value::Array(items) = field(o, "placements", at)? else {
                    return Err(p_at.error(LoadErrorKind::Shape("a list of modifiers")));
                };
                if items.is_empty() {
                    return Err(p_at.error(LoadErrorKind::Invalid("no placements")));
                }
                Child::RandomlySelected(
                    items
                        .iter()
                        .enumerate()
                        .map(|(i, m)| Child::parse(m, &p_at.child(&i.to_string())))
                        .collect::<Result<_, _>>()?,
                )
            }
            _ => {
                return Err(at.child("type").error(LoadErrorKind::Unsupported {
                    what: "modifier in an overlay",
                    name: kind,
                }));
            }
        })
    }

    fn apply(&self, random: &mut WorldgenRandom, pos: [i32; 3]) -> [i32; 3] {
        match self {
            Child::Offset([x, y, z]) => {
                let (dx, dy, dz) = (x.sample(random), y.sample(random), z.sample(random));
                add(pos, [dx, dy, dz])
            }
            Child::RandomlySelected(list) => {
                let i = random.next_int_bounded(list.len() as i32) as usize;
                list[i].apply(random, pos)
            }
        }
    }
}

/// A `template` feature: a weighted template, a random rotation, its processors.
#[derive(Debug, Clone)]
struct TemplateFeature {
    entries: Vec<(Template, i32, Vec<Rotation>)>,
    processors: Vec<Processor>,
}

impl TemplateFeature {
    fn load(
        o: &Map<String, Value>,
        at: &At<'_>,
        resources: &dyn Resources,
        blocks: &mut Blocks,
    ) -> Result<TemplateFeature, LoadError> {
        let t_at = at.child("templates");
        let Value::Array(items) = field(o, "templates", at)? else {
            return Err(t_at.error(LoadErrorKind::Shape("a weighted list of templates")));
        };
        let mut entries = Vec::new();
        for (i, item) in items.iter().enumerate() {
            let at = t_at.child(&i.to_string());
            let e = object(item, &at)?;
            let weight = int(field(e, "weight", &at)?, &at.child("weight"))?;
            let data = object(field(e, "data", &at)?, &at.child("data"))?;
            let id = field(data, "id", &at.child("data"))?
                .as_str()
                .map(full_id)
                .unwrap_or_default();
            let rotations = match data.get("rotations") {
                None => Rotation::ALL.to_vec(),
                Some(_) => {
                    return Err(at.child("data").error(LoadErrorKind::Unsupported {
                        what: "template feature field",
                        name: "rotations".into(),
                    }));
                }
            };
            entries.push((Template::load(resources, blocks, &id)?, weight, rotations));
        }
        if entries.is_empty() {
            return Err(t_at.error(LoadErrorKind::Invalid("no templates")));
        }
        let processors = match o.get("processors") {
            None => Vec::new(),
            Some(p) => Processor::parse_list(p, &at.child("processors"), resources, blocks)?,
        };
        Ok(TemplateFeature {
            entries,
            processors,
        })
    }

    /// `TemplateFeature.place`.
    fn place(
        &self,
        region: &mut Region<'_>,
        random: &mut WorldgenRandom,
        origin: [i32; 3],
    ) -> bool {
        // WeightedList.getRandomOrThrow: nextInt(total weight), drawn even for one entry.
        let total: i32 = self.entries.iter().map(|e| e.1).sum();
        let mut pick = random.next_int_bounded(total);
        let mut entry = &self.entries[0];
        for e in &self.entries {
            if pick < e.1 {
                entry = e;
                break;
            }
            pick -= e.1;
        }
        let (template, _, rotations) = entry;
        let rotation = rotations[random.next_int_bounded(rotations.len() as i32) as usize];
        // The template centred on the origin: west by half its x size, north by half its z,
        // each turned by the rotation.
        let west = rotation.transform([-1, 0, 0]);
        let north = rotation.transform([0, 0, -1]);
        let (hx, hz) = (template.size[0] / 2, template.size[2] / 2);
        let corner = add(
            origin,
            [
                west[0] * hx + north[0] * hz,
                0,
                west[2] * hx + north[2] * hz,
            ],
        );
        place_in_world(region, template, corner, rotation, &self.processors, random)
    }
}

/// An `overlay` of template features: each child's modifiers, then its feature, on the same
/// origin and random, in order.
#[derive(Debug, Clone)]
pub struct Overlay {
    children: Vec<(Vec<Child>, TemplateFeature)>,
}

impl Overlay {
    /// From the configured feature's fields.
    pub fn load(
        o: &Map<String, Value>,
        at: &At<'_>,
        resources: &dyn Resources,
        blocks: &mut Blocks,
    ) -> Result<Overlay, LoadError> {
        let f_at = at.child("features");
        let Value::Array(items) = field(o, "features", at)? else {
            return Err(f_at.error(LoadErrorKind::Shape("a list of placed features")));
        };
        let mut children = Vec::new();
        for (i, item) in items.iter().enumerate() {
            let at = f_at.child(&i.to_string());
            let p = object(item, &at)?;
            let modifiers = match p.get("placement") {
                None => Vec::new(),
                Some(Value::Array(ms)) => ms
                    .iter()
                    .enumerate()
                    .map(|(j, m)| Child::parse(m, &at.child("placement").child(&j.to_string())))
                    .collect::<Result<_, _>>()?,
                Some(_) => {
                    return Err(at.child("placement").error(LoadErrorKind::Shape("a list")));
                }
            };
            let f_at = at.child("feature");
            let feature = object(field(p, "feature", &at)?, &f_at)?;
            let kind = field(feature, "type", &f_at)?
                .as_str()
                .map(full_id)
                .unwrap_or_default();
            if kind != "minecraft:template" {
                return Err(f_at.error(LoadErrorKind::Unsupported {
                    what: "feature in an overlay",
                    name: kind,
                }));
            }
            children.push((
                modifiers,
                TemplateFeature::load(feature, &f_at, resources, blocks)?,
            ));
        }
        Ok(Overlay { children })
    }

    /// `OverlayFeature.place`: whether any child placed.
    pub fn place(
        &self,
        region: &mut Region<'_>,
        random: &mut WorldgenRandom,
        origin: [i32; 3],
    ) -> bool {
        let mut any = false;
        for (modifiers, feature) in &self.children {
            let mut pos = origin;
            for m in modifiers {
                pos = m.apply(random, pos);
            }
            any |= feature.place(region, random, pos);
        }
        any
    }
}

fn add(a: [i32; 3], b: [i32; 3]) -> [i32; 3] {
    [
        a[0].wrapping_add(b[0]),
        a[1].wrapping_add(b[1]),
        a[2].wrapping_add(b[2]),
    ]
}
