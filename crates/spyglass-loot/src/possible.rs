//! The items a table can put in a container, whatever its seed: a superset, for ruling a
//! container out without filling it, and for containers whose seed isn't known.

use std::collections::BTreeSet;

use crate::Loot;
use crate::model::{Entry, EntryKind, Makes, Modifier, ModifierKind};

/// Every item table `id` could fill a container with: each item its entries make, nested tables
/// included, and the items modifiers can turn them into (an enchanted book, 26.2's filled map).
/// Conditions, counts and discards aren't followed, so some may never appear.
pub(crate) fn items(loot: &Loot, id: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    table(loot, id, &mut Vec::new(), &mut out);
    out
}

fn table(loot: &Loot, id: &str, visiting: &mut Vec<String>, out: &mut BTreeSet<String>) {
    // A table that reaches itself again rolls nothing.
    if visiting.iter().any(|v| v == id) {
        return;
    }
    let Some(table) = loot.tables.get(id) else {
        return;
    };
    visiting.push(id.to_string());
    let mut made = BTreeSet::new();
    for pool in &table.pools {
        let mut from_pool = BTreeSet::new();
        for e in &pool.entries {
            entry(loot, e, visiting, &mut from_pool);
        }
        modified(&pool.modifiers, &mut from_pool);
        made.append(&mut from_pool);
    }
    modified(&table.modifiers, &mut made);
    out.append(&mut made);
    visiting.pop();
}

fn entry(loot: &Loot, e: &Entry, visiting: &mut Vec<String>, out: &mut BTreeSet<String>) {
    let mut made = BTreeSet::new();
    match &e.kind {
        EntryKind::Singleton { makes, .. } => match makes {
            Makes::Item(item) => {
                made.insert(item.id.clone());
            }
            Makes::Empty => {}
            Makes::Tables(tables) => {
                for t in tables {
                    table(loot, t, visiting, &mut made);
                }
            }
        },
        EntryKind::Expanded { tables, .. } => {
            for t in tables {
                table(loot, t, visiting, &mut made);
            }
        }
        EntryKind::Alternatives(children)
        | EntryKind::Sequence(children)
        | EntryKind::Group(children) => {
            for child in children {
                entry(loot, child, visiting, &mut made);
            }
        }
    }
    modified(&e.modifiers, &mut made);
    out.append(&mut made);
}

/// Adds to `items` what `modifiers` could turn them into; the items themselves stay.
fn modified(modifiers: &[Modifier], items: &mut BTreeSet<String>) {
    for m in modifiers {
        match &m.kind {
            ModifierKind::SetEnchantments { enchanted_book, .. }
            | ModifierKind::EnchantRandomly { enchanted_book, .. }
            | ModifierKind::EnchantWithLevels { enchanted_book, .. } => {
                if items.contains("minecraft:book") {
                    items.insert(enchanted_book.id.clone());
                }
            }
            ModifierKind::ExplorationMap {
                new_filled_map: Some(filled_map),
                ..
            } => {
                if items.contains("minecraft:map") {
                    items.insert(filled_map.id.clone());
                }
            }
            ModifierKind::Filtered {
                on_pass, on_fail, ..
            } => {
                for list in [on_pass, on_fail].into_iter().flatten() {
                    modified(list, items);
                }
            }
            _ => {}
        }
    }
}
