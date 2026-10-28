# Roadmap

Milestones in order. Each has its own file with goal, scope and exit criteria. Completed milestones move to [completed/](completed/). See [development-process.md](../development-process.md) for how milestones turn into plans and code.

The milestones build toward the goals in the [README](../../README.md#goals): constraint search with ranking, visual preview, existing world saves, all three dimensions, and a library that the CLI and GUI share.

| #   | Milestone                                                                     | Delivers                                                           | Status      |
| --- | ----------------------------------------------------------------------------- | ------------------------------------------------------------------ | ----------- |
| 01  | [Extractor and oracle harness](completed/01-extractor-and-oracle.md)          | Game data for 26.3 and a ground-truth reference.                   | Completed   |
| 02  | [RNG, noise and biomes](completed/02-rng-noise-biomes.md)                     | Exact Overworld biomes.                                            | Completed   |
| 03  | [Structure placement and locate queries](completed/03-structure-placement.md) | "Where is X in this seed?", the public API and the `spyglass` CLI. | Completed   |
| 04  | [World saves](completed/04-world-saves.md)                                    | The same answers for your own worlds.                              | Completed   |
| 05  | [Viewer MVP](completed/05-viewer-mvp.md)                                      | Map preview with selectable layers; image export.                  | Completed   |
| 06  | [Seed search](completed/06-seed-search.md)                                    | Required and preferred constraints, ranked results.                | Completed   |
| 07  | [Terrain, structure validity, jigsaw](completed/07-terrain-and-validity.md)   | Confirmed structures, heights and exact bounds.                    | Completed   |
| 08  | [Nether, End and multiple versions](completed/08-dimensions-and-versions.md)  | All three dimensions; more than one game version.                  | Completed   |
| 09  | [Fast density evaluation](completed/09-fast-density.md)                       | Cheap terrain: density programs where they pay.                    | Completed   |
| 10  | [Hardening](10-hardening.md)                                                  | Safe on hostile saves and packs; prompt stops; accurate docs.      | In progress |

## Follow-ups

Open items from completed milestones, too small for a plan:

- **08:** the first green CI run on GitHub, once the repository is published there (exit criterion 2). Every supported version passes `cargo xtask ci` locally.
- **08:** each new 26.4 snapshot, pre-release, release candidate and the release, added by [the guide](../adding-a-version.md). The first, likely 26.4-snapshot-7, is the first addition from the guide as written (exit criterion 3).

## Not yet planned

Work that completed milestones deferred, waiting for a milestone of its own:

- **Surface rules** (in 26.3, the material rule): the blocks that replace stone at the surface. Deferred by 07; no height query needs them.
- **Features:** decoration order and per-feature randomness over placed and configured features. Deferred by 07.
- **The final spawn block:** needs generated blocks (the surface and features), not only the generator's heights ([spawn.md](../specs/spawn.md)).

## Stretch

- GPU (`wgpu` compute) structure-placement search over the lower 48 bits of the seed, with CPU expansion of the top 16 bits for biome checks.

## Out of scope for now

- **Bedrock Edition:** closed-source, with a different RNG and placement.
- **Pre-1.18 versions:** a different biome system.
