//! Block states, by index into a palette of their names (`docs/specs/material.md#block-states`).

use std::collections::{BTreeMap, HashMap};

use serde_json::{Map, Value};
use spyglass_data::codec::{At, LoadError, LoadErrorKind, full_id, object};
use spyglass_density::Resources;

/// A block state: an index into a [`Palette`].
pub type State = u16;

/// The states the material rule and the noise fill can place, by name: a block id, then its
/// non-default properties in brackets (`minecraft:grass_block[snowy=true]`), as the oracle's
/// block files name them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Palette {
    names: Vec<String>,
    index: HashMap<String, State>,
}

/// Air, always state 0.
pub const AIR: State = 0;

impl Default for Palette {
    fn default() -> Self {
        Self::new()
    }
}

impl Palette {
    /// A palette with air, water and lava, in that order.
    pub fn new() -> Self {
        let mut palette = Palette {
            names: Vec::new(),
            index: HashMap::new(),
        };
        for name in ["minecraft:air", "minecraft:water", "minecraft:lava"] {
            palette.state(name);
        }
        palette
    }

    /// The state named `name`, added if it's new.
    ///
    /// # Panics
    ///
    /// Past 65,536 states.
    pub fn state(&mut self, name: &str) -> State {
        if let Some(&s) = self.index.get(name) {
            return s;
        }
        let s = State::try_from(self.names.len()).expect("at most 65,536 block states");
        self.names.push(name.to_string());
        self.index.insert(name.to_string(), s);
        s
    }

    /// The state named `name`, if the palette has it.
    pub fn get(&self, name: &str) -> Option<State> {
        self.index.get(name).copied()
    }

    /// A state's name.
    pub fn name(&self, state: State) -> &str {
        &self.names[usize::from(state)]
    }

    /// `BlockState.isAir()`: air, cave air or void air.
    pub fn is_air(&self, state: State) -> bool {
        matches!(
            self.block_id(state),
            "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air"
        )
    }

    /// Whether the state holds a fluid (`getFluidState()` isn't empty): water or lava. No state
    /// the material rule places in vanilla data is waterlogged.
    pub fn is_fluid(&self, state: State) -> bool {
        matches!(self.block_id(state), "minecraft:water" | "minecraft:lava")
    }

    /// The block id of a state, without its properties.
    pub fn block_id(&self, state: State) -> &str {
        let name = self.name(state);
        name.split_once('[').map_or(name, |(id, _)| id)
    }
}

/// The water state.
pub const WATER: State = 1;
/// The lava state.
pub const LAVA: State = 2;

/// The data generator's report of every block and its states.
pub const BLOCKS_REPORT: &str = "reports/blocks.json";

/// Each block's properties, with the values they can take and their default state's, from
/// [`BLOCKS_REPORT`]: for naming states as the game's block files do, without default properties
/// (`docs/specs/material.md#block-states`).
#[derive(Debug, Clone, Default)]
pub struct BlockDefaults {
    /// Block id → property → (its values, the default state's value).
    blocks: HashMap<String, BTreeMap<String, (Vec<String>, String)>>,
}

impl BlockDefaults {
    /// Reads [`BLOCKS_REPORT`] from `resources`.
    pub fn load<R: Resources + ?Sized>(resources: &R) -> Result<Self, LoadError> {
        let root = At::root(BLOCKS_REPORT);
        let json = resources
            .report(BLOCKS_REPORT)
            .map_err(|e| root.error(e.into()))?;
        let mut blocks = HashMap::new();
        for (id, block) in object(&json, &root)? {
            let at = root.child(id);
            let block = object(block, &at)?;
            let Some(properties) = block.get("properties") else {
                blocks.insert(id.clone(), BTreeMap::new());
                continue;
            };
            let default = block
                .get("states")
                .and_then(Value::as_array)
                .and_then(|states| {
                    states
                        .iter()
                        .find(|s| s.get("default").and_then(Value::as_bool) == Some(true))
                })
                .and_then(|s| s.get("properties"))
                .and_then(Value::as_object)
                .ok_or_else(|| at.child("states").error(LoadErrorKind::Missing("default")))?;
            let at = at.child("properties");
            let mut props = BTreeMap::new();
            for (name, values) in object(properties, &at)? {
                let at = at.child(name);
                let values: Vec<String> = values
                    .as_array()
                    .and_then(|v| v.iter().map(|v| v.as_str().map(str::to_string)).collect())
                    .ok_or_else(|| at.error(LoadErrorKind::Shape("a list of values")))?;
                let value = default
                    .get(name)
                    .and_then(Value::as_str)
                    .ok_or_else(|| at.error(LoadErrorKind::Missing("default")))?;
                props.insert(name.clone(), (values, value.to_string()));
            }
            blocks.insert(id.clone(), props);
        }
        Ok(BlockDefaults { blocks })
    }

    /// The name of block `id` with `properties` (`BlockState`'s codec: `Name` and
    /// `Properties`): the id, then the properties that differ from the default state's, by
    /// name, in brackets. Unknown blocks, properties and values are errors.
    pub fn name(
        &self,
        id: &str,
        properties: &Map<String, Value>,
        at: &At<'_>,
    ) -> Result<String, LoadError> {
        let id = full_id(id);
        let block = self.blocks.get(&id).ok_or_else(|| {
            at.child("Name").error(LoadErrorKind::Unsupported {
                what: "block",
                name: id.clone(),
            })
        })?;
        let at = at.child("Properties");
        let mut changed = BTreeMap::new();
        for (name, value) in properties {
            let at = at.child(name);
            let value = value
                .as_str()
                .ok_or_else(|| at.error(LoadErrorKind::Shape("a property value")))?;
            let (values, default) = block.get(name).ok_or_else(|| {
                at.error(LoadErrorKind::Unsupported {
                    what: "block property",
                    name: format!("{id}[{name}]"),
                })
            })?;
            if !values.iter().any(|v| v == value) {
                return Err(at.error(LoadErrorKind::Unsupported {
                    what: "block property value",
                    name: format!("{id}[{name}={value}]"),
                }));
            }
            if value != default {
                changed.insert(name.as_str(), value);
            }
        }
        if changed.is_empty() {
            return Ok(id);
        }
        let list: Vec<String> = changed.iter().map(|(k, v)| format!("{k}={v}")).collect();
        Ok(format!("{id}[{}]", list.join(",")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn defaults() -> BlockDefaults {
        let report = json!({
            "minecraft:stone": {"states": [{"id": 1, "default": true}]},
            "minecraft:grass_block": {
                "properties": {"snowy": ["true", "false"]},
                "states": [
                    {"id": 8, "properties": {"snowy": "true"}},
                    {"id": 9, "default": true, "properties": {"snowy": "false"}},
                ],
            },
        });
        let resources = HashMap::from([(BLOCKS_REPORT.to_string(), report)]);
        BlockDefaults::load(&resources).unwrap()
    }

    fn name(id: &str, properties: Value) -> Result<String, LoadError> {
        defaults().name(id, properties.as_object().unwrap(), &At::root("test"))
    }

    #[test]
    fn states_are_named_without_default_properties() {
        assert_eq!(
            name("grass_block", json!({"snowy": "false"})).unwrap(),
            "minecraft:grass_block"
        );
        assert_eq!(
            name("minecraft:grass_block", json!({"snowy": "true"})).unwrap(),
            "minecraft:grass_block[snowy=true]"
        );
        assert_eq!(name("stone", json!({})).unwrap(), "minecraft:stone");
    }

    #[test]
    fn unknown_blocks_properties_and_values_are_errors() {
        let error = |id, p| name(id, p).unwrap_err().to_string();
        assert_eq!(
            error("dirt", json!({})),
            r#"test: Name: unsupported block "minecraft:dirt""#
        );
        assert_eq!(
            error("stone", json!({"axis": "y"})),
            r#"test: Properties.axis: unsupported block property "minecraft:stone[axis]""#
        );
        assert_eq!(
            error("grass_block", json!({"snowy": "maybe"})),
            r#"test: Properties.snowy: unsupported block property value "minecraft:grass_block[snowy=maybe]""#
        );
    }
}
