//! A stack of items in a filled container, with the components loot modifiers set.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::Value;

use crate::model::Item;

/// A stack in a container: an item, a count, and the components filling gave it. Components
/// it didn't touch are the item's defaults and aren't listed.
#[derive(Clone)]
pub struct Stack {
    pub(crate) item: Arc<Item>,
    /// How many, at most the item's maximum stack size once filled.
    pub count: i32,
    /// `damage`, if `set_damage` set it.
    pub damage: Option<i32>,
    /// `enchantments`, by id. The game keeps them in hash order, so they're compared as a map.
    pub enchantments: BTreeMap<String, i32>,
    /// An enchanted book's `stored_enchantments`, by id.
    pub stored_enchantments: BTreeMap<String, i32>,
    /// `potion_contents`' potion.
    pub potion: Option<String>,
    /// A suspicious stew's effects, each with its duration in ticks, in the order added.
    pub stew_effects: Vec<(String, i32)>,
    /// A goat horn's `instrument`.
    pub instrument: Option<String>,
    /// A name `set_name` gave, as its text component, and which component holds it.
    pub name: Option<(NameTarget, Value)>,
    /// An ominous bottle's amplifier.
    pub ominous_bottle_amplifier: Option<i32>,
    /// Where an exploration map points.
    pub map: Option<MapMark>,
}

impl Stack {
    /// `count` of `item`, with no components set.
    pub(crate) fn new(item: Arc<Item>, count: i32) -> Self {
        Stack {
            item,
            count,
            damage: None,
            enchantments: BTreeMap::new(),
            stored_enchantments: BTreeMap::new(),
            potion: None,
            stew_effects: Vec::new(),
            instrument: None,
            name: None,
            ominous_bottle_amplifier: None,
            map: None,
        }
    }

    /// The item's id, e.g. `minecraft:golden_apple`.
    pub fn item(&self) -> &str {
        &self.item.id
    }

    /// `ItemStack.isEmpty`: none left.
    pub(crate) fn is_empty(&self) -> bool {
        self.count <= 0
    }

    /// `ItemStack.split(n)`: takes `min(n, count)` off this stack into a new one.
    pub(crate) fn split(&mut self, n: i32) -> Stack {
        let taken = n.min(self.count);
        let mut part = self.clone();
        part.count = taken;
        self.count -= taken;
        part
    }

    /// The same components on another item, keeping the count (`transmuteCopy`).
    pub(crate) fn transmute(&self, item: Arc<Item>) -> Stack {
        Stack {
            item,
            ..self.clone()
        }
    }
}

impl std::fmt::Debug for Stack {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut s = f.debug_struct("Stack");
        s.field("item", &self.item.id).field("count", &self.count);
        if let Some(damage) = self.damage {
            s.field("damage", &damage);
        }
        if !self.enchantments.is_empty() {
            s.field("enchantments", &self.enchantments);
        }
        if !self.stored_enchantments.is_empty() {
            s.field("stored_enchantments", &self.stored_enchantments);
        }
        if let Some(potion) = &self.potion {
            s.field("potion", potion);
        }
        if !self.stew_effects.is_empty() {
            s.field("stew_effects", &self.stew_effects);
        }
        if let Some(instrument) = &self.instrument {
            s.field("instrument", instrument);
        }
        if let Some(name) = &self.name {
            s.field("name", name);
        }
        if let Some(amplifier) = self.ominous_bottle_amplifier {
            s.field("ominous_bottle_amplifier", &amplifier);
        }
        if let Some(map) = &self.map {
            s.field("map", map);
        }
        s.finish()
    }
}

/// Which component `set_name` writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameTarget {
    /// `custom_name`, the default: a name as an anvil gives it.
    CustomName,
    /// `item_name`: the item's own name.
    ItemName,
}

/// An exploration map's target and how it's drawn.
#[derive(Debug, Clone, PartialEq)]
pub struct MapMark {
    /// The structure's block x and z.
    pub target: [i32; 2],
    /// The marker's decoration type, e.g. `minecraft:red_x`.
    pub decoration: String,
    /// The map's scale.
    pub zoom: i32,
}
