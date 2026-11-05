# 0060. Loot in a crate of its own, below worldgen

- Status: Proposed
- Date: 2026-11-04
- See also: [0052](0052-features-in-their-own-crate.md), [0059](0059-loot-data-in-the-bundle.md)

## Context

Filling a container from its loot table and seed ([loot.md](../specs/loot.md)) is its own body of code: the tables' two forms (26.2's and 26.3's), pools, entries, number providers, conditions, a dozen modifiers, enchanting with its costs and weighted picks, the stack splitter and the slot shuffles. It reads loot tables, enchantments, instruments, tags and item components from the bundle, and draws from a `LegacyRandomSource`.

It needs almost nothing from worldgen. Only two of its parts reach the world: `location_check` reads the biome at the container's block, and `exploration_map` searches for a structure. Both are answers about one position, which whoever placed the container can give. The containers themselves (where they are, their tables and seeds) come from features and structure pieces, in `spyglass-feature` and `spyglass-structure`.

## Decision

We will put loot in a new crate, `spyglass-loot`, depending only on `spyglass-data` (the bundle, tags) and `spyglass-rng` (the random). It holds loading the tables, enchantments, instruments and item components, and filling a container from a table, a seed and the container's position. The biome at the container is passed in; a map's destination is returned for the caller to locate. `spyglass` depends on it and joins containers to their contents. The front ends reach it only through `spyglass`.

## Consequences

- Loot builds and tests without any worldgen crate, and its oracle comparison needs no world either.
- The features and structures crates don't depend on it: they report a container's position, table and seed, and `spyglass` fills it. A feature's check that needed a container's contents would have to move up; none does.
- A caller that fills containers must give each one's zoomed biome (for `location_check`) and locate its maps. `spyglass` has both.
- One more crate to build, lint and publish metadata for.

## Alternatives considered

- **Loot inside `spyglass-feature`:** dungeons are its only containers, while most containers are structure pieces'. It would make loot depend on the whole worldgen stack for no part it uses.
- **Loot inside `spyglass`:** the top crate would hold a whole algorithm family, against "library crates per layer", and the bench and tests would build everything to test filling.
- **Loot depending on `spyglass-biome`** to read `location_check`'s biome itself: it would need the seed's biome source and the zoom, which only the caller has set up.
