# 0052. Features in a crate of their own

- Status: Accepted
- Date: 2026-10-30

## Context

Milestone 19 places features (dungeons, geodes, desert wells, fossils, lava lakes) the way the game decorates a chunk. That needs:

- **a chunk's blocks before features,** since a feature checks and changes them, from `spyglass-blocks`;
- **biomes,** for which features run in a chunk and each try's biome filter, from `spyglass-biome`;
- **the generator's heights and anchors,** for placement modifiers, from `spyglass-terrain` and `spyglass-data`;
- **a new random,** `WorldgenRandom` over Xoroshiro, with the decoration and feature seeds, from `spyglass-rng`;
- **structure pieces** placed in the same steps (mineshafts before dungeons), which may come from `spyglass-structure` later.

Decoration is its own body of code: the feature order every biome's lists sort into, the placement modifiers, each feature's algorithm, and which answers are certain. It's about as big as the material rule and carvers together.

## Decision

We will put decoration in a new crate, `spyglass-feature`, above `spyglass-blocks`. It depends on `spyglass-blocks`, `spyglass-biome`, `spyglass-terrain`, `spyglass-rng` and `spyglass-data`, and may depend on `spyglass-structure` for structure pieces. `spyglass` depends on it, and the front ends reach it only through `spyglass`.

It holds placed and configured features and their data loading, the feature order, the decoration and feature seeds' use, the placement modifiers, block predicates, the features themselves, and whether each answer is certain.

## Consequences

- The dependency direction stays one way: rng, noise, density, then biome and terrain, then structure and blocks, then features, then `spyglass`.
- `spyglass-blocks` stays about a chunk before features, and doesn't grow with each feature.
- **Features can use structures, not the other way round.** A structure check that needs features would have to move up. None does today.
- One more crate to build, lint and publish metadata for.

## Alternatives considered

- **In `spyglass-blocks`:** blocks would grow by a large, separate body of code, and would need structures for the pieces placed before features, which it doesn't depend on today.
- **In `spyglass-structure`:** structure placement would then depend on blocks, which locate and the seed search use without them.
- **In `spyglass`:** worldgen algorithms belong in the library crates below the public API.
