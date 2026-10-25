# Roadmap

Milestones in order. Each has its own file with goal, scope and exit criteria. Completed milestones move to [completed/](completed/). See [development-process.md](../development-process.md) for how milestones turn into plans and code.

| #   | Milestone                                                                   | Status  |
| --- | --------------------------------------------------------------------------- | ------- |
| 01  | [Extractor and oracle harness](01-extractor-and-oracle.md)                  | Planned |
| 02  | [RNG, noise and biomes](02-rng-noise-biomes.md)                             | Planned |
| 03  | [Structure placement, slime chunks, spawn point](03-structure-placement.md) | Planned |
| 04  | [Viewer MVP](04-viewer-mvp.md)                                              | Planned |
| 05  | [Seed search](05-seed-search.md)                                            | Planned |
| 06  | [Terrain, structure validity, jigsaw](06-terrain-and-validity.md)           | Planned |
| 07  | [Nether, End and multiple versions](07-dimensions-and-versions.md)          | Planned |

## Stretch

- GPU (`wgpu` compute) structure-placement search over the lower 48 bits of the seed, with CPU expansion of the top 16 bits for biome checks.

## Out of scope for now

- **Bedrock Edition:** closed-source, with a different RNG and placement.
- **Pre-1.18 versions:** a different biome system.
