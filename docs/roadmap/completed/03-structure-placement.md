# 03. Structure placement and locate queries

- Status: Completed
- Depends on: [02](02-rng-noise-biomes.md)
- Plans: [m03-placement](../../plans/completed/m03-placement.md), [m03-spawn](../../plans/completed/m03-spawn.md), [m03-api-cli](../../plans/completed/m03-api-cli.md)

## Goal

Answer "where are things in this seed?": structure positions, slime chunks and the world spawn. Also establish the public API and command-line tool that every later front end builds on.

## Scope

- `spyglass-structure`: random-spread and concentric-ring placement from extracted structure sets, followed by biome validation.
- Slime chunk test.
- World spawn point search.
- **Locate queries** for one seed: nearest structure of a type to a point, all of a type within a radius, and biome search near a point.
- **Public API:** a `spyglass` facade crate that wraps a version bundle, a seed and a dimension into one session object. Front ends use only this API.
- **Command-line tool:** `spyglass-cli` (binary `spyglass`) with `locate` subcommands, human-readable output, and `--json` for scripts.
- Oracle extended to dump structure starts and the spawn point.

## Exit criteria

- [x] Placement chunks match the oracle for the seed set, for every Overworld structure set in the bundle. The biome check matches exactly where its position doesn't depend on terrain, and exactly at the oracle's start positions everywhere; elsewhere, results are candidates that always include the game's start.
- [x] Slime chunks and the spawn origin (the chunk the game's spawn search starts from) match the oracle.
- [x] `spyglass locate` answers agree with the game's `/locate` at spot-checked positions.
- [x] A spec describes placement and documents which validity checks are deferred to [07](../07-terrain-and-validity.md).
- [x] An ADR records the public API shape and the library-first split between the API, the CLI and the GUI.
- [x] The `--json` output format has a spec, because scripts depend on it.

## Not in this milestone

- Terrain-dependent validity and jigsaw bounds ([07](../07-terrain-and-validity.md)). Results here may include starts that the game would reject on terrain grounds, and are labelled as candidates.
- The biome check at heightmap-projected positions, and the final spawn block, both of which need terrain ([07](../07-terrain-and-validity.md)).
