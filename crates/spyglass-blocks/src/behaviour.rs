//! Each block state's behaviour, from the extractor's block report (ADR 0054,
//! `docs/specs/data-sources.md#block-report`): what features read about the blocks they meet.

use std::collections::HashMap;

use serde_json::{Map, Value};
use spyglass_data::bundle::BundleError;
use spyglass_data::codec::{At, LoadError, LoadErrorKind, object};
use spyglass_density::Resources;

use crate::state::{BLOCKS_REPORT, BlockDefaults};

/// The classes whose `updateShape` only schedules a tick and returns the state unchanged (26.3's
/// bytecode): falling blocks, fluids, slabs (for their water) and brushable blocks.
pub const TICK_ONLY: [&str; 4] = ["FallingBlock", "LiquidBlock", "SlabBlock", "BrushableBlock"];

/// The block report, in a bundle.
pub const BLOCK_BEHAVIOUR_REPORT: &str = "reports/block_behaviour.json";

/// A fluid a state holds (`getFluidState()`, when it isn't empty).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fluid {
    /// Water, or lava.
    pub kind: FluidKind,
    /// `getAmount()`: 8 for a source.
    pub amount: u8,
    /// `isSource()`.
    pub source: bool,
}

/// The fluids of vanilla blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FluidKind {
    /// `minecraft:water` or `minecraft:flowing_water`.
    Water,
    /// `minecraft:lava` or `minecraft:flowing_lava`.
    Lava,
}

/// One state's behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Behaviour {
    /// `isAir()`.
    pub air: bool,
    /// `liquid()`.
    pub liquid: bool,
    /// `isSolid()`: the legacy solid flag, from the collision shape.
    pub solid: bool,
    /// `canBeReplaced()`.
    pub replaceable: bool,
    /// `isSolidRender()`.
    pub solid_render: bool,
    /// `blocksMotion()`, in versions that have it (26.2).
    pub blocks_motion: bool,
    /// The block is a `LeavesBlock`.
    pub leaves: bool,
    /// A neighbour's change can change the block (`updateShape`): a fence joins, a plant
    /// breaks. False for blocks whose override only schedules a tick ([`TICK_ONLY`]).
    pub updates_shape: bool,
    /// Its fluid, if any.
    pub fluid: Option<Fluid>,
}

/// Every state's behaviour, by the name a [`crate::Palette`] gives it.
#[derive(Debug, Clone, Default)]
pub struct BlockBehaviour {
    states: HashMap<String, Behaviour>,
}

impl BlockBehaviour {
    /// Reads [`BLOCK_BEHAVIOUR_REPORT`] and names its states from [`BLOCKS_REPORT`].
    pub fn load<R: Resources + ?Sized>(resources: &R) -> Result<Self, LoadError> {
        let root = At::root(BLOCK_BEHAVIOUR_REPORT);
        let report = resources.report(BLOCK_BEHAVIOUR_REPORT).map_err(|e| {
            // Bundles extracted before the report was added don't have it.
            let e = match e {
                BundleError::NotFound(_) => BundleError::Layout(
                    "the bundle has no block behaviour report; extract it again with this \
                     version of spyglass"
                        .to_string(),
                ),
                e => e,
            };
            root.error(e.into())
        })?;
        let names = state_names(resources)?;
        let report = object(&report, &root)?;
        let list = report
            .get("states")
            .and_then(Value::as_array)
            .ok_or_else(|| root.child("states").error(LoadErrorKind::Missing("states")))?;
        let mut states = HashMap::with_capacity(list.len());
        for (i, entry) in list.iter().enumerate() {
            let at = root.child("states").child(&i.to_string());
            let o = object(entry, &at)?;
            let id = o
                .get("id")
                .and_then(Value::as_u64)
                .ok_or_else(|| at.child("id").error(LoadErrorKind::Shape("a state id")))?;
            let name = names.get(&id).ok_or_else(|| {
                at.child("id").error(LoadErrorKind::Invalid(
                    "a state id that reports/blocks.json doesn't list",
                ))
            })?;
            let flag = |k: &str| o.get(k).and_then(Value::as_bool).unwrap_or(false);
            let fluid = match o.get("fluid") {
                None => None,
                Some(f) => Some(fluid(f, &at.child("fluid"))?),
            };
            states.insert(
                name.clone(),
                Behaviour {
                    air: flag("air"),
                    liquid: flag("liquid"),
                    solid: flag("solid"),
                    replaceable: flag("replaceable"),
                    solid_render: flag("solid_render"),
                    blocks_motion: flag("blocks_motion"),
                    leaves: flag("leaves"),
                    updates_shape: o
                        .get("shape_updater")
                        .and_then(Value::as_str)
                        .is_some_and(|c| !TICK_ONLY.contains(&c)),
                    fluid,
                },
            );
        }
        Ok(BlockBehaviour { states })
    }

    /// The behaviour of the state named `name`, as a palette names it.
    pub fn of(&self, name: &str) -> Option<Behaviour> {
        self.states.get(name).copied()
    }
}

fn fluid(json: &Value, at: &At<'_>) -> Result<Fluid, LoadError> {
    let o = object(json, at)?;
    let kind = match o.get("id").and_then(Value::as_str) {
        Some("minecraft:water" | "minecraft:flowing_water") => FluidKind::Water,
        Some("minecraft:lava" | "minecraft:flowing_lava") => FluidKind::Lava,
        Some(other) => {
            return Err(at.child("id").error(LoadErrorKind::Unsupported {
                what: "fluid",
                name: other.to_string(),
            }));
        }
        None => return Err(at.child("id").error(LoadErrorKind::Missing("id"))),
    };
    let amount = o
        .get("amount")
        .and_then(Value::as_u64)
        .and_then(|a| u8::try_from(a).ok())
        .ok_or_else(|| {
            at.child("amount")
                .error(LoadErrorKind::Shape("a fluid amount"))
        })?;
    let source = o.get("source").and_then(Value::as_bool).unwrap_or(false);
    Ok(Fluid {
        kind,
        amount,
        source,
    })
}

/// Each state id in [`BLOCKS_REPORT`], by the name a palette gives the state.
fn state_names<R: Resources + ?Sized>(resources: &R) -> Result<HashMap<u64, String>, LoadError> {
    let root = At::root(BLOCKS_REPORT);
    let defaults = BlockDefaults::load(resources)?;
    let json = resources
        .report(BLOCKS_REPORT)
        .map_err(|e| root.error(e.into()))?;
    let mut names = HashMap::new();
    for (block, entry) in object(&json, &root)? {
        let at = root.child(block);
        let states = object(entry, &at)?
            .get("states")
            .and_then(Value::as_array)
            .ok_or_else(|| at.child("states").error(LoadErrorKind::Missing("states")))?;
        for state in states {
            let id = state
                .get("id")
                .and_then(Value::as_u64)
                .ok_or_else(|| at.child("states").error(LoadErrorKind::Shape("a state id")))?;
            let empty = Map::new();
            let properties = state
                .get("properties")
                .and_then(Value::as_object)
                .unwrap_or(&empty);
            names.insert(id, defaults.name(block, properties, &at)?);
        }
    }
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn states_are_named_as_the_palette_names_them() {
        let blocks = json!({
            "minecraft:air": {"states": [{"id": 0, "default": true}]},
            "minecraft:water": {
                "properties": {"level": ["0", "1"]},
                "states": [
                    {"id": 1, "default": true, "properties": {"level": "0"}},
                    {"id": 2, "properties": {"level": "1"}},
                ],
            },
        });
        let report = json!({"format": 1, "states": [
            {"id": 0, "air": true, "replaceable": true},
            {"id": 1, "liquid": true, "fluid": {"id": "minecraft:water", "amount": 8, "source": true}},
            {"id": 2, "liquid": true, "fluid": {"id": "minecraft:flowing_water", "amount": 7}},
        ]});
        let resources = HashMap::from([
            (BLOCKS_REPORT.to_string(), blocks),
            (BLOCK_BEHAVIOUR_REPORT.to_string(), report),
        ]);
        let b = BlockBehaviour::load(&resources).unwrap();
        let air = b.of("minecraft:air").unwrap();
        assert!(air.air && air.replaceable && !air.solid && air.fluid.is_none());
        let water = b.of("minecraft:water").unwrap().fluid.unwrap();
        assert_eq!(
            (water.kind, water.amount, water.source),
            (FluidKind::Water, 8, true)
        );
        let flowing = b.of("minecraft:water[level=1]").unwrap().fluid.unwrap();
        assert_eq!((flowing.amount, flowing.source), (7, false));
        assert_eq!(b.of("minecraft:stone"), None);
    }

    #[test]
    fn a_bundle_without_the_report_says_to_extract_it_again() {
        let error = BlockBehaviour::load(&HashMap::<String, Value>::new()).unwrap_err();
        assert!(error.to_string().contains("extract it again"), "{error}");
    }
}
