# 0050. A chunk's blocks in a crate of their own

- Status: Accepted
- Date: 2026-10-29

## Context

Milestone 18 computes a chunk's blocks before features: the noise fill, the material rule (surface blocks, ore veins, icebergs, badlands pillars) and the carvers. Each part needs something from crates that don't depend on each other:

- **biomes,** for the material rule's biome and temperature conditions and for which carvers run, from `spyglass-biome`;
- **terrain columns and the aquifer,** for the noise fill and the fluids of carved positions, from `spyglass-terrain`;
- **density functions,** for the preliminary surface, the noise conditions and the ore-vein inputs, from `spyglass-density`.

`spyglass-terrain` and `spyglass-biome` sit side by side, above density. `spyglass-structure` already sits above both. It uses terrain for heights and biomes for placement.

## Decision

We will put blocks in a new crate, `spyglass-blocks`, beside `spyglass-structure`. It depends on `spyglass-terrain`, `spyglass-biome`, `spyglass-density`, `spyglass-noise`, `spyglass-rng` and `spyglass-data`. `spyglass` depends on it, and the front ends reach it only through `spyglass`.

It holds:

- block states and a chunk's storage, with the heightmap generation keeps up to date;
- the noise fill of whole chunks, from terrain's columns;
- the material rule (26.3 and 26.4) and 26.2's surface rules, with their data loading;
- the biome lookups blocks need: the biome manager's zoom and biome temperature;
- the carvers, when they come.

## Consequences

- The dependency direction stays one way: rng, noise, density, then biome and terrain, then structure and blocks, then `spyglass`.
- **Structures can't use blocks yet.** A feature or structure check that needs real blocks (milestone 19) would have to depend on `spyglass-blocks`. That's allowed, since both sit at the same level and blocks doesn't depend on structures.
- **The zoom and biome temperature live here,** not in `spyglass-biome`. They're only needed for blocks so far. If biome queries need them later, they can move down, a change inside the workspace.

## Alternatives considered

- **In `spyglass-terrain`:** terrain would depend on `spyglass-biome`, which it doesn't today. Heights for structure placement would then pull in biomes for nothing.
- **In `spyglass-biome`:** the same problem the other way round. Biome lookups would pull in terrain and the aquifer.
- **In `spyglass`:** worldgen algorithms belong in the library crates below the public API, and `spyglass` is the API.
