# Loot

Status: draft. Covers filling a container from its loot table and loot seed (`LootTable.fill`), as the game does when a container is first opened, broken or emptied by a hopper: the tables' forms, pools, entries, number providers, conditions, modifiers and enchanting, and how the items go into slots. Implemented in `spyglass-loot` ([ADR 0060](../adr/0060-loot-in-its-own-crate.md)), with the random in `spyglass-rng` ([rng.md](rng.md)) and the data from the bundle ([ADR 0059](../adr/0059-loot-data-in-the-bundle.md)). Checked against the oracle's [loot golden files](oracle.md#loot-golden-files) in 26.2, 26.3 and 26.4-snapshot-5 to 7: every fill of every container table matches.

Ported from the 26.2, 26.3 and 26.4-snapshot-7 bytecode. The draws are the same in all three; the JSON differs ([Forms](#forms)).

## Data

From the bundle: tables in `loot_table/`, enchantments in `enchantment/`, instruments in `instrument/`, the item, enchantment and instrument tags in `tags/item/`, `tags/enchantment/` and `tags/instrument/`, and each item's default components in `reports/minecraft/components/item/<id>.json` ([bundle-format.md](bundle-format.md#layout)). Of the components, filling reads `max_stack_size`, `enchantable` and `max_damage`.

A tag's members are in its file's `values` order, a nested `#tag` expanded in place, a repeat kept only where it first appears, and an optional entry (`{"id": ..., "required": false}`) skipped if it names nothing (`TagLoader`). Vanilla's container tables use these orders: `enchant_randomly`'s and `enchant_with_levels`'s `options`, and `set_instrument`'s tag.

## Filling

`RandomizableContainer.unpackLootTable` fills with the origin at the container block's centre (`x + 0.5`, `y + 0.5`, `z + 0.5`) and the opener's luck.

1. **The random.** A seed other than 0 gives `new LegacyRandomSource(seed)`, used by every step below, nested tables included. The table's `random_sequence` isn't used. A seed of 0 takes the world's random sequence instead, which isn't the seed's; spyglass doesn't roll it.
2. **The items.** The table's pools in order ([Pools](#pools)). Every stack made goes through its entry's modifiers, then the pool's, then the table's ([Modifiers](#modifiers)), and then the splitter: a stack whose item is disabled by a feature flag is dropped, a stack of less than its item's `max_stack_size` is kept, and a larger one is cut into stacks of `min(max, left)` in order. A modifier draws when its stack is made, between the pool's draws.
3. **The slots.** The container's empty slots in ascending order, then [shuffled](#shuffle).
4. **The split.** Empty stacks are dropped. The stacks of more than one move, in order, to a split list. While the free slots outnumber both lists together and the split list isn't empty:
   1. take the split list's stack at `nextInt(0, size − 1)` ([numbers](#numbers));
   2. split off `nextInt(1, count / 2)`, in integer division;
   3. the rest, then the part split off: one of more than one draws `nextBoolean()`, true going back to the split list and false to the items; one of one goes to the items without a draw.

   Then the split list's stacks are appended to the items, and the items [shuffled](#shuffle).

5. **Placing.** Each stack in order goes into the slot taken from the end of the slot list. With no slot left, the rest are dropped ("Tried to over-fill a container").

The luck changes only an entry's `quality` and a pool's `bonus_rolls`, which no vanilla container table has.

### Pools

1. The pool's condition, if any; false skips the pool.
2. The rolls: `rolls` as an int, plus `floor(bonus_rolls × luck)` in `f32` (`bonus_rolls` defaults to 0).
3. For each roll: every entry in order is expanded ([Entries](#entries)), each choice it gives counting with weight `max(floor(weight + quality × luck), 0)` if that's above 0. With no choice, or a total of 0, nothing and no draw; with one choice, it without a draw; otherwise `j = nextInt(total)`, and the first choice in order for which `j −= weight` goes below 0.

Entries are expanded again for every roll, so their conditions, and their draws, run again.

### Entries

An entry's condition is tested when it's expanded; false gives no choice. Weight defaults to 1 and quality to 0.

- **`item`:** one choice, which makes a stack of 1 of `name`.
- **`empty`:** one choice, which makes nothing: no stack, and its modifiers don't run.
- **`loot_table`:** one choice, which rolls the named table's pools with the same random and gives their stacks to this entry's modifiers. The named table's own `random_sequence` isn't used, and the splitter runs only once, in the outer table. A table that reaches itself again rolls nothing. From 26.3 `value` is a set of tables and `expand` (default false) makes one choice per table instead of one for all; vanilla names one table.
- **Composites** (`alternatives`, `sequence`, `group`, unused by vanilla's container tables): their children in order, stopping at the first that gives a choice, the first that doesn't, or never.

### Numbers

- `nextInt(min, max)` (`Mth.nextInt`) is `min` without a draw if `min ≥ max`, else `nextInt(max − min + 1) + min`.
- `nextFloat(min, max)` (`Mth.nextFloat`) is `min` without a draw if `min ≥ max`, else `nextFloat() × (max − min) + min` in `f32`.

A `uniform` is `nextInt(min, max)` as an int and `nextFloat(min, max)` as a float, `min` evaluated before `max`; a `constant` is its value. JSON floats are the literal's nearest `f32` (`Float.parseFloat`), not narrowed from `f64`.

### Shuffle

`Util.shuffle`: for `i` from the list's size down to 2, `j = nextInt(i)` and swap the elements at `i − 1` and `j`.

## Conditions

- **`random_chance`:** `chance` as a float, then `nextFloat() < chance`.
- **`location_check`:** the origin plus `offsetX`, `offsetY`, `offsetZ` (ints, default 0), floored to a block. False if that block's chunk isn't loaded; else the `predicate`'s `biomes` (an id, a `#tag` or a list) must hold the biome there by `BiomeManager.getBiome` ([biome-lookup.md](biome-lookup.md)): the zoomed biome at the container's block, not the quart's. No draws.
- **`inverted`:** the opposite of `term`.

## Modifiers

Each modifier's condition is tested first; false leaves the stack. Where a list is allowed, its members run in order.

- **`set_count`:** the count is `count` as an int, plus the old count with `add`. It isn't clamped: the splitter cuts it.
- **`set_damage`:** on an item that can't be damaged (without `max_damage` and `damage` components, or with `unbreakable`), nothing, and `damage` isn't evaluated. Else `damage` as a float `d`, and the stack's damage is `floor((1 − clamp(d, 0, 1)) × max_damage)` in `f32`, clamped to `0..max_damage`. With `add`, `d` is added to the share of durability left first.
- **`set_potion`, `set_name`:** no draws.
- **`set_stew_effect`:** on suspicious stew only, with a list that isn't empty: `nextInt(effects)` picks one, then its `duration` as an int, in seconds, times 20 unless the effect is instant (the game's instant effects, `instant_health`, `instant_damage` and `saturation`, which are code in every supported version).
- **`set_instrument`:** `nextInt(size)` of the tag's members in order ([Data](#data)); no draw for an empty tag.
- **`set_ominous_bottle_amplifier`:** `amplifier` as an int, clamped to `0..4`.
- **`set_enchantments`:** each of `enchantments` in the file's key order, its level as an int clamped to `0..255` (with `add`, plus the stack's level), level 0 removing it. A book becomes an enchanted book, keeping its count.
- **`enchant_randomly`**, **`enchant_with_levels`:** [Enchanting](#enchanting).
- **`exploration_map`:** no draws from the loot random ([Maps](#maps)).
- **`filtered`** (26.3 on): `item_filter` tests the stack; `on_pass` or `on_fail`, if given, runs on it. `discard` gives an empty stack, which takes no slot.

### Enchanting

**`enchant_randomly`.** The candidates are `options` in order (the whole registry, sorted by id, without it). Unless the item is a book, with `only_compatible` (default true) only those whose `supported_items` hold the item are kept. With none, the stack is left without a draw. `nextInt(n)` picks one, a draw even for one candidate, then its level is `nextInt(1, max_level)`. A book becomes an enchanted book of one.

**`enchant_with_levels`.** `levels` as an int, then `selectEnchantment`:

1. An item without `enchantable` gets nothing, without a draw. Else, with `e` its value: `cost = levels + 1 + nextInt(e / 4 + 1) + nextInt(e / 4 + 1)`, then `bonus = (nextFloat() + nextFloat() − 1) × 0.15`, and `cost = clamp(round(cost + cost × bonus), 1, i32::MAX)`, in `f32` with `Math.round`.
2. The candidates are `options` in order whose `primary_items` (or, without it, `supported_items`) hold the item, or all of them for a book, each at its highest level from `max_level` down whose `min_cost ≤ cost ≤ max_cost`; one with no such level is left out.
3. With candidates: a weighted pick, then while `nextInt(50) ≤ cost`: drop every candidate not compatible with the last pick (the pick among them), stop if none are left, pick again, and halve the cost (integer division).

A weighted pick sums the weights, draws `nextInt(total)`, and takes the first whose running subtraction goes below 0. A book becomes an enchanted book of one, even with nothing picked.

An enchantment's cost at a level is `base + per_level_above_first × (level − 1)`, in `int`. Two are compatible if they differ and neither's `exclusive_set` holds the other. A stack holds each enchantment once, at the higher level if added twice; its enchantments have no order.

### Maps

`exploration_map` searches for the nearest structure of `destination` (a structure tag) from the container's block, within `search_radius` (default 50) placement cells, skipping chunks already generated with `skip_existing_chunks` (default true), and marks it with `decoration` (default `mansion`) at `zoom` (default 2). The map's id comes from the world's counter. A fill gives the destination, the decoration and the zoom; where the map points is for the container's caller, which knows the world.

_Versions:_ 26.2 needs a `map`, and gives a new `filled_map` (count 1, earlier components dropped), or the plain map if nothing is found. From 26.3 the destination is a holder set (`#tag`), the stack itself is marked (so a `set_name` before it stays), and nothing found leaves it as it was, which vanilla's `filtered` then discards; with `skip_existing_chunks`, a destination structure whose box holds the container is claimed first.

## Forms

The draws above are the same in every supported version; the JSON differs, and the loaders tell the forms apart by their shape ([ADR 0026](../adr/0026-how-game-versions-coexist.md)).

- **26.2:** pools, entries and modifiers take a `conditions` list (all must hold, in order) and a `functions` list. A modifier object's id is in `function` and a condition's in `condition`. Number providers are one float-valued kind: a number is a constant, an object without `type` is a uniform, and an int is `Math.round` of the float. A `loot_table` entry's `value` is an id or an inline table.
- **26.3 on:** one optional `condition` and one optional `modifier` (an object, a list run in order, or an id), each object's id in `type`. Composite entries take them too. Number providers are an int kind and a float kind: a number is a constant, an object has a `type`, and a string names a registered provider. Rolls, counts, levels, durations and amplifiers are ints; `bonus_rolls`, `chance` and `damage` are floats.

## Testing

Hand-worked tests in `spyglass-loot` (`tests/fill.rs`): both forms of one table fill alike, and single draws checked by hand. `every_container_table_loads_and_fills` loads every container table of a bundle and fills each for 1,000 seeds. `every_fill_matches_the_game` compares 500 fills of every container table with the game's, slot for slot, from the oracle's [loot golden files](oracle.md#loot-golden-files): all match in every supported version.
