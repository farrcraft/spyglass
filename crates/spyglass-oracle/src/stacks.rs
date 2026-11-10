//! Stacks from the game's fills and spyglass's in one comparable form
//! (`docs/specs/oracle.md#loot-golden-files`): the item, the count, and each component spyglass
//! sets, defaults left out. The fills of loot and container golden files both compare this way.
//! An exploration map from the oracle finds nothing, so a stack never has a target.

use std::collections::BTreeMap;

use serde_json::Value;
use spyglass_data::bundle::Bundle;
use spyglass_loot::{NameTarget, Stack};

/// A stack as comparable text: the item, the count, and each component spyglass sets, in a
/// fixed order, defaults left out.
#[derive(Debug, PartialEq)]
pub struct Canonical {
    /// The item's id.
    pub item: String,
    /// How many.
    pub count: i64,
    /// Each component set, by a short name, as text.
    pub components: BTreeMap<&'static str, String>,
}

/// The game's encoded stack (`ItemStack.CODEC`), whose `components` are the changes from the
/// item's defaults.
pub fn game(stack: &Value) -> Canonical {
    let mut components = BTreeMap::new();
    if let Some(patch) = stack.get("components").and_then(Value::as_object) {
        for (key, value) in patch {
            let (name, text) = match key.as_str() {
                "minecraft:damage" if value.as_i64() == Some(0) => continue,
                "minecraft:damage" => ("damage", value.to_string()),
                "minecraft:enchantments" => ("enchantments", sorted(value)),
                "minecraft:stored_enchantments" => ("stored_enchantments", sorted(value)),
                "minecraft:potion_contents" => (
                    "potion",
                    match value {
                        Value::String(s) => s.clone(),
                        v => v["potion"].as_str().unwrap_or_default().to_string(),
                    },
                ),
                "minecraft:suspicious_stew_effects" => (
                    "stew_effects",
                    value
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|e| {
                            // A duration of 160 ticks is the default, left out.
                            let duration = e.get("duration").and_then(Value::as_i64).unwrap_or(160);
                            format!("{}={duration}", e["id"].as_str().unwrap())
                        })
                        .collect::<Vec<_>>()
                        .join(","),
                ),
                "minecraft:instrument" => ("instrument", value.as_str().unwrap().to_string()),
                "minecraft:custom_name" => ("custom_name", value.to_string()),
                "minecraft:item_name" => ("item_name", value.to_string()),
                "minecraft:ominous_bottle_amplifier" => {
                    ("ominous_bottle_amplifier", value.to_string())
                }
                other => panic!("a component spyglass doesn't set: {other} = {value}"),
            };
            components.insert(name, text);
        }
    }
    Canonical {
        item: stack["id"].as_str().unwrap().into(),
        count: stack.get("count").and_then(Value::as_i64).unwrap_or(1),
        components,
    }
}

/// spyglass's stack, with the components the game's patch would hold.
pub fn ours(stack: &Stack, bundle: &Bundle) -> Canonical {
    let mut components = BTreeMap::new();
    let levels = |map: &BTreeMap<String, i32>| {
        serde_json::to_string(&map.iter().collect::<BTreeMap<_, _>>()).unwrap()
    };
    if let Some(damage) = stack.damage.filter(|&d| d != 0) {
        components.insert("damage", damage.to_string());
    }
    if !stack.enchantments.is_empty() {
        components.insert("enchantments", levels(&stack.enchantments));
    }
    if !stack.stored_enchantments.is_empty() {
        components.insert("stored_enchantments", levels(&stack.stored_enchantments));
    }
    if let Some(potion) = &stack.potion {
        components.insert("potion", potion.clone());
    }
    if !stack.stew_effects.is_empty() {
        let effects: Vec<String> = stack
            .stew_effects
            .iter()
            .map(|(id, ticks)| format!("{id}={ticks}"))
            .collect();
        components.insert("stew_effects", effects.join(","));
    }
    // The game's patch leaves out a component equal to the item's default, as a goat horn's
    // `ponder_goat_horn` is.
    if let Some(instrument) = &stack.instrument
        && default(bundle, stack.item(), "minecraft:instrument").as_ref()
            != Some(&Value::String(instrument.clone()))
    {
        components.insert("instrument", instrument.clone());
    }
    if let Some((target, name)) = &stack.name {
        let key = match target {
            NameTarget::CustomName => "custom_name",
            NameTarget::ItemName => "item_name",
        };
        components.insert(key, name.to_string());
    }
    if let Some(amplifier) = stack.ominous_bottle_amplifier
        && default(bundle, stack.item(), "minecraft:ominous_bottle_amplifier")
            != Some(Value::from(amplifier))
    {
        components.insert("ominous_bottle_amplifier", amplifier.to_string());
    }
    assert!(stack.map.is_none(), "the oracle's maps find nothing");
    Canonical {
        item: stack.item().into(),
        count: stack.count.into(),
        components,
    }
}

/// Component `name`'s default on `item`, from the bundle's item report.
fn default(bundle: &Bundle, item: &str, name: &str) -> Option<Value> {
    let path = item.strip_prefix("minecraft:").unwrap();
    let report = bundle
        .report(&format!("reports/minecraft/components/item/{path}.json"))
        .ok()?;
    report["components"].get(name).cloned()
}

/// An enchantment map's JSON with its keys sorted: the game's order is its hash map's.
fn sorted(value: &Value) -> String {
    let map: BTreeMap<String, i64> = value
        .as_object()
        .unwrap()
        .iter()
        .map(|(k, v)| (k.clone(), v.as_i64().unwrap()))
        .collect();
    serde_json::to_string(&map).unwrap()
}
