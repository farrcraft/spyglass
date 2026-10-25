# 03. Structure placement, slime chunks, spawn point

- Status: Planned
- Depends on: [02](02-rng-noise-biomes.md)
- Plans: none yet

## Goal

Answer "where could structures generate?" for a seed, plus slime chunks and the world spawn point.

## Scope

- `spyglass-structure`: random-spread and concentric-ring placement from extracted structure sets, followed by biome validation.
- Slime chunk test.
- World spawn point search.
- Oracle extended to dump structure starts and the spawn point.

## Exit criteria

- [ ] Structure starts that pass the biome check match the oracle for the seed set, for every Overworld structure set in the bundle.
- [ ] Slime chunks and spawn point match the oracle.
- [ ] A spec describes placement and documents which validity checks are deferred to [06](06-terrain-and-validity.md).

## Not in this milestone

- Terrain-dependent validity and jigsaw bounds ([06](06-terrain-and-validity.md)). Results here may include starts that the game would reject on terrain grounds.
