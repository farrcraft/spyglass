# Specs

Behavior spyglass must match. Update a spec in the same change as the code that implements or alters it.

A spec is **Stable** when the behavior it describes is implemented and checked: against the game through the [oracle](oracle.md) for worldgen, by tests otherwise. A stable spec can still grow in scope; its status line says what it covers so far. A spec is **Draft** while that behavior is still being built or isn't checked yet.

| Spec                                             | Covers                                                            | Status |
| ------------------------------------------------ | ----------------------------------------------------------------- | ------ |
| [data-sources.md](data-sources.md)               | Where game data comes from, and the extractor.                    | Stable |
| [oracle.md](oracle.md)                           | The ground-truth harness: probes, golden files, `/locate` checks. | Stable |
| [bundle-format.md](bundle-format.md)             | Version bundles on disk; NBT.                                     | Stable |
| [rng.md](rng.md)                                 | The game's random sources.                                        | Stable |
| [noise.md](noise.md)                             | Perlin, octave and normal noise.                                  | Stable |
| [density-functions.md](density-functions.md)     | Density-function graphs, for the Overworld's climate.             | Stable |
| [biome-lookup.md](biome-lookup.md)               | Overworld biomes at quart resolution.                             | Stable |
| [structure-placement.md](structure-placement.md) | Overworld structure placement and biome checks.                   | Stable |
| [spawn.md](spawn.md)                             | The world spawn's origin.                                         | Stable |
| [locate.md](locate.md)                           | `/locate structure` and `/locate biome`.                          | Stable |
| [cli-json.md](cli-json.md)                       | The `spyglass` command's JSON output.                             | Stable |
| [world-saves.md](world-saves.md)                 | Reading world saves and judging whether spyglass can answer.      | Stable |
| [render.md](render.md)                           | Map images: sampling, colours, layers.                            | Draft  |
