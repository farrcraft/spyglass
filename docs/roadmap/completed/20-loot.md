# 20. Loot

- Status: Completed
- Depends on: [07](07-terrain-and-validity.md), [19](19-features.md)
- Plans, in order: [m20-tables](../../plans/completed/m20-tables.md), [m20-containers](../../plans/completed/m20-containers.md), [m20-layouts](../../plans/completed/m20-layouts.md), [m20-queries](../../plans/completed/m20-queries.md)

## Goal

What's in the chests: the loot of a structure's or feature's containers, exactly as the game rolls it, so "an enchanted golden apple within 1,000 blocks of spawn" is a question spyglass can answer and search for.

## Scope

- **Containers:** where each chest, barrel and other loot container is placed, with its loot table and loot seed. This covers structure pieces (from jigsaw and the other piece generators [07](07-terrain-and-validity.md) assembled) and features (dungeons, from [19](19-features.md)).
- **Loot tables:** pools, rolls, entries, conditions and functions (enchanting, set count, and the rest vanilla uses), as data from the bundle with the algorithms in code.
- **Queries:** a container's contents, containers in an area holding an item, and the nearest such container, in the API and as `spyglass loot` with `--json`.
- **The viewer:** a container's contents when its structure is selected, and an item highlight layer ("chests with an enchanted golden apple").
- **Search:** loot as a constraint in both searches.
- **The oracle:** golden chest contents from the game, for the golden seeds' nearest structures of each kind, in every supported version.
- **Specs:** a loot spec, with per-version notes.

Not in scope:

- Loot rolled only when a player first opens a container: the luck stat, and tables keyed to the player.
- Mob drops, fishing, bartering and archaeology.

## Exit criteria

- [x] Every golden chest's contents match the game's exactly, for every supported version.
- [x] The viewer shows a chest's contents, and highlights the chests holding a chosen item, on a real seed.
- [x] `cargo xtask check` and `cargo xtask oracle-test` pass.
