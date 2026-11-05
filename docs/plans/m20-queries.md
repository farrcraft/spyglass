# m20. Loot queries

- Status: Draft
- Milestone: [20. Loot](../roadmap/20-loot.md)
- Started:
- Completed:

## Goal

Loot in every front end: a container's contents, the containers in an area holding an item, and the nearest one, in the API and as `spyglass loot`; contents and an item highlight in the viewer; loot as a constraint in both searches. It meets the milestone's second exit criterion, and builds on [m20-tables](m20-tables.md) and [m20-containers](m20-containers.md).

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

- [ ] **API:** `containers_in`, `nearest_container`, item and tag matching, components in the answers; locate.md.
- [ ] **CLI:** `spyglass loot` with `--json`, on seeds and saves; the CLI's tests.
- [ ] **Viewer:** contents of a selected structure's containers; the item layer; Nearest loot. Checked by eye on a real seed.
- [ ] **Search:** `loot` in both searches, its parser, prefilter and order; search.md.
- [ ] **Cost:** containers for a seed in `cargo xtask bench`; a seed search for an enchanted golden apple near spawn, timed.
- [ ] **Changelog** entries.
- [ ] `cargo xtask check` and `cargo xtask oracle-test`. Complete the plan, then the milestone.

## Verification

- `spyglass loot find minecraft:enchanted_golden_apple` on a golden seed gives a container the containers comparison confirms.
- The viewer shows a chest's contents and rings the chests holding a chosen item on a real seed (the second exit criterion), screenshotted.
- A seed search for loot returns seeds whose containers the game confirms (an ignored oracle test).

## Durable outputs

- [ ] locate.md: loot queries.
- [ ] search.md: `loot` constraints.
- [ ] render.md: the loot markers and layer.
- [ ] An ADR if `loot` in the search language needs a decision of its own.

## Notes
