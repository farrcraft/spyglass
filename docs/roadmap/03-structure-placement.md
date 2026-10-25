# 03. Structure placement and locate queries

- Status: Planned
- Depends on: [02](completed/02-rng-noise-biomes.md)
- Plans: none yet

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

- [ ] Structure starts that pass the biome check match the oracle for the seed set, for every Overworld structure set in the bundle.
- [ ] Slime chunks and spawn point match the oracle.
- [ ] `spyglass locate` answers agree with the game's `/locate` at spot-checked positions.
- [ ] A spec describes placement and documents which validity checks are deferred to [07](07-terrain-and-validity.md).
- [ ] An ADR records the public API shape and the library-first split between the API, the CLI and the GUI.
- [ ] The `--json` output format has a spec, because scripts depend on it.

## Not in this milestone

- Terrain-dependent validity and jigsaw bounds ([07](07-terrain-and-validity.md)). Results here may include starts that the game would reject on terrain grounds, and are labelled as candidates.
