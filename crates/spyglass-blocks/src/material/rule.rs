//! The material rule's data (26.3 on): rules and conditions from `noise_settings.material_rule`
//! and the `worldgen/material_rule` and `worldgen/material_condition` registries, where any rule
//! or condition can be another's id (`docs/specs/material.md#rules`).

use serde_json::Value;
use spyglass_data::anchor::VerticalAnchor;
use spyglass_data::codec::{
    At, LoadError, LoadErrorKind, double, field, float, full_id, int, int_in, object,
};
use spyglass_density::{Df, Loader, Resources};

use crate::state::{BlockDefaults, Palette, State};

/// The rules registry.
const RULES: &str = "worldgen/material_rule";
/// The conditions registry.
const CONDITIONS: &str = "worldgen/material_condition";

/// A rule: the block a position gets, or none to try the next.
#[derive(Debug, Clone)]
pub enum Rule {
    /// `block`: always `result_state`.
    Block(State),
    /// `sequence`: the first rule that gives a block.
    Sequence(Vec<Rule>),
    /// `condition`: `then_run` where `if_true` holds.
    Condition(Box<Condition>, Box<Rule>),
    /// `bandlands`: the clay band at the position.
    Bandlands,
    /// `ore_vein`.
    OreVein(Box<OreVein>),
}

impl Rule {
    /// Whether the rule or any rule in it is an ore vein.
    pub fn has_ore_vein(&self) -> bool {
        match self {
            Rule::OreVein(_) => true,
            Rule::Sequence(rules) => rules.iter().any(Rule::has_ore_vein),
            Rule::Condition(_, then) => then.has_ore_vein(),
            Rule::Block(_) | Rule::Bandlands => false,
        }
    }
}

/// `ore_vein`'s fields.
#[derive(Debug, Clone)]
pub struct OreVein {
    /// `ore_block`.
    pub ore_block: State,
    /// `raw_ore_block`.
    pub raw_ore_block: State,
    /// `filler_block`.
    pub filler_block: State,
    /// `raw_ore_chance`, in 0 to 1.
    pub raw_ore_chance: f32,
    /// `density`: where it's positive, a vein can be.
    pub density: Df,
    /// `richness`: the chance of ore rather than filler.
    pub richness: Df,
    /// `filler_gap`: ore only where it's negative.
    pub filler_gap: Df,
}

/// A condition on the position being tried.
#[derive(Debug, Clone)]
pub enum Condition {
    /// `biome`: the position's biome is one of these ids.
    Biome(Vec<String>),
    /// `noise_threshold`: the noise at the column (or the position, if `is_3d`) is in
    /// `min..=max`.
    NoiseThreshold {
        /// The noise's id.
        noise: String,
        /// `min_threshold`.
        min: f64,
        /// `max_threshold`.
        max: f64,
        /// `is_3d`.
        is_3d: bool,
    },
    /// `vertical_gradient`: true below, false above, random in between.
    VerticalGradient {
        /// The name of the random factory, `random_name`.
        random_name: String,
        /// `true_at_and_below`.
        true_at_and_below: VerticalAnchor,
        /// `false_at_and_above`.
        false_at_and_above: VerticalAnchor,
    },
    /// `y_above`.
    YAbove {
        /// `anchor`.
        anchor: VerticalAnchor,
        /// `surface_depth_multiplier`, in −20 to 20.
        surface_depth_multiplier: i32,
        /// `add_stone_depth`.
        add_stone_depth: bool,
    },
    /// `water`.
    Water {
        /// `offset`.
        offset: i32,
        /// `surface_depth_multiplier`, in −20 to 20.
        surface_depth_multiplier: i32,
        /// `add_stone_depth`.
        add_stone_depth: bool,
    },
    /// `temperature`: cold enough to snow.
    Temperature,
    /// `steep`.
    Steep,
    /// `hole`: the column's surface depth is 0 or less.
    Hole,
    /// `above_preliminary_surface`.
    AbovePreliminarySurface,
    /// `not`.
    Not(Box<Condition>),
    /// `stone_depth`.
    StoneDepth {
        /// `offset`.
        offset: i32,
        /// `add_surface_depth`.
        add_surface_depth: bool,
        /// `secondary_depth_range`.
        secondary_depth_range: i32,
        /// `surface_type` is `ceiling` (otherwise `floor`).
        ceiling: bool,
    },
}

/// Loads rules and conditions, with their states into a palette and their density functions
/// through a density loader.
pub struct RuleLoader<'r, 'p, R: Resources + ?Sized> {
    resources: &'r R,
    pub(crate) functions: Loader<'r, R>,
    palette: &'p mut Palette,
    /// Read when a state first names properties.
    defaults: Option<BlockDefaults>,
    /// Ids being loaded, for finding cycles.
    loading: Vec<String>,
}

impl<'r, 'p, R: Resources + ?Sized> RuleLoader<'r, 'p, R> {
    /// A loader reading from `resources`, adding states to `palette`.
    pub fn new(resources: &'r R, palette: &'p mut Palette) -> Self {
        RuleLoader {
            resources,
            functions: Loader::new(resources),
            palette,
            defaults: None,
            loading: Vec::new(),
        }
    }

    /// A rule, inline or by id, at `at`.
    pub fn rule(&mut self, json: &Value, at: &At<'_>) -> Result<Rule, LoadError> {
        if let Value::String(id) = json {
            return self.reference(RULES, id, |l, json, at| l.rule(json, at));
        }
        let o = object(json, at)?;
        let kind = full_id(type_of(o, at)?);
        Ok(match kind.as_str() {
            "minecraft:block" => Rule::Block(self.state(field(o, "result_state", at)?, at)?),
            "minecraft:sequence" => {
                let at = at.child("sequence");
                let list = field(o, "sequence", &at)?
                    .as_array()
                    .ok_or_else(|| at.error(LoadErrorKind::Shape("a list of rules")))?;
                if list.is_empty() {
                    return Err(
                        at.error(LoadErrorKind::Invalid("a sequence needs at least one rule"))
                    );
                }
                let rules = list
                    .iter()
                    .enumerate()
                    .map(|(i, r)| self.rule(r, &at.child(&i.to_string())))
                    .collect::<Result<_, _>>()?;
                Rule::Sequence(rules)
            }
            "minecraft:condition" => {
                let condition = self.condition(field(o, "if_true", at)?, &at.child("if_true"))?;
                let then = self.rule(field(o, "then_run", at)?, &at.child("then_run"))?;
                Rule::Condition(Box::new(condition), Box::new(then))
            }
            "minecraft:bandlands" => Rule::Bandlands,
            "minecraft:ore_vein" => {
                let state = |l: &mut Self, name: &'static str| {
                    l.state(field(o, name, at)?, &at.child(name))
                };
                let chance_at = at.child("raw_ore_chance");
                let raw_ore_chance = float(field(o, "raw_ore_chance", at)?, &chance_at)?;
                if !(0.0..=1.0).contains(&raw_ore_chance) {
                    return Err(chance_at.error(LoadErrorKind::Invalid("not in 0 to 1")));
                }
                let function = |l: &mut Self, name: &'static str| {
                    l.functions.inline(
                        at.resource(),
                        &format!("{}.{name}", at.path()),
                        field(o, name, at)?,
                    )
                };
                Rule::OreVein(Box::new(OreVein {
                    ore_block: state(self, "ore_block")?,
                    raw_ore_block: state(self, "raw_ore_block")?,
                    filler_block: state(self, "filler_block")?,
                    raw_ore_chance,
                    density: function(self, "density")?,
                    richness: function(self, "richness")?,
                    filler_gap: function(self, "filler_gap")?,
                }))
            }
            _ => {
                return Err(at.child("type").error(LoadErrorKind::Unsupported {
                    what: "material rule type",
                    name: kind,
                }));
            }
        })
    }

    /// A condition, inline or by id, at `at`.
    pub fn condition(&mut self, json: &Value, at: &At<'_>) -> Result<Condition, LoadError> {
        if let Value::String(id) = json {
            return self.reference(CONDITIONS, id, |l, json, at| l.condition(json, at));
        }
        let o = object(json, at)?;
        let kind = full_id(type_of(o, at)?);
        let boolean = |name: &'static str| {
            field(o, name, at)?
                .as_bool()
                .ok_or_else(|| at.child(name).error(LoadErrorKind::Shape("true or false")))
        };
        let depth_multiplier = || {
            int_in(
                field(o, "surface_depth_multiplier", at)?,
                -20,
                20,
                &at.child("surface_depth_multiplier"),
            )
        };
        let anchor =
            |name: &'static str| VerticalAnchor::parse(field(o, name, at)?, &at.child(name));
        Ok(match kind.as_str() {
            "minecraft:biome" => {
                Condition::Biome(biomes(field(o, "biome_is", at)?, &at.child("biome_is"))?)
            }
            "minecraft:noise_threshold" => Condition::NoiseThreshold {
                noise: {
                    let id = full_id(field(o, "noise", at)?.as_str().ok_or_else(|| {
                        at.child("noise").error(LoadErrorKind::Shape("a noise id"))
                    })?);
                    self.functions.noise(&id)?;
                    id
                },
                min: double(field(o, "min_threshold", at)?, &at.child("min_threshold"))?,
                max: double(field(o, "max_threshold", at)?, &at.child("max_threshold"))?,
                is_3d: match o.get("is_3d") {
                    None => false,
                    Some(_) => boolean("is_3d")?,
                },
            },
            "minecraft:vertical_gradient" => {
                Condition::VerticalGradient {
                    random_name: full_id(field(o, "random_name", at)?.as_str().ok_or_else(
                        || at.child("random_name").error(LoadErrorKind::Shape("an id")),
                    )?),
                    true_at_and_below: anchor("true_at_and_below")?,
                    false_at_and_above: anchor("false_at_and_above")?,
                }
            }
            "minecraft:y_above" => Condition::YAbove {
                anchor: anchor("anchor")?,
                surface_depth_multiplier: depth_multiplier()?,
                add_stone_depth: boolean("add_stone_depth")?,
            },
            "minecraft:water" => Condition::Water {
                offset: int(field(o, "offset", at)?, &at.child("offset"))?,
                surface_depth_multiplier: depth_multiplier()?,
                add_stone_depth: boolean("add_stone_depth")?,
            },
            "minecraft:temperature" => Condition::Temperature,
            "minecraft:steep" => Condition::Steep,
            "minecraft:hole" => Condition::Hole,
            "minecraft:above_preliminary_surface" => Condition::AbovePreliminarySurface,
            "minecraft:not" => Condition::Not(Box::new(
                self.condition(field(o, "invert", at)?, &at.child("invert"))?,
            )),
            "minecraft:stone_depth" => Condition::StoneDepth {
                offset: int(field(o, "offset", at)?, &at.child("offset"))?,
                add_surface_depth: boolean("add_surface_depth")?,
                secondary_depth_range: int(
                    field(o, "secondary_depth_range", at)?,
                    &at.child("secondary_depth_range"),
                )?,
                ceiling: match field(o, "surface_type", at)?.as_str() {
                    Some("ceiling") => true,
                    Some("floor") => false,
                    _ => {
                        return Err(at
                            .child("surface_type")
                            .error(LoadErrorKind::Shape("floor or ceiling")));
                    }
                },
            },
            _ => {
                return Err(at.child("type").error(LoadErrorKind::Unsupported {
                    what: "material condition type",
                    name: kind,
                }));
            }
        })
    }

    /// The entry `id` of `registry`, loaded by `load`, with cycles reported.
    fn reference<T>(
        &mut self,
        registry: &str,
        id: &str,
        load: impl FnOnce(&mut Self, &Value, &At<'_>) -> Result<T, LoadError>,
    ) -> Result<T, LoadError> {
        let id = full_id(id);
        let root = At::root(&id);
        if self.loading.contains(&id) {
            let mut cycle = self.loading.clone();
            cycle.push(id.clone());
            return Err(root.error(LoadErrorKind::Cycle(cycle)));
        }
        let json = self
            .resources
            .resource(registry, &id)
            .map_err(|e| root.error(e.into()))?;
        self.loading.push(id.clone());
        let result = load(self, &json, &root);
        self.loading.pop();
        result
    }

    /// A block state: an id (the block's default state), or `{"Name": id, "Properties": {...}}`.
    pub(crate) fn state(&mut self, json: &Value, at: &At<'_>) -> Result<State, LoadError> {
        let name = state_name(self.resources, &mut self.defaults, json, at)?;
        Ok(self.palette.state(&name))
    }
}

/// The name of a block state as `BlockState`'s codec reads it: an id (the block's default
/// state), or `{"Name": id, "Properties": {...}}`. `defaults` are read from `resources` the
/// first time a state has properties.
pub(crate) fn state_name<R: Resources + ?Sized>(
    resources: &R,
    defaults: &mut Option<BlockDefaults>,
    json: &Value,
    at: &At<'_>,
) -> Result<String, LoadError> {
    match json {
        Value::String(id) => Ok(full_id(id)),
        Value::Object(o) => {
            let id = o
                .get("Name")
                .and_then(Value::as_str)
                .ok_or_else(|| at.error(LoadErrorKind::Missing("Name")))?;
            match o.get("Properties") {
                None => Ok(full_id(id)),
                Some(properties) => {
                    let properties = object(properties, &at.child("Properties"))?;
                    let defaults = match defaults {
                        Some(d) => d,
                        None => defaults.insert(BlockDefaults::load(resources)?),
                    };
                    defaults.name(id, properties, at)
                }
            }
        }
        _ => Err(at.error(LoadErrorKind::Shape("a block state"))),
    }
}

/// A map's `type`.
fn type_of<'v>(o: &'v serde_json::Map<String, Value>, at: &At<'_>) -> Result<&'v str, LoadError> {
    field(o, "type", at)?
        .as_str()
        .ok_or_else(|| at.child("type").error(LoadErrorKind::Shape("a type id")))
}

/// `biome_is`: one biome id or a list of them. Tags aren't supported yet: vanilla doesn't use
/// them here.
fn biomes(json: &Value, at: &At<'_>) -> Result<Vec<String>, LoadError> {
    let one = |v: &Value, at: &At<'_>| match v.as_str() {
        Some(t) if t.starts_with('#') => Err(at.error(LoadErrorKind::Unsupported {
            what: "biome tag in a material condition",
            name: t.to_string(),
        })),
        Some(id) => Ok(full_id(id)),
        None => Err(at.error(LoadErrorKind::Shape("a biome id"))),
    };
    match json {
        Value::Array(list) => list
            .iter()
            .enumerate()
            .map(|(i, v)| one(v, &at.child(&i.to_string())))
            .collect(),
        v => Ok(vec![one(v, at)?]),
    }
}
