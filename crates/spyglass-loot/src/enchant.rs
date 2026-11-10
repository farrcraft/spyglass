//! Enchanting: `enchant_randomly`, `enchant_with_levels` (`EnchantmentHelper.selectEnchantment`)
//! and `set_enchantments` ([loot.md](../../../docs/specs/loot.md#enchanting)).

use std::sync::Arc;

use spyglass_rng::{LegacyRandomSource, RandomSource};

use crate::model::{Enchantment, IntProvider, Item};
use crate::random::{self, next_int};
use crate::stack::Stack;

const BOOK: &str = "minecraft:book";
const ENCHANTED_BOOK: &str = "minecraft:enchanted_book";

/// `enchant_randomly`: one of `options` the item supports (any, for a book), at a random level.
pub(crate) fn randomly(
    stack: Stack,
    options: &[Arc<Enchantment>],
    only_compatible: bool,
    enchanted_book: &Arc<Item>,
    random: &mut LegacyRandomSource,
) -> Stack {
    let is_book = stack.item() == BOOK;
    let filter = !is_book && only_compatible;
    let candidates: Vec<&Arc<Enchantment>> = options
        .iter()
        .filter(|e| !filter || e.supported_items.contains(stack.item()))
        .collect();
    if candidates.is_empty() {
        return stack;
    }
    let enchantment = candidates[random.next_int_bounded(candidates.len() as i32) as usize];
    let level = next_int(random, 1, enchantment.max_level);
    let mut stack = if is_book {
        Stack::new(enchanted_book.clone(), 1)
    } else {
        stack
    };
    upgrade(&mut stack, enchantment, level);
    stack
}

/// `enchant_with_levels`: `EnchantmentHelper.enchantItem` with `levels` already drawn.
pub(crate) fn with_levels(
    stack: Stack,
    levels: i32,
    options: &[Arc<Enchantment>],
    enchanted_book: &Arc<Item>,
    random: &mut LegacyRandomSource,
) -> Stack {
    let chosen = select(&stack, levels, options, random);
    let mut stack = if stack.item() == BOOK {
        Stack::new(enchanted_book.clone(), 1)
    } else {
        stack
    };
    for (enchantment, level) in chosen {
        upgrade(&mut stack, enchantment, level);
    }
    stack
}

/// `selectEnchantment`: the cost from `level` and the item's enchantability, then a weighted
/// pick among the candidates that fit it, and more picks while `nextInt(50) <= cost`, the cost
/// halving after each.
fn select<'e>(
    stack: &Stack,
    level: i32,
    options: &'e [Arc<Enchantment>],
    random: &mut LegacyRandomSource,
) -> Vec<(&'e Arc<Enchantment>, i32)> {
    let mut chosen = Vec::new();
    let Some(enchantability) = stack.item.enchantable else {
        return chosen;
    };
    let spread = enchantability / 4 + 1;
    let first = random.next_int_bounded(spread);
    let second = random.next_int_bounded(spread);
    let mut cost = level.wrapping_add(1i32.wrapping_add(first).wrapping_add(second));
    let bonus = (random.next_float() + random.next_float() - 1.0) * 0.15;
    let cost_f = cost as f32;
    cost = random::round(cost_f + cost_f * bonus).max(1);

    let is_book = stack.item() == BOOK;
    let mut available: Vec<(&Arc<Enchantment>, i32)> = options
        .iter()
        .filter(|e| is_book || is_primary_item(e, stack.item()))
        .filter_map(|e| {
            (1..=e.max_level)
                .rev()
                .find(|&l| cost >= cost_at(e.min_cost, l) && cost <= cost_at(e.max_cost, l))
                .map(|l| (e, l))
        })
        .collect();
    if available.is_empty() {
        return chosen;
    }
    if let Some(pick) = weighted(&available, random) {
        chosen.push(pick);
    }
    while random.next_int_bounded(50) <= cost {
        if let Some(&(last, _)) = chosen.last() {
            available.retain(|(e, _)| compatible(last, e));
        }
        if available.is_empty() {
            break;
        }
        if let Some(pick) = weighted(&available, random) {
            chosen.push(pick);
        }
        cost /= 2;
    }
    chosen
}

/// `set_enchantments`: each level clamped to `0..=255` (added to the stack's with `add`), 0
/// removing it, in order. A book becomes an enchanted book, keeping its count.
pub(crate) fn set(
    stack: Stack,
    enchantments: &[(Arc<Enchantment>, IntProvider)],
    add: bool,
    enchanted_book: &Arc<Item>,
    mut level_of: impl FnMut(&IntProvider) -> i32,
) -> Stack {
    let mut stack = if stack.item() == BOOK {
        stack.transmute(enchanted_book.clone())
    } else {
        stack
    };
    // No component to write into: nothing, and the providers aren't evaluated.
    let Some(map) = enchantments_of(&mut stack) else {
        return stack;
    };
    for (enchantment, provider) in enchantments {
        let old = map.get(&enchantment.id).copied().unwrap_or(0);
        let given = level_of(provider);
        let level = if add { old.wrapping_add(given) } else { given }.clamp(0, 255);
        if level <= 0 {
            map.remove(&enchantment.id);
        } else {
            map.insert(enchantment.id.clone(), level);
        }
    }
    stack
}

/// `ItemStack.enchant`: the higher of the stack's level and `level`, at most 255.
fn upgrade(stack: &mut Stack, enchantment: &Enchantment, level: i32) {
    if level <= 0 {
        return;
    }
    if let Some(map) = enchantments_of(stack) {
        let level = level.min(255);
        let slot = map.entry(enchantment.id.clone()).or_insert(level);
        *slot = (*slot).max(level);
    }
}

/// The map enchanting writes: an enchanted book's stored enchantments, else the item's own,
/// or `None` if the item has no such component (`updateEnchantments` then does nothing).
fn enchantments_of(stack: &mut Stack) -> Option<&mut std::collections::BTreeMap<String, i32>> {
    if stack.item() == ENCHANTED_BOOK {
        stack
            .item
            .has_stored_enchantments
            .then_some(&mut stack.stored_enchantments)
    } else {
        stack
            .item
            .has_enchantments
            .then_some(&mut stack.enchantments)
    }
}

/// `WeightedRandom.getRandomItem`: `nextInt(total weight)`, then the first whose running
/// subtraction goes below 0.
fn weighted<'e>(
    list: &[(&'e Arc<Enchantment>, i32)],
    random: &mut LegacyRandomSource,
) -> Option<(&'e Arc<Enchantment>, i32)> {
    let total: i32 = list.iter().map(|(e, _)| e.weight).sum();
    if total <= 0 {
        return None;
    }
    let mut left = random.next_int_bounded(total);
    for &(e, level) in list {
        left -= e.weight;
        if left < 0 {
            return Some((e, level));
        }
    }
    None
}

/// `Enchantment.Cost.calculate`: `base + per_level_above_first * (level - 1)`, in `int`.
fn cost_at((base, per_level): (i32, i32), level: i32) -> i32 {
    base.wrapping_add(per_level.wrapping_mul(level - 1))
}

/// `isPrimaryItem`: supported, and primary if the enchantment names primary items.
fn is_primary_item(enchantment: &Enchantment, item: &str) -> bool {
    enchantment.supported_items.contains(item)
        && enchantment
            .primary_items
            .as_ref()
            .is_none_or(|primary| primary.contains(item))
}

/// `Enchantment.areCompatible`: different, and neither in the other's exclusive set.
fn compatible(a: &Enchantment, b: &Enchantment) -> bool {
    a.id != b.id && !a.exclusive_set.contains(&b.id) && !b.exclusive_set.contains(&a.id)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    fn item(id: &str, enchantable: Option<i32>) -> Arc<Item> {
        Arc::new(Item {
            id: id.into(),
            max_stack_size: 1,
            enchantable,
            max_damage: 0,
            damageable: false,
            has_enchantments: true,
            has_stored_enchantments: id == ENCHANTED_BOOK,
        })
    }

    fn enchantment(id: &str, weight: i32, max_level: i32, items: &[&str]) -> Arc<Enchantment> {
        Arc::new(Enchantment {
            id: id.into(),
            weight,
            max_level,
            min_cost: (1, 10),
            max_cost: (21, 10),
            supported_items: items.iter().map(|s| s.to_string()).collect(),
            primary_items: None,
            exclusive_set: HashSet::new(),
        })
    }

    #[test]
    fn randomly_draws_the_pick_then_the_level() {
        let sword = item("minecraft:diamond_sword", Some(10));
        let book = item(ENCHANTED_BOOK, None);
        let options = [
            enchantment("minecraft:sharpness", 10, 5, &["minecraft:diamond_sword"]),
            enchantment("minecraft:mending", 2, 1, &["minecraft:diamond_sword"]),
            enchantment("minecraft:power", 10, 5, &["minecraft:bow"]),
        ];
        let mut random = LegacyRandomSource::new(5);
        let mut copy = LegacyRandomSource::new(5);
        let got = randomly(Stack::new(sword, 1), &options, true, &book, &mut random);
        // Two compatible candidates: nextInt(2), then nextInt(max_level) + 1 for sharpness.
        let pick = copy.next_int_bounded(2);
        let (id, level) = if pick == 0 {
            ("minecraft:sharpness", copy.next_int_bounded(5) + 1)
        } else {
            ("minecraft:mending", 1)
        };
        assert_eq!(got.enchantments.get(id), Some(&level));
        assert_eq!(random.next_long(), copy.next_long());
    }

    #[test]
    fn a_book_takes_any_option_and_becomes_one_enchanted_book() {
        let mut book_stack = Stack::new(item(BOOK, Some(1)), 3);
        book_stack.count = 3;
        let enchanted = item(ENCHANTED_BOOK, None);
        let options = [enchantment("minecraft:power", 10, 1, &["minecraft:bow"])];
        let got = randomly(
            book_stack,
            &options,
            true,
            &enchanted,
            &mut LegacyRandomSource::new(1),
        );
        assert_eq!(got.item(), ENCHANTED_BOOK);
        assert_eq!(got.count, 1);
        assert_eq!(got.stored_enchantments.get("minecraft:power"), Some(&1));
    }

    #[test]
    fn without_enchantability_nothing_is_drawn() {
        let stick = Stack::new(item("minecraft:stick", None), 1);
        let options = [enchantment(
            "minecraft:sharpness",
            10,
            5,
            &["minecraft:stick"],
        )];
        let mut random = LegacyRandomSource::new(9);
        let got = with_levels(
            stick,
            30,
            &options,
            &item(ENCHANTED_BOOK, None),
            &mut random,
        );
        assert!(got.enchantments.is_empty());
        assert_eq!(random.next_long(), LegacyRandomSource::new(9).next_long());
    }

    #[test]
    fn costs_and_compatibility() {
        assert_eq!(cost_at((1, 11), 1), 1);
        assert_eq!(cost_at((1, 11), 4), 34);
        let mut a = (*enchantment("minecraft:a", 1, 1, &[])).clone();
        let b = enchantment("minecraft:b", 1, 1, &[]);
        assert!(compatible(&a, &b));
        assert!(!compatible(&a, &a.clone()));
        a.exclusive_set.insert("minecraft:b".into());
        assert!(!compatible(&a, &b) && !compatible(&b, &a));
    }
}
