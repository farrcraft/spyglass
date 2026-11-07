# m20. Loot queries

- Status: In progress
- Milestone: [20. Loot](../roadmap/20-loot.md)
- Started: 2026-11-06
- Completed:

## Goal

Loot in every front end: a container's contents, the containers in an area holding an item, and the nearest one, in the API and as `spyglass loot`; contents and an item highlight in the viewer; loot as a constraint in both searches. It meets the milestone's second exit criterion, and builds on [m20-tables](completed/m20-tables.md), [m20-containers](completed/m20-containers.md) and [m20-layouts](completed/m20-layouts.md).

## Approach

**The API** (`spyglass`), beside the feature queries ([locate.md](../specs/locate.md#features)):

- `Session::containers_in(area)`: each container with its position, kind (chest, barrel, minecart, dispenser, pot), structure or feature, table, certainty and reason, and its contents when its seed is known.
- `Session::nearest_container(item, from)`: the nearest container whose contents hold an item (or a tag, `#minecraft:...`), searched ring by ring as `nearest_feature` is, with an optional enchantment (`minecraft:enchanted_book` with `mending`).
- An item's components in the contents: count, enchantments with levels, damage, potion, name, a map's destination.

Each answer is certain or a candidate, as structures and features are. A container that's certain but whose seed isn't known has no contents and says so.

**The CLI:** `spyglass loot at <x> <y> <z>`, `spyglass loot in <area>` and `spyglass loot find <item>` (nearest), with `--json`, on a seed or a world save. For a save, a container already opened or a chunk already generated may hold something else, so a save's answer says the seed's contents, not the chest's now.

**The viewer:** a selected structure's or dungeon's containers, listed with their contents and drawn as small markers up close; a **Loot** section in the Layers tab that rings the containers holding a chosen item. Locate gains **Nearest loot**.

**Search:** `loot I` counts containers holding item `I` in a place, in [16](../roadmap/completed/16-search-within-a-seed.md)'s `find` and in the seed search: `require loot minecraft:enchanted_golden_apple within 1000 of spawn`. Containers need the structure's chunks decorated, so the search runs them last, after cheaper constraints have ruled seeds out, as features do ([ADR 0057](../adr/0057-features-in-the-search-language.md)). Its cost is measured; the target is the milestone's question, an enchanted golden apple within 1,000 blocks of spawn, at a useful rate.

## Tasks

- [x] **API:** `containers_in`, `nearest_container`, item and tag matching, components in the answers; locate.md. _`Session::containers_in`, `Session::nearest_container`, `World::item_query`; `Loot::possible_items` rules out containers whose table can't hold the item, and answers seedless candidates that could. `containers_in_are_the_game_s` finds no differences in 26.2, 26.3 and 26.4-snapshot-5 to 7 (26.3: 538 of the game's containers, 13 in unlikely dungeons, the rest answered; 440 certain answers, all the game's). About 0.1 s a placed chunk in 26.3, mostly carving; 26.2's comparison takes about 5.5 minutes a seed, so CI's container units grew._
- [x] **CLI:** `spyglass loot` with `--json`, on seeds and saves; the CLI's tests. _`loot at x,y,z` (the 3 × 3 chunks around it, as a dungeon's chest can be beside its chunk), `loot in --min --max [--item]` (at most 1,024 chunks), `loot find <item> [--enchantment --level]`; cli-json.md and getting-started. `loot_answers_are_the_library_s` checks all three against the library by seed -1's mansion in 26.2, 26.3 and 26.4-snapshot-7; `a_default_world` checks a save's answer is its seed's. `loot find` over 16 chunks takes about a minute in 26.3._
- [x] **Viewer:** contents of a selected structure's containers; the item layer; Nearest loot. Checked by eye on a real seed. _The viewer has no selected structure, so the Layers tab's **Loot** section marks every container up close (**Containers**) and rings those holding an item; hovering one lists its contents, and clicking opens them slot by slot. **Nearest** the item is in the map's menu, and exports and `spyglass render --loot` draw the rings (`Layers::loot`, `Session::containers_holding`, which skips dungeons for an item their table can't hold). Each tile places its chunks and one more on every side, as a dungeon's chest can be beside its chunk. Checked by eye on seed -1 in 26.3 at its woodland mansion: the two chests with a diamond hoe ringed, one opened with its 11 stacks, and Nearest diamond hoe finding none within 8 chunks from 4301, −3533, as neither is._
- [ ] **Search:** `loot` in both searches, its parser, prefilter and order; search.md.
- [ ] **Cost:** containers for a seed in `cargo xtask bench`; a seed search for an enchanted golden apple near spawn, timed.
- [ ] **Changelog** entries.
- [ ] `cargo xtask check` and `cargo xtask oracle-test`. Complete the plan, then the milestone.

## Verification

- `spyglass loot find minecraft:enchanted_golden_apple` on a golden seed gives a container the containers comparison confirms.
- The viewer shows a chest's contents and rings the chests holding a chosen item on a real seed (the second exit criterion), screenshotted.
- A seed search for loot returns seeds whose containers the game confirms (an ignored oracle test).

## Durable outputs

- [x] locate.md: loot queries.
- [ ] search.md: `loot` constraints.
- [x] render.md: the loot markers and layer.
- [ ] An ADR if `loot` in the search language needs a decision of its own.

## Notes
