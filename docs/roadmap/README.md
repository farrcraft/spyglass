# Roadmap

Milestones in order. Each has its own file with goal, scope and exit criteria. Completed milestones move to [completed/](completed/). See [development-process.md](../development-process.md) for how milestones turn into plans and code.

The milestones build toward the goals in the [README](../../README.md#goals): constraint search with ranking, visual preview, existing world saves, all three dimensions, and a library that the CLI and GUI share.

| #   | Milestone                                                                     | Delivers                                                            | Status    |
| --- | ----------------------------------------------------------------------------- | ------------------------------------------------------------------- | --------- |
| 01  | [Extractor and oracle harness](completed/01-extractor-and-oracle.md)          | Game data for 26.3 and a ground-truth reference.                    | Completed |
| 02  | [RNG, noise and biomes](completed/02-rng-noise-biomes.md)                     | Exact Overworld biomes.                                             | Completed |
| 03  | [Structure placement and locate queries](completed/03-structure-placement.md) | "Where is X in this seed?", the public API and the `spyglass` CLI.  | Completed |
| 04  | [World saves](completed/04-world-saves.md)                                    | The same answers for your own worlds.                               | Completed |
| 05  | [Viewer MVP](completed/05-viewer-mvp.md)                                      | Map preview with selectable layers; image export.                   | Completed |
| 06  | [Seed search](completed/06-seed-search.md)                                    | Required and preferred constraints, ranked results.                 | Completed |
| 07  | [Terrain, structure validity, jigsaw](completed/07-terrain-and-validity.md)   | Confirmed structures, heights and exact bounds.                     | Completed |
| 08  | [Nether, End and multiple versions](completed/08-dimensions-and-versions.md)  | All three dimensions; more than one game version.                   | Completed |
| 09  | [Fast density evaluation](completed/09-fast-density.md)                       | Cheap terrain: density programs where they pay.                     | Completed |
| 10  | [Hardening](completed/10-hardening.md)                                        | Safe on hostile saves and packs; prompt stops; accurate docs.       | Completed |
| 11  | [Releases](completed/11-releases.md)                                          | Downloadable, versioned builds for Windows, Linux and macOS.        | Completed |
| 12  | [First run](completed/12-first-run.md)                                        | Game data from the viewer, beside the binaries, with no terminal.   | Completed |
| 13  | [Viewer redesign](completed/13-viewer-redesign.md)                            | A designed layout, map controls and icons, and visible loading.     | Completed |
| 14  | [Viewer performance](completed/14-viewer-performance.md)                      | A full map in less time, measured; faster rendering in the library. | Completed |
| 15  | [Nearest locate](completed/15-nearest-locate.md)                              | The nearest structure, beside `/locate`'s answer.                   | Completed |
| 16  | [Search within a seed](completed/16-search-within-a-seed.md)                  | Places in one seed that fit a query, nearest first.                 | Completed |
| 17  | [Markers and saved seeds](completed/17-markers-and-saved-seeds.md)            | Your own pins, done structures, saved seeds and shared views.       | Completed |
| 18  | [Surface rules, ore veins, caves, carvers](18-surface-veins-and-carvers.md)   | A chunk's blocks before features, and layers to show them.          | Planned   |
| 19  | [Features](19-features.md)                                                    | Dungeons, geodes, wells, fossils and lava lakes, certain or not.    | Planned   |
| 20  | [Loot](20-loot.md)                                                            | What's in the chests, and searching for it.                         | Planned   |

## Follow-ups

Open items from completed milestones, too small for a plan:

- **08:** each new 26.4 snapshot, pre-release, release candidate and the release, added by [the guide](../adding-a-version.md). The first, likely 26.4-snapshot-7, is the first addition from the guide as written (exit criterion 3).
- **11:** the first-run guide followed on a clean macOS and Linux machine (exit criterion 2).
- **11:** reproducibility on Linux and macOS: two builds of one commit compared, the result recorded in [ADR 0038](../adr/0038-how-release-archives-are-built.md) (exit criterion 3).

## Not yet planned

Work that completed milestones deferred, waiting for a milestone of its own:

- **The final spawn block:** needs generated blocks (the surface and features), not only the generator's heights ([spawn.md](../specs/spawn.md)). Surface rules are now [18](18-surface-veins-and-carvers.md)'s and features [19](19-features.md)'s; what's left is for after them.
- **More game versions:** Chunkbase covers Java back to 1.7 and Bedrock. Older 1.18+ versions are a matter of [the guide](../adding-a-version.md) and version switches; pre-1.18 and Bedrock stay out of scope (below).

## Stretch

- GPU (`wgpu` compute) structure-placement search over the lower 48 bits of the seed, with CPU expansion of the top 16 bits for biome checks.

## Out of scope for now

- **Bedrock Edition:** closed-source, with a different RNG and placement.
- **Pre-1.18 versions:** a different biome system.
