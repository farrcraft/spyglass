//! Loaded loot tables: what `load` builds from either version's JSON and `fill` rolls
//! ([loot.md](../../../docs/specs/loot.md)). One model holds both forms; the loader settles their
//! differences (26.2's float numbers, its condition lists) as it reads them.

use std::collections::HashSet;
use std::sync::Arc;

use serde_json::Value;

/// A loot table: its pools and the modifiers every stack it makes goes through.
pub(crate) struct Table {
    pub pools: Vec<Pool>,
    pub modifiers: Vec<Modifier>,
}

/// A pool: `rolls` rolls, each picking one entry by weight ([loot.md](../../../docs/specs/loot.md#pools)).
pub(crate) struct Pool {
    /// All must hold, in order, or the pool is skipped.
    pub conditions: Vec<Condition>,
    pub modifiers: Vec<Modifier>,
    pub rolls: IntProvider,
    /// Rolls added per point of luck, floored.
    pub bonus_rolls: FloatProvider,
    pub entries: Vec<Entry>,
}

/// An entry, with what every kind shares.
pub(crate) struct Entry {
    /// Tested when the entry is expanded, on every roll.
    pub conditions: Vec<Condition>,
    /// A singleton's modifiers apply to the stacks it makes; a composite's (26.3 on) to its
    /// children's.
    pub modifiers: Vec<Modifier>,
    pub kind: EntryKind,
}

/// What an entry gives when expanded ([loot.md](../../../docs/specs/loot.md#entries)).
pub(crate) enum EntryKind {
    /// One choice with this weight and quality.
    Singleton {
        weight: i32,
        quality: i32,
        makes: Makes,
    },
    /// `loot_table` with `expand` (26.3 on): one choice per table, each with the entry's
    /// weight and quality.
    Expanded {
        weight: i32,
        quality: i32,
        tables: Vec<String>,
    },
    /// Its children in order, until the first that gives a choice.
    Alternatives(Vec<Entry>),
    /// Its children in order, until the first that gives none.
    Sequence(Vec<Entry>),
    /// All its children.
    Group(Vec<Entry>),
}

/// What a singleton's choice makes when it's picked.
pub(crate) enum Makes {
    /// A stack of one.
    Item(Arc<Item>),
    /// Nothing.
    Empty,
    /// Every stack these tables roll, in order, by their keys in [`crate::Loot`].
    Tables(Vec<String>),
}

/// A condition ([loot.md](../../../docs/specs/loot.md#conditions)).
pub(crate) enum Condition {
    RandomChance(FloatProvider),
    /// The biome at the container's block plus `offset`, which `biomes` must hold; no
    /// `biomes` always holds.
    Location {
        offset: [i32; 3],
        biomes: Option<HashSet<String>>,
    },
    Inverted(Box<Condition>),
    AllOf(Vec<Condition>),
    AnyOf(Vec<Condition>),
}

/// A modifier and the conditions it runs under ([loot.md](../../../docs/specs/loot.md#modifiers)).
pub(crate) struct Modifier {
    pub conditions: Vec<Condition>,
    pub kind: ModifierKind,
}

pub(crate) enum ModifierKind {
    SetCount {
        count: IntProvider,
        add: bool,
    },
    SetDamage {
        /// The share of durability left.
        damage: FloatProvider,
        add: bool,
    },
    SetPotion(String),
    /// Effects, each with its duration in seconds and whether it's instant (then in ticks).
    SetStewEffect(Vec<(String, IntProvider, bool)>),
    /// The instruments to pick from, in their holder set's order.
    SetInstrument(Vec<String>),
    /// Without a name it does nothing.
    SetName {
        name: Option<Value>,
        target: crate::NameTarget,
    },
    SetOminousBottleAmplifier(IntProvider),
    /// In the file's key order.
    SetEnchantments {
        enchantments: Vec<(Arc<Enchantment>, IntProvider)>,
        add: bool,
        enchanted_book: Arc<Item>,
    },
    EnchantRandomly {
        options: Vec<Arc<Enchantment>>,
        only_compatible: bool,
        enchanted_book: Arc<Item>,
    },
    EnchantWithLevels {
        levels: IntProvider,
        options: Vec<Arc<Enchantment>>,
        enchanted_book: Arc<Item>,
    },
    ExplorationMap {
        destination: Vec<String>,
        decoration: String,
        zoom: i32,
        search_radius: i32,
        skip_existing_chunks: bool,
        /// 26.2's form: only a `map`, which becomes a new `filled_map`. From 26.3 the stack
        /// itself is marked.
        new_filled_map: Option<Arc<Item>>,
    },
    Filtered {
        filter: ItemFilter,
        on_pass: Option<Vec<Modifier>>,
        on_fail: Option<Vec<Modifier>>,
    },
    Discard,
}

/// The item predicates `filtered` uses: the items it may be, and whether it must have a
/// map id (`predicates: {"minecraft:map_id": {}}`).
pub(crate) struct ItemFilter {
    pub items: Option<HashSet<String>>,
    pub has_map_id: bool,
}

/// An int number provider; 26.2's float ones are rounded into these as they load.
pub(crate) enum IntProvider {
    Constant(i32),
    Uniform(Box<IntProvider>, Box<IntProvider>),
}

/// A float number provider.
pub(crate) enum FloatProvider {
    Constant(f32),
    Uniform(Box<FloatProvider>, Box<FloatProvider>),
}

/// What filling reads of an item: its default components.
pub(crate) struct Item {
    pub id: String,
    pub max_stack_size: i32,
    /// `enchantable`'s value: whether `enchant_with_levels` can enchant it, and how well.
    pub enchantable: Option<i32>,
    /// `max_damage`, 0 without it.
    pub max_damage: i32,
    /// `isDamageableItem`: `max_damage` and `damage`, and not `unbreakable`.
    pub damageable: bool,
    /// Whether it has an `enchantments` component, which enchanting writes into.
    pub has_enchantments: bool,
    /// Whether it has `stored_enchantments` (an enchanted book).
    pub has_stored_enchantments: bool,
}

/// An enchantment's definition, as enchanting reads it.
#[derive(Clone)]
pub(crate) struct Enchantment {
    pub id: String,
    pub weight: i32,
    pub max_level: i32,
    /// `min_cost` and `max_cost`: `base` and `per_level_above_first`.
    pub min_cost: (i32, i32),
    pub max_cost: (i32, i32),
    pub supported_items: HashSet<String>,
    pub primary_items: Option<HashSet<String>>,
    pub exclusive_set: HashSet<String>,
}
