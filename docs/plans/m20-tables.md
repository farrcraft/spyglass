# m20. Loot tables

- Status: In progress
- Milestone: [20. Loot](../roadmap/20-loot.md)
- Started: 2026-11-04
- Completed:

## Goal

A container's contents from its loot table and loot seed, slot by slot, exactly as the game fills it. This is the half of the milestone that needs no worldgen: given a table id, a seed and the container's position, spyglass gives the game's items, counts, enchantments and other components, in the game's slots. [m20-containers](m20-containers.md) finds the tables and seeds; [m20-queries](m20-queries.md) asks about them. It meets the first exit criterion's rolling half: for every container table of every supported version, the game's contents for any seed.

## Approach

**How the game fills a container**, the same draws in 26.2, 26.3 and 26.4-snapshot-7 (the spike, in the notes). When a container with a loot table is first opened, broken or emptied by a hopper, `RandomizableContainer.unpackLootTable` calls `LootTable.fill(container, params, seed)` with the origin at the block's centre:

1. `LootContext.Builder.withOptionalRandomSeed(seed)`: a seed other than 0 gives `RandomSource.create(seed)`, a `LegacyRandomSource`, shared by everything below, nested tables included. Every vanilla container table names a `random_sequence`, but a seeded fill never uses it. A seed of 0 would use the world's random sequence, which isn't the seed's. No vanilla container gets 0 except by a 1-in-2⁶⁴ draw.
2. `getRandomItems`: each pool's condition, rolls and entries, the entries' modifiers, and nested `loot_table` entries, in order, with stacks over their item's maximum cut up (`createStackSplitter`). A modifier draws when its stack is made, between the pool's draws.
3. `getAvailableSlots`: the container's empty slots, in order, shuffled with the same random.
4. `shuffleAndSplitItems`: empty stacks dropped, stacks split at random until the free slots are used, then all shuffled again.
5. Each stack goes into the slot taken from the end of the slot list.

**What vanilla's container tables use.** Counted over the `chests/`, `barrels/`, `pots/` and `dispensers/` tables of 26.2, 26.3 and 26.4-snapshot-7 (60, 63 and 63 tables):

- entries: `item`, `empty`, `loot_table`;
- number providers: `uniform` (and constants);
- modifiers: `set_count`, `enchant_randomly`, `enchant_with_levels`, `set_damage`, `set_potion`, `set_stew_effect`, `set_instrument`, `set_name`, `set_ominous_bottle_amplifier`, `set_enchantments`, `exploration_map`, and from 26.3 `filtered` with `on_fail: discard`;
- conditions: `random_chance`, `location_check` on the container's biome, from 26.3 `inverted`.

No entry has a `quality` and no pool has `bonus_rolls`, so the opener's luck changes nothing in vanilla. Tables keyed to the opener stay out of scope, as the milestone says.

**The format changed in 26.3.** 26.2 has `functions` and `conditions` lists with `function` and `condition` ids; 26.3 has a `modifier` (one, or a list) whose objects have a `type`, and a `condition` object. Loaders tell them apart by shape, as everywhere ([ADR 0026](../adr/0026-how-game-versions-coexist.md)). A loot spec section lists both.

**Data the bundle needs.** Today's bundle has none of this. It gains `data/minecraft/loot_table/`, `enchantment/`, `instrument/`, `tags/item/` and `tags/enchantment/` from the jar, and the item properties rolling reads: each item's maximum stack size (splitting), its enchantability and enchantable tags (enchanting), and its maximum damage (`set_damage`). Those are default item components, which the data generator's reports write one item a file, `reports/minecraft/components/item/<id>.json`, in every supported version; the bundle copies them. Adding directories doesn't change `format` ([bundle-format.md](../specs/bundle-format.md)); loaders that need them ask for the bundle to be extracted again.

**Enchanting** is the hard part: `EnchantmentHelper.selectEnchantment` (levels, the item's enchantability, the cost ranges, weights and exclusive sets of the enchantments in `options` or the tag), and `enchant_randomly`'s pick from the compatible ones. Its order of draws is ported from each version's bytecode. Enchantments are data from 1.21, so this is algorithms over the bundle's `enchantment/` files.

**Exploration maps** search for the nearest structure from the container (`findNearestMapStructure`), whose answer spyglass's locate can give, and whose `skip_existing_chunks` depends on what the world has explored. Rolling gives the map's destination tag, decoration and zoom and leaves its target to the caller. [m20-containers](m20-containers.md) fills it in for a fresh world, and says why a save's can differ.

**Where it lives.** A new crate, `spyglass-loot`, above `spyglass-data` and `spyglass-rng`, beside `spyglass-feature`: loading tables, items and enchantments, and `roll(table, seed, origin) -> Contents`. It needs no worldgen except the container's biome for `location_check`, which the caller passes in.

**The oracle.** A new `cargo xtask oracle loot` run fills a stand-in container for each container table of the version, for a fixed list of seeds (1,000 a table, including `i64::MIN`, `−1`, `1` and `i64::MAX`), with the origin in a few biomes for the tables that check one. It records each slot's item, count and components as the game serializes them. The golden file stays local ([ADR 0008](../adr/0008-game-derived-test-values-stay-local.md)). The comparison is an ignored test in `spyglass-loot`. Maps are compared without their target.

## Tasks

- [x] **Spike:** `LootTable.fill` and everything it calls, in 26.2, 26.3 and 26.4-snapshot-7: draw order, the splitter, slot shuffles, each modifier, number provider and condition above, `selectEnchantment`. Where the versions differ. Whether `reports/items.json` has the components rolling reads, in every version. Findings in the notes, reconstructions kept locally. _The same draws in all three; the JSON differs; item components are per-item reports._
- [x] **ADR:** loot in the bundle: the new directories and where item properties come from. _[ADR 0059](../adr/0059-loot-data-in-the-bundle.md); the crate is [ADR 0060](../adr/0060-loot-in-its-own-crate.md)._
- [x] **Extractor:** the directories and item properties; bundle-format.md updated. _26.2 and 26.3 re-extracted; tag resolution moved down to `spyglass-density`, beside `Resources`._
- [x] **`spyglass-loot`:** loading 26.2's and 26.3's forms; items; the random; pools, entries and conditions; the splitter, the slots and the shuffles. _Every container table of 26.2, 26.3 and 26.4-snapshot-7 loads; a fill takes about 15 µs._
- [x] **Modifiers:** each one above, hand-worked tests for each, without game data. _`tests/fill.rs`; the game's own answers come with the comparison._
- [x] **Enchanting:** `enchant_with_levels`, `enchant_randomly`, `set_enchantments`, from the enchantment data. _Without `options` (the whole registry in its order) and `set_enchantments` with several random levels are refused: vanilla has neither._
- [ ] **Oracle:** `oracle loot` and its golden file, and the CI unit for it ([ADR 0058](../adr/0058-oracle-ci-planned-by-cost.md)): a new `Unit` kind and its `COSTS`.
- [ ] **Comparison:** every table, every seed, every version: slot for slot the game's.
- [ ] **Spec:** loot.md: filling, each modifier and condition, the versions' forms.
- [ ] **Cost:** a roll in `cargo xtask bench`.
- [ ] `cargo xtask check` and `cargo xtask oracle-test`. Complete the plan.

## Verification

- The loot comparison: for each supported version, every container table and seed gives the game's slots, items, counts and components.
- Hand-worked tests for each modifier, condition and enchanting step, committed without game data.
- A roll's time in the bench: a seed search may roll many containers per seed.

## Durable outputs

- [ ] loot.md: filling a container, the tables' forms per version, each modifier and condition.
- [x] bundle-format.md: the loot, enchantment, instrument and tag directories; item properties.
- [ ] oracle.md: `oracle loot`, its golden file and comparison.
- [x] An ADR for loot data in the bundle.

## Notes

- 2026-11-04: **Spike: the fill** (26.2, 26.3, 26.4-snapshot-7; reconstructions in `target/spike/loot/`, never committed). The draws are the same in all three; the JSON differs.
  - **Pools:** the condition first (a `random_chance` draws there), then `rolls.getInt`, then `bonus_rolls.getFloat` times luck, floored. Each roll expands the entries again, so entry conditions and their draws run on every roll. Choices with weight above 0 count (`max(floor(weight + quality × luck), 0)`). No choice or a total of 0: no draw; one choice: no draw; otherwise `nextInt(total)` and the first whose running subtraction goes below 0.
  - **Entries:** `empty` makes nothing and its modifiers never run; `item` a stack of 1; `loot_table` rolls the other table with the same random, without its sequence or the splitter. From 26.3 its `value` is a set with `expand`; vanilla gives one id, which acts as 26.2's.
  - **Modifiers** run innermost first (entry, pool, table), then the splitter, which only the outer table has. It drops a stack whose item a feature flag disables, after its draws, and cuts one at or over its maximum into pieces of at most the maximum.
  - **Numbers:** `Mth.nextInt(r, min, max)` is `min` without a draw when `min ≥ max`, else `nextInt(max − min + 1) + min`; the float uniform is `nextFloat() × (max − min) + min` in `f32`, likewise. `min` is evaluated before `max`. 26.2 has one float provider (an int is `Math.round` of it; an object without `type` is uniform); 26.3 has int and float providers apart, and a bare number is a constant. JSON floats are `Float.parseFloat` of the literal: parse to `f32` directly.
  - **Conditions:** `random_chance` evaluates its chance, then `nextFloat() < chance`. `location_check` is the container's block plus an offset (0 in vanilla); false if the chunk isn't loaded, else the biome by `BiomeManager.getBiome`, the zoomed lookup ([biome-lookup.md](../specs/biome-lookup.md)), not the quart's. `inverted` is `!term`. 26.2 ANDs a `conditions` list in order; 26.3 has one `condition`.
  - **Slots:** `Util.shuffle` from `i = size` down to 2: `j = nextInt(i)`, swap `i − 1` and `j`. Then the split: empties dropped; stacks over 1 moved to a split list in order; while free slots exceed the stacks and the split list isn't empty, take one at `Mth.nextInt(0, size − 1)`, split off `Mth.nextInt(1, count / 2)`, and for the rest, then the piece, a count over 1 draws `nextBoolean` (true back to the split list, false to the items) and a count of 1 goes to the items. The split list's rest is appended and the whole shuffled.
  - **The random** is `java.util.Random`'s in every method used, all in `spyglass-rng`'s `LegacyRandom` already. `Mth.nextInt` and `Util.shuffle` exist privately in `spyglass-data` and `spyglass-structure`; the loot crate shares or repeats them.
- 2026-11-04: **Spike: enchanting.** The same in all three versions. `nextInt(1)` still draws.
  - **`enchant_randomly`:** the `options` set in its order (every vanilla table gives one); unless the item is a book, only those whose `supported_items` hold it (`only_compatible`, default true). `nextInt(n)` picks one, a draw even for 1, then `nextInt(max_level) + 1` if `max_level > 1`. An empty set leaves the stack without a draw. A book becomes an enchanted book of 1.
  - **`enchant_with_levels`:** the levels' draw, then `selectEnchantment`: nothing without a draw if the item has no `enchantable`; else `cost = levels + 1 + nextInt(e/4 + 1) + nextInt(e/4 + 1)`, `bonus = (nextFloat() + nextFloat() − 1) × 0.15` and `cost = clamp(round(cost + cost × bonus), 1, max)` in `f32`. The candidates, in the set's order, whose `primary_items` (or `supported_items`) hold the item, or all for a book, each at its highest level with `min_cost ≤ cost ≤ max_cost`. A weighted pick (`nextInt(total)`, subtracting in order), then while `nextInt(50) ≤ cost`: drop the candidates incompatible with the last pick (itself included), stop if none are left, pick again, halve the cost. A book becomes an enchanted book even with nothing picked.
  - **`set_enchantments`:** levels clamped to 0..255 in the map's key order, no check; a book becomes an enchanted book keeping its count. Vanilla's one use is `wind_burst` 1.
  - **Costs:** `base + per_level_above_first × (level − 1)` in `int`. Compatible means different, and neither in the other's `exclusive_set`. Enchantments are held in a hash map, so compare them as maps.
  - **Tag order** is the file's `values`, nested tags expanded in place, repeats dropped after the first: `#on_random_loot` is `#non_treasure`'s 36 then 4 more. The data is the same in all three versions.
  - **Items:** no `reports/items.json`; `--reports` writes `reports/minecraft/components/item/<id>.json` with every default component (1,537, 1,658 and 1,686 items), including `max_stack_size`, `enchantable` (77 items) and `max_damage` (84).
- 2026-11-04: **Spike: other modifiers.** The same in all three versions, except maps.
  - **`set_count`:** the count is the provider's (plus the old with `add`), unclamped; the splitter cuts it later.
  - **`set_damage`:** an item that can't be damaged returns before the provider draws. Else damage = `floor((1 − clamp(d, 0, 1)) × max_damage)` in `f32`.
  - **`set_potion`, `set_name`:** no draws. **`set_stew_effect`:** on suspicious stew only, `nextInt(effects)` then the duration, in seconds times 20 unless the effect is instant. **`set_instrument`:** `nextInt(size)` of the tag in its order. **`set_ominous_bottle_amplifier`:** the provider clamped to 0..4.
  - **`exploration_map`** never draws from the loot random: it searches from the container's block, `search_radius` (default 50) counted in placement cells. 26.2 needs a `map`, gives a new `filled_map` with a map id and its marker, or the plain map if none is found. From 26.3 it writes onto the same stack (so `set_name` stays), and leaves it as it was if none is found, which `filtered` then discards; with `skip_existing_chunks` it first claims the structure the container is in. A map's id comes from the world's counter: compare maps without it.
  - **`filtered`:** `item_filter` passes or fails the stack to `on_pass` or `on_fail`; `discard` empties it, and an empty stack takes no slot.

- 2026-11-04: from the survey that wrote this plan (26.3 bytecode): `withOptionalRandomSeed` is `seed != 0 ? RandomSource.create(seed) : null`, and `create` prefers that random over the table's sequence; `RandomSource.create(long)` is `new LegacyRandomSource(seed)`. The tables are in the server jar under `data/minecraft/loot_table/`, not only in the generator's output.
