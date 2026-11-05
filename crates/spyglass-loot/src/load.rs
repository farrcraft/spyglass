//! Loading loot tables, and the items, enchantments and tags they name, from either version's
//! JSON ([loot.md](../../../docs/specs/loot.md#forms)).
//!
//! The forms are told apart by their shape (ADR 0026): 26.2's tables have `functions` and
//! `conditions` lists, whose objects name their id in `function` and `condition`; from 26.3 a
//! `modifier` and a `condition`, keyed `type`. One table is one form throughout, and its form
//! also decides what 26.2's float-valued numbers mean where an int is wanted (rounded), and how
//! an exploration map behaves.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde_json::{Map, Value};
use spyglass_data::bundle::BundleError;
use spyglass_data::codec::{
    At, LoadError, LoadErrorKind, LoadResult, f32_of, field, float, full_id, id, int, object,
};
use spyglass_density::{Resources, tag};

use crate::NameTarget;
use crate::model::{
    Condition, Enchantment, Entry, EntryKind, FloatProvider, IntProvider, Item, ItemFilter, Makes,
    Modifier, ModifierKind, Pool, Table,
};
use crate::random;

const LOOT_TABLE: &str = "loot_table";
const ENCHANTMENT: &str = "enchantment";

/// The items and enchantments loaded so far, shared by every table.
#[derive(Default)]
pub(crate) struct Cache {
    items: HashMap<String, Arc<Item>>,
    enchantments: HashMap<String, Arc<Enchantment>>,
}

/// Which version's JSON a table is in.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Form {
    /// 26.2's: lists of `functions` and `conditions`, float numbers.
    Lists,
    /// From 26.3: one `modifier` and one `condition`, int and float numbers apart.
    Single,
}

/// Loads tables into a [`crate::Loot`]'s map.
pub(crate) struct Loader<'l, R: ?Sized> {
    resources: &'l R,
    cache: &'l mut Cache,
    tables: &'l mut HashMap<String, Table>,
}

impl<'l, R: Resources + ?Sized> Loader<'l, R> {
    pub(crate) fn new(
        resources: &'l R,
        cache: &'l mut Cache,
        tables: &'l mut HashMap<String, Table>,
    ) -> Self {
        Loader {
            resources,
            cache,
            tables,
        }
    }

    /// Table `id` and every table it reaches. A table reaching itself again is loaded once;
    /// filling rolls nothing for the repeat.
    pub(crate) fn table(&mut self, id: &str) -> LoadResult<()> {
        let mut pending = vec![full_id(id)];
        while let Some(id) = pending.pop() {
            if self.tables.contains_key(&id) {
                continue;
            }
            let json = self
                .resources
                .resource(LOOT_TABLE, &id)
                .map_err(|e| missing(&id, e, "loot table"))?;
            let form = form_of(&json).unwrap_or(Form::Single);
            let mut parse = Parse {
                loader: self,
                form,
                key: id.clone(),
                nested: Vec::new(),
                inline: Vec::new(),
            };
            let table = parse.table(&json, &At::root(&id))?;
            let Parse { nested, inline, .. } = parse;
            self.tables.insert(id, table);
            self.tables.extend(inline);
            pending.extend(nested);
        }
        Ok(())
    }

    /// An item's default components, from the data generator's report.
    fn item(&mut self, id: &str) -> LoadResult<Arc<Item>> {
        let id = full_id(id);
        if let Some(item) = self.cache.items.get(&id) {
            return Ok(item.clone());
        }
        let (namespace, path) = id.split_once(':').expect("full ids have a namespace");
        let report = format!("reports/{namespace}/components/item/{path}.json");
        let json = self
            .resources
            .report(&report)
            .map_err(|e| missing(&report, e, "item report"))?;
        let at = At::root(&report);
        let components = object(field(object(&json, &at)?, "components", &at)?, &at)?;
        let component = |name: &str| components.get(&format!("minecraft:{name}"));
        let at_component = |name: &str| at.child(&format!("components.minecraft:{name}"));
        let max_stack_size = match component("max_stack_size") {
            Some(v) => int(v, &at_component("max_stack_size"))?,
            None => 64,
        };
        let enchantable = match component("enchantable") {
            Some(v) => {
                let at = at_component("enchantable");
                Some(int(
                    field(object(v, &at)?, "value", &at)?,
                    &at.child("value"),
                )?)
            }
            None => None,
        };
        let max_damage = match component("max_damage") {
            Some(v) => Some(int(v, &at_component("max_damage"))?),
            None => None,
        };
        let item = Arc::new(Item {
            damageable: max_damage.is_some()
                && component("damage").is_some()
                && component("unbreakable").is_none(),
            max_damage: max_damage.unwrap_or(0),
            enchantable,
            max_stack_size,
            has_enchantments: component("enchantments").is_some(),
            has_stored_enchantments: component("stored_enchantments").is_some(),
            id: id.clone(),
        });
        self.cache.items.insert(id, item.clone());
        Ok(item)
    }

    /// An enchantment's definition.
    fn enchantment(&mut self, id: &str) -> LoadResult<Arc<Enchantment>> {
        let id = full_id(id);
        if let Some(enchantment) = self.cache.enchantments.get(&id) {
            return Ok(enchantment.clone());
        }
        let json = self
            .resources
            .resource(ENCHANTMENT, &id)
            .map_err(|e| missing(&id, e, "enchantment"))?;
        let at = At::root(&id);
        let obj = object(&json, &at)?;
        let cost = |name: &'static str| -> LoadResult<(i32, i32)> {
            let at = at.child(name);
            let cost = object(field(obj, name, &at)?, &at)?;
            Ok((
                int(field(cost, "base", &at)?, &at.child("base"))?,
                int(
                    field(cost, "per_level_above_first", &at)?,
                    &at.child("per_level_above_first"),
                )?,
            ))
        };
        let enchantment = Arc::new(Enchantment {
            weight: int(field(obj, "weight", &at)?, &at.child("weight"))?,
            max_level: int(field(obj, "max_level", &at)?, &at.child("max_level"))?,
            min_cost: cost("min_cost")?,
            max_cost: cost("max_cost")?,
            supported_items: self
                .holder_set(
                    field(obj, "supported_items", &at)?,
                    "tags/item",
                    &at.child("supported_items"),
                )?
                .into_iter()
                .collect(),
            primary_items: match obj.get("primary_items") {
                Some(v) => Some(
                    self.holder_set(v, "tags/item", &at.child("primary_items"))?
                        .into_iter()
                        .collect(),
                ),
                None => None,
            },
            exclusive_set: match obj.get("exclusive_set") {
                Some(v) => self
                    .holder_set(v, "tags/enchantment", &at.child("exclusive_set"))?
                    .into_iter()
                    .collect(),
                None => HashSet::new(),
            },
            id: id.clone(),
        });
        self.cache.enchantments.insert(id, enchantment.clone());
        Ok(enchantment)
    }

    /// A holder set's ids, in its order ([loot.md](../../../docs/specs/loot.md#data)): a `#tag`
    /// of `tags` (a tag registry path), an id, or a list of ids.
    fn holder_set(&self, json: &Value, tags: &str, at: &At<'_>) -> LoadResult<Vec<String>> {
        match json {
            Value::String(s) => match s.strip_prefix('#') {
                Some(t) => tag(self.resources, tags, t),
                None => Ok(vec![full_id(s)]),
            },
            Value::Array(list) => list
                .iter()
                .enumerate()
                .map(|(i, v)| id(v, &at.child(&format!("[{i}]"))).map(full_id))
                .collect(),
            _ => Err(at.error(LoadErrorKind::Shape("a #tag, an id or a list of ids"))),
        }
    }
}

/// A missing resource as a load error that says what to do about it.
fn missing(resource: &str, error: BundleError, what: &'static str) -> LoadError {
    let kind = match error {
        BundleError::NotFound(_) => LoadErrorKind::Invalid(match what {
            "item report" => {
                "the bundle has no item components; extract it again with this version of spyglass"
            }
            _ => {
                "the bundle has no such resource, or none of its kind; extract it again with this version of spyglass"
            }
        }),
        e => e.into(),
    };
    LoadError::new(format!("{what} {resource}"), "", kind)
}

/// The form a table's JSON shows, from the first key that tells: `functions` or `conditions`,
/// or a `function` or `condition` naming an id, are 26.2's; `modifier`, or a `condition`
/// holding an object, are 26.3's. `None` if nothing tells, as for a table of plain pools, whose
/// numbers then read the same either way.
fn form_of(json: &Value) -> Option<Form> {
    match json {
        Value::Object(obj) => {
            for (key, value) in obj {
                let form = match (key.as_str(), value) {
                    ("functions" | "conditions" | "function", _) => Some(Form::Lists),
                    ("condition", Value::String(_)) => Some(Form::Lists),
                    ("modifier", _) | ("condition", Value::Object(_)) => Some(Form::Single),
                    _ => None,
                };
                if let Some(form) = form.or_else(|| form_of(value)) {
                    return Some(form);
                }
            }
            None
        }
        Value::Array(list) => list.iter().find_map(form_of),
        _ => None,
    }
}

/// One table being parsed.
struct Parse<'p, 'l, R: ?Sized> {
    loader: &'p mut Loader<'l, R>,
    form: Form,
    /// The table's key, for naming its inline tables.
    key: String,
    /// The tables it names, to load after it.
    nested: Vec<String>,
    /// Inline tables (26.2's `loot_table` entries with an object), under keys of their own.
    inline: Vec<(String, Table)>,
}

impl<R: Resources + ?Sized> Parse<'_, '_, R> {
    fn table(&mut self, json: &Value, at: &At<'_>) -> LoadResult<Table> {
        let obj = object(json, at)?;
        let pools = match obj.get("pools") {
            Some(v) => list(v, at, "pools")?
                .map(|(v, at)| self.pool(v, &at))
                .collect::<LoadResult<_>>()?,
            None => Vec::new(),
        };
        Ok(Table {
            pools,
            modifiers: self.modifiers_of(obj, at)?,
        })
    }

    fn pool(&mut self, json: &Value, at: &At<'_>) -> LoadResult<Pool> {
        let obj = object(json, at)?;
        let entries = list(field(obj, "entries", at)?, at, "entries")?
            .map(|(v, at)| self.entry(v, &at))
            .collect::<LoadResult<_>>()?;
        Ok(Pool {
            conditions: self.conditions_of(obj, at)?,
            modifiers: self.modifiers_of(obj, at)?,
            rolls: self.int(field(obj, "rolls", at)?, &at.child("rolls"))?,
            bonus_rolls: match obj.get("bonus_rolls") {
                Some(v) => self.float(v, &at.child("bonus_rolls"))?,
                None => FloatProvider::Constant(0.0),
            },
            entries,
        })
    }

    fn entry(&mut self, json: &Value, at: &At<'_>) -> LoadResult<Entry> {
        let obj = object(json, at)?;
        let kind = full_id(id(field(obj, "type", at)?, &at.child("type"))?);
        let weight = |field: &str| match obj.get(field) {
            Some(v) => int(v, &at.child(field)),
            None => Ok(if field == "weight" { 1 } else { 0 }),
        };
        let children = |this: &mut Self| -> LoadResult<Vec<Entry>> {
            match obj.get("children") {
                Some(v) => list(v, at, "children")?
                    .map(|(v, at)| this.entry(v, &at))
                    .collect(),
                None => Ok(Vec::new()),
            }
        };
        let kind = match kind.as_str() {
            "minecraft:item" => {
                let item = self
                    .loader
                    .item(id(field(obj, "name", at)?, &at.child("name"))?)?;
                EntryKind::Singleton {
                    weight: weight("weight")?,
                    quality: weight("quality")?,
                    makes: Makes::Item(item),
                }
            }
            "minecraft:empty" => EntryKind::Singleton {
                weight: weight("weight")?,
                quality: weight("quality")?,
                makes: Makes::Empty,
            },
            "minecraft:loot_table" => {
                let value = field(obj, "value", at)?;
                let tables = self.tables(value, &at.child("value"))?;
                let expand = match obj.get("expand") {
                    Some(v) => v.as_bool().ok_or_else(|| {
                        at.child("expand").error(LoadErrorKind::Shape("a boolean"))
                    })?,
                    None => false,
                };
                if expand {
                    EntryKind::Expanded {
                        weight: weight("weight")?,
                        quality: weight("quality")?,
                        tables,
                    }
                } else {
                    EntryKind::Singleton {
                        weight: weight("weight")?,
                        quality: weight("quality")?,
                        makes: Makes::Tables(tables),
                    }
                }
            }
            "minecraft:alternatives" => EntryKind::Alternatives(children(self)?),
            "minecraft:sequence" => EntryKind::Sequence(children(self)?),
            "minecraft:group" => EntryKind::Group(children(self)?),
            _ => {
                return Err(at.child("type").error(LoadErrorKind::Unsupported {
                    what: "loot entry type",
                    name: kind,
                }));
            }
        };
        Ok(Entry {
            conditions: self.conditions_of(obj, at)?,
            modifiers: self.modifiers_of(obj, at)?,
            kind,
        })
    }

    /// A `loot_table` entry's `value`: an id, a list of ids (26.3), or an inline table, each
    /// under its key.
    fn tables(&mut self, json: &Value, at: &At<'_>) -> LoadResult<Vec<String>> {
        match json {
            Value::String(s) if s.starts_with('#') => Err(at.error(LoadErrorKind::Unsupported {
                what: "loot table tag",
                name: s.clone(),
            })),
            Value::String(s) => {
                let id = full_id(s);
                self.nested.push(id.clone());
                Ok(vec![id])
            }
            Value::Array(list) => list
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    self.tables(v, &at.child(&format!("[{i}]")))
                        .map(|t| t[0].clone())
                })
                .collect(),
            Value::Object(_) => {
                let key = format!("{}#inline{}", self.key, self.inline.len());
                let table = self.table(json, at)?;
                self.inline.push((key.clone(), table));
                Ok(vec![key])
            }
            _ => Err(at.error(LoadErrorKind::Shape("a loot table id, a list or a table"))),
        }
    }

    /// A pool's, entry's or modifier's conditions: 26.2's `conditions` list, or 26.3's one
    /// `condition`.
    fn conditions_of(
        &mut self,
        obj: &Map<String, Value>,
        at: &At<'_>,
    ) -> LoadResult<Vec<Condition>> {
        match self.form {
            Form::Lists => match obj.get("conditions") {
                Some(v) => list(v, at, "conditions")?
                    .map(|(v, at)| self.condition(v, &at))
                    .collect(),
                None => Ok(Vec::new()),
            },
            Form::Single => match obj.get("condition") {
                Some(v) => Ok(vec![self.condition(v, &at.child("condition"))?]),
                None => Ok(Vec::new()),
            },
        }
    }

    /// A table's, pool's or entry's modifiers, in order: 26.2's `functions` list, or 26.3's
    /// `modifier`.
    fn modifiers_of(&mut self, obj: &Map<String, Value>, at: &At<'_>) -> LoadResult<Vec<Modifier>> {
        let name = match self.form {
            Form::Lists => "functions",
            Form::Single => "modifier",
        };
        match obj.get(name) {
            Some(v) => self.modifiers(v, &at.child(name)),
            None => Ok(Vec::new()),
        }
    }

    /// One modifier or a list run in order (`sequence`), flattened.
    fn modifiers(&mut self, json: &Value, at: &At<'_>) -> LoadResult<Vec<Modifier>> {
        match json {
            Value::Array(list) => {
                let mut all = Vec::new();
                for (i, v) in list.iter().enumerate() {
                    all.extend(self.modifiers(v, &at.child(&format!("[{i}]")))?);
                }
                Ok(all)
            }
            Value::Object(obj) => {
                let key = match self.form {
                    Form::Lists => "function",
                    Form::Single => "type",
                };
                let kind = full_id(id(field(obj, key, at)?, &at.child(key))?);
                if kind == "minecraft:sequence" {
                    return match obj.get("functions") {
                        Some(v) => self.modifiers(v, &at.child("functions")),
                        None => Ok(Vec::new()),
                    };
                }
                Ok(vec![Modifier {
                    conditions: self.conditions_of(obj, at)?,
                    kind: self.modifier(&kind, obj, at)?,
                }])
            }
            Value::String(s) => Err(at.error(LoadErrorKind::Unsupported {
                what: "item modifier reference",
                name: s.clone(),
            })),
            _ => Err(at.error(LoadErrorKind::Shape("a modifier or a list of them"))),
        }
    }

    fn modifier(
        &mut self,
        kind: &str,
        obj: &Map<String, Value>,
        at: &At<'_>,
    ) -> LoadResult<ModifierKind> {
        let flag = |name: &str, default: bool| match obj.get(name) {
            Some(v) => v
                .as_bool()
                .ok_or_else(|| at.child(name).error(LoadErrorKind::Shape("a boolean"))),
            None => Ok(default),
        };
        let int_or = |name: &str, default: i32| match obj.get(name) {
            Some(v) => int(v, &at.child(name)),
            None => Ok(default),
        };
        Ok(match kind {
            "minecraft:set_count" => ModifierKind::SetCount {
                count: self.int(field(obj, "count", at)?, &at.child("count"))?,
                add: flag("add", false)?,
            },
            "minecraft:set_damage" => ModifierKind::SetDamage {
                damage: self.float(field(obj, "damage", at)?, &at.child("damage"))?,
                add: flag("add", false)?,
            },
            "minecraft:set_potion" => {
                ModifierKind::SetPotion(full_id(id(field(obj, "id", at)?, &at.child("id"))?))
            }
            "minecraft:set_stew_effect" => {
                let mut effects = Vec::new();
                if let Some(v) = obj.get("effects") {
                    for (v, at) in list(v, at, "effects")? {
                        let effect = object(v, &at)?;
                        let id = full_id(id(field(effect, "type", &at)?, &at.child("type"))?);
                        let duration =
                            self.int(field(effect, "duration", &at)?, &at.child("duration"))?;
                        let instant = INSTANT_EFFECTS.contains(&id.as_str());
                        effects.push((id, duration, instant));
                    }
                }
                ModifierKind::SetStewEffect(effects)
            }
            "minecraft:set_instrument" => ModifierKind::SetInstrument(self.loader.holder_set(
                field(obj, "options", at)?,
                "tags/instrument",
                &at.child("options"),
            )?),
            "minecraft:set_name" => {
                if obj.contains_key("entity") {
                    return Err(at.child("entity").error(LoadErrorKind::Unsupported {
                        what: "set_name with",
                        name: "entity".into(),
                    }));
                }
                let target = match obj.get("target").and_then(Value::as_str) {
                    None | Some("custom_name") => NameTarget::CustomName,
                    Some("item_name") => NameTarget::ItemName,
                    Some(other) => {
                        return Err(at.child("target").error(LoadErrorKind::Unsupported {
                            what: "set_name target",
                            name: other.into(),
                        }));
                    }
                };
                ModifierKind::SetName {
                    name: obj.get("name").cloned(),
                    target,
                }
            }
            "minecraft:set_ominous_bottle_amplifier" => ModifierKind::SetOminousBottleAmplifier(
                self.int(field(obj, "amplifier", at)?, &at.child("amplifier"))?,
            ),
            "minecraft:set_enchantments" => {
                let mut enchantments = Vec::new();
                if let Some(v) = obj.get("enchantments") {
                    let at = at.child("enchantments");
                    let map = object(v, &at)?;
                    for (id, level) in map {
                        let level = self.int(level, &at.child(id))?;
                        enchantments.push((self.loader.enchantment(id)?, level));
                    }
                    // The game applies them in the file's order; the JSON here is read sorted,
                    // which only matters when more than one level is drawn.
                    let drawn = enchantments
                        .iter()
                        .filter(|(_, l)| matches!(l, IntProvider::Uniform(..)))
                        .count();
                    if enchantments.len() > 1 && drawn > 0 {
                        return Err(at.error(LoadErrorKind::Unsupported {
                            what: "set_enchantments with random levels and more than",
                            name: "one enchantment".into(),
                        }));
                    }
                }
                ModifierKind::SetEnchantments {
                    enchantments,
                    add: flag("add", false)?,
                    enchanted_book: self.loader.item("minecraft:enchanted_book")?,
                }
            }
            "minecraft:enchant_randomly" => ModifierKind::EnchantRandomly {
                options: self.options(obj, at)?,
                only_compatible: flag("only_compatible", true)?,
                enchanted_book: self.loader.item("minecraft:enchanted_book")?,
            },
            "minecraft:enchant_with_levels" => ModifierKind::EnchantWithLevels {
                levels: self.int(field(obj, "levels", at)?, &at.child("levels"))?,
                options: self.options(obj, at)?,
                enchanted_book: self.loader.item("minecraft:enchanted_book")?,
            },
            "minecraft:exploration_map" => {
                let destination = match (self.form, obj.get("destination")) {
                    // 26.2: a structure tag written without its `#`.
                    (Form::Lists, None) => vec!["#minecraft:on_treasure_maps".to_string()],
                    (Form::Lists, Some(v)) => {
                        vec![format!("#{}", full_id(id(v, &at.child("destination"))?))]
                    }
                    (Form::Single, v) => {
                        let v = v.ok_or_else(|| at.error(LoadErrorKind::Missing("destination")))?;
                        match v {
                            Value::String(s) if s.starts_with('#') => {
                                vec![format!("#{}", full_id(&s[1..]))]
                            }
                            _ => self.loader.holder_set(
                                v,
                                "tags/worldgen/structure",
                                &at.child("destination"),
                            )?,
                        }
                    }
                };
                ModifierKind::ExplorationMap {
                    destination,
                    decoration: match obj.get("decoration") {
                        Some(v) => full_id(id(v, &at.child("decoration"))?),
                        None => "minecraft:mansion".into(),
                    },
                    zoom: int_or("zoom", 2)?,
                    search_radius: int_or("search_radius", 50)?,
                    skip_existing_chunks: flag("skip_existing_chunks", true)?,
                    new_filled_map: match self.form {
                        Form::Lists => Some(self.loader.item("minecraft:filled_map")?),
                        Form::Single => None,
                    },
                }
            }
            "minecraft:filtered" => {
                let at_filter = at.child("item_filter");
                let filter = object(field(obj, "item_filter", at)?, &at_filter)?;
                let mut items = None;
                let mut has_map_id = false;
                for (key, value) in filter {
                    match key.as_str() {
                        "items" => {
                            items = Some(
                                self.loader
                                    .holder_set(value, "tags/item", &at_filter.child("items"))?
                                    .into_iter()
                                    .collect(),
                            );
                        }
                        "predicates" => {
                            let at = at_filter.child("predicates");
                            for (predicate, args) in object(value, &at)? {
                                let empty = args.as_object().is_some_and(Map::is_empty);
                                if full_id(predicate) != "minecraft:map_id" || !empty {
                                    return Err(at.child(predicate).error(
                                        LoadErrorKind::Unsupported {
                                            what: "item predicate",
                                            name: predicate.clone(),
                                        },
                                    ));
                                }
                                has_map_id = true;
                            }
                        }
                        other => {
                            return Err(at_filter.child(other).error(LoadErrorKind::Unsupported {
                                what: "item filter field",
                                name: other.into(),
                            }));
                        }
                    }
                }
                let mut branch = |name: &str| match obj.get(name) {
                    Some(v) => self.modifiers(v, &at.child(name)).map(Some),
                    None => Ok(None),
                };
                ModifierKind::Filtered {
                    filter: ItemFilter { items, has_map_id },
                    on_pass: branch("on_pass")?,
                    on_fail: branch("on_fail")?,
                }
            }
            "minecraft:discard" => ModifierKind::Discard,
            _ => {
                return Err(at.error(LoadErrorKind::Unsupported {
                    what: "loot modifier",
                    name: kind.into(),
                }));
            }
        })
    }

    /// An enchanting modifier's `options`, in order. Without them the game uses every
    /// enchantment in the registry's order, which a bundle can't list; vanilla always gives them.
    fn options(
        &mut self,
        obj: &Map<String, Value>,
        at: &At<'_>,
    ) -> LoadResult<Vec<Arc<Enchantment>>> {
        let v = obj.get("options").ok_or_else(|| {
            at.error(LoadErrorKind::Unsupported {
                what: "enchanting without",
                name: "options".into(),
            })
        })?;
        let ids = self
            .loader
            .holder_set(v, "tags/enchantment", &at.child("options"))?;
        ids.iter().map(|id| self.loader.enchantment(id)).collect()
    }

    fn condition(&mut self, json: &Value, at: &At<'_>) -> LoadResult<Condition> {
        let obj = match json {
            Value::Object(obj) => obj,
            Value::String(s) => {
                return Err(at.error(LoadErrorKind::Unsupported {
                    what: "predicate reference",
                    name: s.clone(),
                }));
            }
            _ => return Err(at.error(LoadErrorKind::Shape("a condition"))),
        };
        let key = match self.form {
            Form::Lists => "condition",
            Form::Single => "type",
        };
        let kind = full_id(id(field(obj, key, at)?, &at.child(key))?);
        let terms = |this: &mut Self| -> LoadResult<Vec<Condition>> {
            list(field(obj, "terms", at)?, at, "terms")?
                .map(|(v, at)| this.condition(v, &at))
                .collect()
        };
        Ok(match kind.as_str() {
            "minecraft:random_chance" => {
                Condition::RandomChance(self.float(field(obj, "chance", at)?, &at.child("chance"))?)
            }
            "minecraft:location_check" => {
                let mut offset = [0; 3];
                for (i, name) in ["offsetX", "offsetY", "offsetZ"].into_iter().enumerate() {
                    if let Some(v) = obj.get(name) {
                        offset[i] = int(v, &at.child(name))?;
                    }
                }
                let mut biomes = None;
                if let Some(predicate) = obj.get("predicate") {
                    let at = at.child("predicate");
                    for (key, value) in object(predicate, &at)? {
                        if key != "biomes" {
                            return Err(at.child(key).error(LoadErrorKind::Unsupported {
                                what: "location predicate field",
                                name: key.clone(),
                            }));
                        }
                        biomes = Some(
                            self.loader
                                .holder_set(value, "tags/worldgen/biome", &at.child(key))?
                                .into_iter()
                                .collect(),
                        );
                    }
                }
                Condition::Location { offset, biomes }
            }
            "minecraft:inverted" => Condition::Inverted(Box::new(
                self.condition(field(obj, "term", at)?, &at.child("term"))?,
            )),
            "minecraft:all_of" => Condition::AllOf(terms(self)?),
            "minecraft:any_of" => Condition::AnyOf(terms(self)?),
            _ => {
                return Err(at.child(key).error(LoadErrorKind::Unsupported {
                    what: "loot condition",
                    name: kind,
                }));
            }
        })
    }

    /// A number provider read as an int: 26.2's floats rounded (`Math.round`), 26.3's ints.
    fn int(&self, json: &Value, at: &At<'_>) -> LoadResult<IntProvider> {
        if json.is_number() {
            return Ok(IntProvider::Constant(match self.form {
                Form::Lists => random::round(float(json, at)?),
                Form::Single => int(json, at)?,
            }));
        }
        match self.number_type(json, at)? {
            NumberType::Constant(value) => self.int(value, &at.child("value")),
            NumberType::Uniform(min, max) => Ok(IntProvider::Uniform(
                Box::new(self.int(min, &at.child("min"))?),
                Box::new(self.int(max, &at.child("max"))?),
            )),
        }
    }

    /// A number provider read as a float.
    fn float(&self, json: &Value, at: &At<'_>) -> LoadResult<FloatProvider> {
        if json.is_number() {
            return Ok(FloatProvider::Constant(
                f32_of(json).ok_or_else(|| at.error(LoadErrorKind::Shape("a number")))?,
            ));
        }
        match self.number_type(json, at)? {
            NumberType::Constant(value) => self.float(value, &at.child("value")),
            NumberType::Uniform(min, max) => Ok(FloatProvider::Uniform(
                Box::new(self.float(min, &at.child("min"))?),
                Box::new(self.float(max, &at.child("max"))?),
            )),
        }
    }

    /// A number provider object's kind. 26.2 reads one without `type` as a uniform.
    fn number_type<'v>(&self, json: &'v Value, at: &At<'_>) -> LoadResult<NumberType<'v>> {
        let obj = match json {
            Value::Object(obj) => obj,
            Value::String(s) => {
                return Err(at.error(LoadErrorKind::Unsupported {
                    what: "number provider reference",
                    name: s.clone(),
                }));
            }
            _ => return Err(at.error(LoadErrorKind::Shape("a number or a number provider"))),
        };
        let kind = match (obj.get("type"), self.form) {
            (Some(t), _) => full_id(id(t, &at.child("type"))?),
            (None, Form::Lists) => "minecraft:uniform".into(),
            (None, Form::Single) => return Err(at.error(LoadErrorKind::Missing("type"))),
        };
        match kind.as_str() {
            "minecraft:constant" => Ok(NumberType::Constant(field(obj, "value", at)?)),
            "minecraft:uniform" => Ok(NumberType::Uniform(
                field(obj, "min", at)?,
                field(obj, "max", at)?,
            )),
            _ => Err(at.child("type").error(LoadErrorKind::Unsupported {
                what: "number provider",
                name: kind,
            })),
        }
    }
}

/// A number provider object, before it's read as an int or a float.
enum NumberType<'v> {
    Constant(&'v Value),
    Uniform(&'v Value, &'v Value),
}

/// The effects whose suspicious-stew duration is in ticks, not seconds: `InstantaneousMobEffect`s.
/// Effects are code, not data, in every supported version.
const INSTANT_EFFECTS: &[&str] = &[
    "minecraft:instant_health",
    "minecraft:instant_damage",
    "minecraft:saturation",
];

/// The elements of list field `name`, each with its position.
fn list<'v, 'a>(
    json: &'v Value,
    at: &At<'a>,
    name: &str,
) -> LoadResult<impl Iterator<Item = (&'v Value, At<'a>)>> {
    let items = json
        .as_array()
        .ok_or_else(|| at.child(name).error(LoadErrorKind::Shape("a list")))?;
    let at = at.clone();
    let name = name.to_string();
    Ok(items
        .iter()
        .enumerate()
        .map(move |(i, v)| (v, at.child(&format!("{name}[{i}]")))))
}
