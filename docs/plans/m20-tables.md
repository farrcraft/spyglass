# m20. Loot tables

- Status: Draft
- Milestone: [20. Loot](../roadmap/20-loot.md)
- Started:
- Completed:

## Goal

A container's contents from its loot table and loot seed, slot by slot, exactly as the game fills it. This is the half of the milestone that needs no worldgen: given a table id, a seed and the container's position, spyglass gives the game's items, counts, enchantments and other components, in the game's slots. [m20-containers](m20-containers.md) finds the tables and seeds; [m20-queries](m20-queries.md) asks about them. It meets the first exit criterion's rolling half: for every container table of every supported version, the game's contents for any seed.

## Approach

**How the game fills a container** (26.3 bytecode, to be confirmed in every version by the spike). When a container with a loot table is first opened, broken or emptied by a hopper, `RandomizableContainer.unpackLootTable` calls `LootTable.fill(container, params, seed)`:

1. `LootContext.Builder.withOptionalRandomSeed(seed)`: a seed other than 0 gives `RandomSource.create(seed)`, a `LegacyRandomSource`. Every vanilla container table names a `random_sequence`, but a seeded fill never uses it. A seed of 0 would use the world's random sequence, which isn't the seed's. No vanilla container gets 0 except by a 1-in-2⁶⁴ draw.
2. `getRandomItems`: each pool's rolls, its entries' weights and conditions, the entries' modifiers, and nested `loot_table` entries, in order, with stacks over their item's maximum split (`createStackSplitter`).
3. `getAvailableSlots`: the container's empty slots, shuffled with the same random.
4. `shuffleAndSplitItems`: stacks split at random into the free slots, then shuffled again.
5. Each stack goes into the next slot; an empty stack takes a slot and leaves it empty.

**What vanilla's container tables use.** Counted over the `chests/`, `barrels/`, `pots/` and `dispensers/` tables of 26.2, 26.3 and 26.4-snapshot-7 (60, 63 and 63 tables):

- entries: `item`, `empty`, `loot_table`;
- number providers: `uniform` (and constants);
- modifiers: `set_count`, `enchant_randomly`, `enchant_with_levels`, `set_damage`, `set_potion`, `set_stew_effect`, `set_instrument`, `set_name`, `set_ominous_bottle_amplifier`, `set_enchantments`, `exploration_map`, and from 26.3 `filtered` with `on_fail: discard`;
- conditions: `random_chance`, `location_check` on the container's biome, from 26.3 `inverted`.

No entry has a `quality` and no pool has `bonus_rolls`, so the opener's luck changes nothing in vanilla. Tables keyed to the opener stay out of scope, as the milestone says.

**The format changed in 26.3.** 26.2 has `functions` and `conditions` lists with `function` and `condition` ids; 26.3 has a `modifier` (one, or a list) whose objects have a `type`, and a `condition` object. Loaders tell them apart by shape, as everywhere ([ADR 0026](../adr/0026-how-game-versions-coexist.md)). A loot spec section lists both.

**Data the bundle needs.** Today's bundle has none of this. It gains `data/minecraft/loot_table/`, `enchantment/`, `instrument/`, `tags/item/` and `tags/enchantment/` from the jar, and the item properties rolling reads: each item's maximum stack size (splitting), its enchantability and enchantable tags (enchanting), and its maximum damage (`set_damage`). Those are default item components. If the data generator's `reports/items.json` has them in every supported version, the bundle copies it. If not, the extractor's block report ([ADR 0054](../adr/0054-block-behaviour-from-the-running-game.md)) grows an item part. Adding directories doesn't change `format` ([bundle-format.md](../specs/bundle-format.md)); loaders that need them ask for the bundle to be extracted again.

**Enchanting** is the hard part: `EnchantmentHelper.selectEnchantment` (levels, the item's enchantability, the cost ranges, weights and exclusive sets of the enchantments in `options` or the tag), and `enchant_randomly`'s pick from the compatible ones. Its order of draws is ported from each version's bytecode. Enchantments are data from 1.21, so this is algorithms over the bundle's `enchantment/` files.

**Exploration maps** search for the nearest structure from the container (`findNearestMapStructure`), whose answer spyglass's locate can give, and whose `skip_existing_chunks` depends on what the world has explored. Rolling gives the map's destination tag, decoration and zoom and leaves its target to the caller. [m20-containers](m20-containers.md) fills it in for a fresh world, and says why a save's can differ.

**Where it lives.** A new crate, `spyglass-loot`, above `spyglass-data` and `spyglass-rng`, beside `spyglass-feature`: loading tables, items and enchantments, and `roll(table, seed, origin) -> Contents`. It needs no worldgen except the container's biome for `location_check`, which the caller passes in.

**The oracle.** A new `cargo xtask oracle loot` run fills a stand-in container for each container table of the version, for a fixed list of seeds (1,000 a table, including `i64::MIN`, `−1`, `1` and `i64::MAX`), with the origin in a few biomes for the tables that check one. It records each slot's item, count and components as the game serializes them. The golden file stays local ([ADR 0008](../adr/0008-game-derived-test-values-stay-local.md)). The comparison is an ignored test in `spyglass-loot`. Maps are compared without their target.

## Tasks

- [ ] **Spike:** `LootTable.fill` and everything it calls, in 26.2, 26.3 and 26.4-snapshot-7: draw order, the splitter, slot shuffles, each modifier, number provider and condition above, `selectEnchantment`. Where the versions differ. Whether `reports/items.json` has the components rolling reads, in every version. Findings in the notes, reconstructions kept locally.
- [ ] **ADR:** loot in the bundle: the new directories and where item properties come from.
- [ ] **Extractor:** the directories and item properties; bundle-format.md updated.
- [ ] **`spyglass-loot`:** loading 26.2's and 26.3's forms; items; the random; pools, entries and conditions; the splitter, the slots and the shuffles.
- [ ] **Modifiers:** each one above, hand-worked tests for each, without game data.
- [ ] **Enchanting:** `enchant_with_levels`, `enchant_randomly`, `set_enchantments`, from the enchantment data.
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
- [ ] bundle-format.md: the loot, enchantment, instrument and tag directories; item properties.
- [ ] oracle.md: `oracle loot`, its golden file and comparison.
- [ ] An ADR for loot data in the bundle.

## Notes

- 2026-11-04: from the survey that wrote this plan (26.3 bytecode): `withOptionalRandomSeed` is `seed != 0 ? RandomSource.create(seed) : null`, and `create` prefers that random over the table's sequence; `RandomSource.create(long)` is `new LegacyRandomSource(seed)`. The tables are in the server jar under `data/minecraft/loot_table/`, not only in the generator's output.
