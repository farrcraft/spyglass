# 0059. Loot data in the bundle, with item components from the data generator

- Status: Proposed
- Date: 2026-11-04
- See also: [0006](0006-bundle-format-raw-json-tree.md), [0054](0054-block-behaviour-from-the-running-game.md)

## Context

Rolling a container's loot ([loot.md](../specs/loot.md)) reads data no bundle has yet: the loot tables themselves, the enchantments `enchant_randomly` and `enchant_with_levels` choose from, the instruments `set_instrument` picks, the item, enchantment and instrument tags those name, and three properties of each item. Splitting a stack needs its maximum stack size, enchanting its enchantability, and `set_damage` its maximum damage. All of them change between versions: 26.3 and 26.4-snapshot-7 add items and tables, and 26.3 reshaped the tables' JSON. So by "algorithms in code, parameters in data" they belong in the bundle.

The tables, enchantments, instruments and tags are JSON files in the server jar, as worldgen's are. The item properties aren't in any data file: they're item components, set in the game's code (`Items`). The data generator's `--reports`, which the extractor already runs, writes each item's default components to `reports/minecraft/components/item/<id>.json` in every supported version (26.2's 1,537 items, 26.3's 1,658, 26.4-snapshot-7's 1,686). `reports/items.json`, which older versions had, doesn't exist in any of them.

## Decision

We will copy into every bundle, byte for byte as the format requires ([ADR 0006](0006-bundle-format-raw-json-tree.md)):

- from the jar, `data/minecraft/loot_table/`, `enchantment/`, `instrument/`, `tags/item/`, `tags/enchantment/` and `tags/instrument/`, whole;
- from the generator's reports, `reports/minecraft/components/item/`.

`format` stays 1: adding directories is how bundles grow ([bundle-format.md](../specs/bundle-format.md#layout)). Loaders that need them, and bundles extracted before them, ask for the bundle to be extracted again.

## Consequences

- Loot runs on the game's own data in every version, and a new version's tables and items need no change in spyglass unless their shapes do.
- A bundle grows by about 2.3 MB (26.3: loot tables 1.06 MB, item components 1.16 MB, the rest 0.11 MB), against 26 MB now.
- Every loot table is copied, not only the containers': blocks', entities' and the rest add about 0.9 MB that loot rolling doesn't read. A `loot_table` entry may name any table, and a data pack's container may use any, so a filter would be a list to keep right in every version.
- The item components are a report, so the extractor gains a copied directory, not code. The report holds every default component (about 700 bytes an item), most of them unused here.
- Bundles extracted before this need extracting again before loot works. The viewer and the CLI already say so for missing files.

## Alternatives considered

- **An item part in the extractor's block report** ([ADR 0054](0054-block-behaviour-from-the-running-game.md)): the dumper would read `Item.components()` by reflection and write only the three properties, about 60 KB. It's code to keep working in every version, for data the generator already writes.
- **Only the container tables** (`chests/`, `barrels/`, `pots/`, `dispensers/`): saves 0.9 MB, but a nested `loot_table` entry or a data pack can reach any table, and which directories hold containers' tables is itself version data.
- **A Rust table of item properties:** version data in code, to be redone for each version's new items.
