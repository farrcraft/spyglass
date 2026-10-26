# Roadmap

Milestones in order. Each has its own file with goal, scope and exit criteria. Completed milestones move to [completed/](completed/). See [development-process.md](../development-process.md) for how milestones turn into plans and code.

The milestones build toward the goals in the [README](../../README.md#goals): constraint search with ranking, visual preview, existing world saves, all three dimensions, and a library that the CLI and GUI share.

| #   | Milestone                                                                     | Delivers                                                           | Status    |
| --- | ----------------------------------------------------------------------------- | ------------------------------------------------------------------ | --------- |
| 01  | [Extractor and oracle harness](completed/01-extractor-and-oracle.md)          | Game data for 26.3 and a ground-truth reference.                   | Completed |
| 02  | [RNG, noise and biomes](completed/02-rng-noise-biomes.md)                     | Exact Overworld biomes.                                            | Completed |
| 03  | [Structure placement and locate queries](completed/03-structure-placement.md) | "Where is X in this seed?", the public API and the `spyglass` CLI. | Completed |
| 04  | [World saves](completed/04-world-saves.md)                                    | The same answers for your own worlds.                              | Completed |
| 05  | [Viewer MVP](completed/05-viewer-mvp.md)                                      | Map preview with selectable layers; image export.                  | Completed |
| 06  | [Seed search](completed/06-seed-search.md)                                    | Required and preferred constraints, ranked results.                | Completed |
| 07  | [Terrain, structure validity, jigsaw](07-terrain-and-validity.md)             | Confirmed structures, heights and exact bounds.                    | Planned   |
| 08  | [Nether, End and multiple versions](08-dimensions-and-versions.md)            | All three dimensions; more than one game version.                  | Planned   |

## Stretch

- GPU (`wgpu` compute) structure-placement search over the lower 48 bits of the seed, with CPU expansion of the top 16 bits for biome checks.

## Out of scope for now

- **Bedrock Edition:** closed-source, with a different RNG and placement.
- **Pre-1.18 versions:** a different biome system.
