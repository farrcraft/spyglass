//! `LootTable.fill`: rolling a table's pools with one seeded random, then putting the stacks
//! into shuffled slots ([loot.md](../../../docs/specs/loot.md#filling)).
//!
//! Every stack a choice makes passes through the modifiers of everything around it, innermost
//! first: its entry's, any composite's, its pool's, its table's, and for a nested table the same
//! again outward. They're kept as a chain of modifier lists, outermost first, so a stack is
//! modified the moment it's made, between the pool's draws, as in the game.

use spyglass_rng::{LegacyRandomSource, RandomSource};

use crate::model::{
    Condition, Entry, EntryKind, FloatProvider, IntProvider, Makes, Modifier, ModifierKind, Pool,
};
use crate::random::{self, next_float, next_int, shuffle};
use crate::stack::{MapMark, Stack};
use crate::{Loot, MapRequest, World, enchant};

/// The contents of a container of `slots` slots at `at`, from table `id` and `seed` (not 0).
pub(crate) fn fill(
    loot: &Loot,
    id: &str,
    seed: i64,
    at: [i32; 3],
    slots: usize,
    world: &dyn World,
) -> Vec<Option<Stack>> {
    let mut roll = Roll {
        loot,
        random: LegacyRandomSource::new(seed),
        at,
        world,
        // No player opens it, and the opener's luck changes nothing in vanilla's tables.
        luck: 0.0,
        visiting: Vec::new(),
        items: Vec::new(),
    };
    roll.table(id, &mut Vec::new());
    let Roll {
        mut random,
        mut items,
        ..
    } = roll;

    let mut free: Vec<usize> = (0..slots).collect();
    shuffle(&mut free, &mut random);
    shuffle_and_split(&mut items, free.len(), &mut random);
    let mut contents = vec![None; slots];
    for stack in items {
        let Some(slot) = free.pop() else {
            break;
        };
        contents[slot] = Some(stack);
    }
    contents
}

/// One fill's state.
struct Roll<'a> {
    loot: &'a Loot,
    random: LegacyRandomSource,
    at: [i32; 3],
    world: &'a dyn World,
    luck: f32,
    /// The tables being rolled, outermost first: a table that reaches itself rolls nothing.
    visiting: Vec<&'a str>,
    /// The stacks made so far, after the splitter.
    items: Vec<Stack>,
}

/// A choice an expanded entry gives: the singleton or expanded entry, the table it names for
/// an expanded one, and the composites' modifier lists around it, outermost first.
struct Choice<'a> {
    weight: i32,
    makes: Made<'a>,
    wrappers: Vec<&'a [Modifier]>,
}

enum Made<'a> {
    Entry(&'a Entry, &'a Makes),
    Table(&'a Entry, &'a str),
}

impl<'a> Roll<'a> {
    /// `getRandomItemsRaw`: the table's pools in order, its modifiers around them.
    fn table(&mut self, id: &'a str, chain: &mut Vec<&'a [Modifier]>) {
        let Some((key, table)) = self.loot.tables.get_key_value(id) else {
            return;
        };
        if self.visiting.contains(&key.as_str()) {
            return;
        }
        self.visiting.push(key);
        chain.push(&table.modifiers);
        for pool in &table.pools {
            self.pool(pool, chain);
        }
        chain.pop();
        self.visiting.pop();
    }

    /// `LootPool.addRandomItems`: the condition, the rolls, then one pick a roll.
    fn pool(&mut self, pool: &'a Pool, chain: &mut Vec<&'a [Modifier]>) {
        if !self.all(&pool.conditions) {
            return;
        }
        chain.push(&pool.modifiers);
        let rolls = self.int(&pool.rolls);
        let bonus = random::floor(self.float(&pool.bonus_rolls) * self.luck);
        for _ in 0..rolls.wrapping_add(bonus) {
            self.roll_once(pool, chain);
        }
        chain.pop();
    }

    /// `addRandomItem`: expand every entry, then pick one choice by weight.
    fn roll_once(&mut self, pool: &'a Pool, chain: &mut Vec<&'a [Modifier]>) {
        let mut choices = Vec::new();
        for entry in &pool.entries {
            self.expand(entry, &mut Vec::new(), &mut choices);
        }
        let total = choices
            .iter()
            .map(|c| c.weight)
            .fold(0i32, i32::wrapping_add);
        let picked = match choices.len() {
            _ if total == 0 => return,
            0 => return,
            1 => &choices[0],
            _ => {
                let mut left = self.random.next_int_bounded(total);
                let Some(choice) = choices.iter().find(|c| {
                    left -= c.weight;
                    left < 0
                }) else {
                    return;
                };
                choice
            }
        };
        let depth = chain.len();
        chain.extend(&picked.wrappers);
        match picked.makes {
            Made::Entry(entry, makes) => {
                chain.push(&entry.modifiers);
                match makes {
                    Makes::Item(item) => self.emit(Stack::new(item.clone(), 1), chain),
                    Makes::Empty => {}
                    Makes::Tables(tables) => {
                        for table in tables {
                            self.table(table, chain);
                        }
                    }
                }
            }
            Made::Table(entry, table) => {
                chain.push(&entry.modifiers);
                self.table(table, chain);
            }
        }
        chain.truncate(depth);
    }

    /// `LootPoolEntryContainer.expand`: whether the entry holds, adding its choices.
    fn expand(
        &mut self,
        entry: &'a Entry,
        wrappers: &mut Vec<&'a [Modifier]>,
        choices: &mut Vec<Choice<'a>>,
    ) -> bool {
        if !self.all(&entry.conditions) {
            return false;
        }
        let mut add = |weight: i32, quality: i32, makes: Made<'a>, wrappers: &[&'a [Modifier]]| {
            // `getWeight(luck)`: floor(weight + quality × luck) in f32, at least 0.
            let weight = random::floor(weight as f32 + quality as f32 * self.luck).max(0);
            if weight > 0 {
                choices.push(Choice {
                    weight,
                    makes,
                    wrappers: wrappers.to_vec(),
                });
            }
        };
        match &entry.kind {
            EntryKind::Singleton {
                weight,
                quality,
                makes,
            } => {
                add(*weight, *quality, Made::Entry(entry, makes), wrappers);
                true
            }
            EntryKind::Expanded {
                weight,
                quality,
                tables,
            } => {
                for table in tables {
                    add(*weight, *quality, Made::Table(entry, table), wrappers);
                }
                true
            }
            EntryKind::Alternatives(children)
            | EntryKind::Sequence(children)
            | EntryKind::Group(children) => {
                wrappers.push(&entry.modifiers);
                let held = match &entry.kind {
                    EntryKind::Alternatives(_) => {
                        children.iter().any(|c| self.expand(c, wrappers, choices))
                    }
                    EntryKind::Sequence(_) => {
                        children.iter().all(|c| self.expand(c, wrappers, choices))
                    }
                    _ => {
                        for child in children {
                            self.expand(child, wrappers, choices);
                        }
                        true
                    }
                };
                wrappers.pop();
                held
            }
        }
    }

    /// A stack made: through the chain's modifiers, innermost first, then the splitter.
    fn emit(&mut self, mut stack: Stack, chain: &[&'a [Modifier]]) {
        for modifiers in chain.iter().rev() {
            stack = self.modify(stack, modifiers);
        }
        // `createStackSplitter`: below its item's maximum it's kept whole (an empty stack too);
        // else it's cut into stacks of at most the maximum.
        let max = stack.item.max_stack_size;
        if stack.count < max {
            self.items.push(stack);
            return;
        }
        let mut left = stack.count;
        while left > 0 {
            let mut piece = stack.clone();
            piece.count = max.min(left);
            left -= piece.count;
            self.items.push(piece);
        }
    }

    /// The modifiers of a list, in order.
    fn modify(&mut self, mut stack: Stack, modifiers: &'a [Modifier]) -> Stack {
        for modifier in modifiers {
            if self.all(&modifier.conditions) {
                stack = self.run(stack, &modifier.kind);
            }
        }
        stack
    }

    /// One modifier ([loot.md](../../../docs/specs/loot.md#modifiers)).
    fn run(&mut self, mut stack: Stack, kind: &'a ModifierKind) -> Stack {
        match kind {
            ModifierKind::SetCount { count, add } => {
                let base = if *add { stack.count } else { 0 };
                stack.count = base.wrapping_add(self.int(count));
            }
            ModifierKind::SetDamage { damage, add } => {
                if !stack.item.damageable {
                    return stack;
                }
                let max = stack.item.max_damage;
                let added = if *add {
                    1.0 - stack.damage.unwrap_or(0) as f32 / max as f32
                } else {
                    0.0
                };
                let left = 1.0 - random::clamp(self.float(damage) + added, 0.0, 1.0);
                stack.damage = Some(random::floor(left * max as f32).clamp(0, max));
            }
            ModifierKind::SetPotion(potion) => stack.potion = Some(potion.clone()),
            ModifierKind::SetStewEffect(effects) => {
                if stack.item() != "minecraft:suspicious_stew" || effects.is_empty() {
                    return stack;
                }
                let (effect, duration, instant) =
                    &effects[self.random.next_int_bounded(effects.len() as i32) as usize];
                let mut ticks = self.int(duration);
                if !instant {
                    ticks = ticks.wrapping_mul(20);
                }
                stack.stew_effects.push((effect.clone(), ticks));
            }
            ModifierKind::SetInstrument(options) => {
                if !options.is_empty() {
                    let i = self.random.next_int_bounded(options.len() as i32) as usize;
                    stack.instrument = Some(options[i].clone());
                }
            }
            ModifierKind::SetName { name, target } => {
                if let Some(name) = name {
                    stack.name = Some((*target, name.clone()));
                }
            }
            ModifierKind::SetOminousBottleAmplifier(amplifier) => {
                stack.ominous_bottle_amplifier = Some(self.int(amplifier).clamp(0, 4));
            }
            ModifierKind::SetEnchantments {
                enchantments,
                add,
                enchanted_book,
            } => {
                // The levels are drawn in order, inside `updateEnchantments`.
                stack = enchant::set(stack, enchantments, *add, enchanted_book, |provider| {
                    self.int(provider)
                });
            }
            ModifierKind::EnchantRandomly {
                options,
                only_compatible,
                enchanted_book,
            } => {
                stack = enchant::randomly(
                    stack,
                    options,
                    *only_compatible,
                    enchanted_book,
                    &mut self.random,
                );
            }
            ModifierKind::EnchantWithLevels {
                levels,
                options,
                enchanted_book,
            } => {
                let levels = self.int(levels);
                stack =
                    enchant::with_levels(stack, levels, options, enchanted_book, &mut self.random);
            }
            ModifierKind::ExplorationMap {
                destination,
                decoration,
                zoom,
                search_radius,
                skip_existing_chunks,
                new_filled_map,
            } => {
                let legacy = new_filled_map.is_some();
                if (legacy && stack.item() != "minecraft:map") || stack.is_empty() {
                    return stack;
                }
                let request = MapRequest {
                    from: self.at,
                    destination: destination.clone(),
                    search_radius: *search_radius,
                    skip_existing_chunks: *skip_existing_chunks,
                };
                let Some(target) = self.world.map_target(&request) else {
                    return stack;
                };
                if let Some(filled_map) = new_filled_map {
                    stack = Stack::new(filled_map.clone(), 1);
                }
                stack.map = Some(MapMark {
                    target,
                    decoration: decoration.clone(),
                    zoom: *zoom,
                });
            }
            ModifierKind::Filtered {
                filter,
                on_pass,
                on_fail,
            } => {
                let passes = filter
                    .items
                    .as_ref()
                    .is_none_or(|i| i.contains(stack.item()))
                    && (!filter.has_map_id || stack.map.is_some());
                let then = if passes { on_pass } else { on_fail };
                if let Some(modifiers) = then {
                    stack = self.modify(stack, modifiers);
                }
            }
            ModifierKind::Discard => stack.count = 0,
        }
        stack
    }

    /// Whether every condition holds, tested in order until one doesn't.
    fn all(&mut self, conditions: &[Condition]) -> bool {
        conditions.iter().all(|c| self.holds(c))
    }

    fn holds(&mut self, condition: &Condition) -> bool {
        match condition {
            Condition::RandomChance(chance) => {
                let chance = self.float(chance);
                self.random.next_float() < chance
            }
            Condition::Location { offset, biomes } => {
                let Some(biomes) = biomes else {
                    return true;
                };
                let [x, y, z] = self.at;
                let [dx, dy, dz] = *offset;
                self.world
                    .biome(x.wrapping_add(dx), y.wrapping_add(dy), z.wrapping_add(dz))
                    .is_some_and(|biome| biomes.contains(&biome))
            }
            Condition::Inverted(term) => !self.holds(term),
            Condition::AllOf(terms) => terms.iter().all(|t| self.holds(t)),
            Condition::AnyOf(terms) => terms.iter().any(|t| self.holds(t)),
        }
    }

    fn int(&mut self, provider: &IntProvider) -> i32 {
        match provider {
            IntProvider::Constant(value) => *value,
            IntProvider::Uniform(min, max) => {
                let min = self.int(min);
                let max = self.int(max);
                next_int(&mut self.random, min, max)
            }
        }
    }

    fn float(&mut self, provider: &FloatProvider) -> f32 {
        match provider {
            FloatProvider::Constant(value) => *value,
            FloatProvider::Uniform(min, max) => {
                let min = self.float(min);
                let max = self.float(max);
                next_float(&mut self.random, min, max)
            }
        }
    }
}

/// `shuffleAndSplitItems`: empty stacks dropped, stacks split at random while there are free
/// slots to fill, then everything shuffled.
fn shuffle_and_split(items: &mut Vec<Stack>, slots: usize, random: &mut LegacyRandomSource) {
    let mut split = Vec::new();
    let mut kept = Vec::with_capacity(items.len());
    for stack in items.drain(..) {
        if stack.is_empty() {
            continue;
        }
        if stack.count > 1 {
            split.push(stack);
        } else {
            kept.push(stack);
        }
    }
    *items = kept;
    while slots as i64 - items.len() as i64 - split.len() as i64 > 0 && !split.is_empty() {
        let at = next_int(random, 0, split.len() as i32 - 1) as usize;
        let mut stack = split.remove(at);
        let n = next_int(random, 1, stack.count / 2);
        let part = stack.split(n);
        for piece in [stack, part] {
            if piece.count > 1 && random.next_boolean() {
                split.push(piece);
            } else {
                items.push(piece);
            }
        }
    }
    items.append(&mut split);
    shuffle(items, random);
}
