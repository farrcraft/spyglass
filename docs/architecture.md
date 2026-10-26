# Architecture

## Code vs data

spyglass separates what the game _does_ (algorithms, written in Rust) from the _parameters_ it does it with (extracted per version from the game jar). Algorithms change rarely between versions; parameters change constantly.

| Layer      | Code (Rust)                                                             | Data (extracted per version)                                    |
| ---------- | ----------------------------------------------------------------------- | --------------------------------------------------------------- |
| RNG        | Java LCG, Xoroshiro128++, positional random factories, MD5 name seeding | —                                                               |
| Noise      | Perlin (`f32` samples), noise stacks, normal noise                      | Noise parameters                                                |
| Density    | Graph interpreter (later: flattened, SIMD-friendly evaluation)          | Density-function graphs, noise settings                         |
| Biomes     | Climate sampler, exact nearest-point search that reports ties           | Biome parameter list                                            |
| Structures | Random-spread and concentric-ring placement, validity checks, jigsaw    | Structure sets, biome tags, template pools, structure templates |
| Features   | Decoration order, per-feature RNG                                       | Placed/configured features                                      |

## Crates

| Crate                | Role                                                                                                                                                                                                                        | Depends on                      |
| -------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------- |
| `spyglass-rng`       | Bit-exact RNGs. Small, but every bug here poisons everything downstream.                                                                                                                                                    | —                               |
| `spyglass-noise`     | Noise primitives.                                                                                                                                                                                                           | rng                             |
| `spyglass-data`      | Reads version bundles; the JSON codec helpers every loader shares (`LoadError`, JSON paths, numbers as the game parses them); NBT. No network or archive code ([ADR 0016](adr/0016-extractor-crate-and-read-only-data.md)). | —                               |
| `spyglass-oracle`    | Test support: reads the oracle's golden files, and builds small hand-written bundles (`fixtures`) for tests that need no game data. A dev-dependency only, never a normal one.                                              | data                            |
| `spyglass-density`   | Loads and evaluates density-function graphs.                                                                                                                                                                                | noise, data                     |
| `spyglass-biome`     | Climate sampling and biome lookup at quart (1:4) resolution; Overworld, Nether, End. The spawn's climate search; `/locate biome`.                                                                                           | rng, noise, density, data       |
| `spyglass-terrain`   | _Planned._ Heightmaps and surface rules.                                                                                                                                                                                    | density, data                   |
| `spyglass-structure` | Placement (seed-only, cheap), then biome validation, then terrain-dependent validity, with optional jigsaw for exact bounds; `/locate structure`.                                                                           | rng, biome, data; later terrain |
| `spyglass-search`    | _Planned._ Parallel seed search (`rayon`) with required and preferred constraints, ranked results, checks ordered cheapest first.                                                                                           | rng, biome, structure           |
| `spyglass-world`     | Reads local world saves: seed, version, generators, data packs, spawn and player; chunk versions from region files; launcher folders.                                                                                       | data                            |
| `spyglass`           | The public API: a `World` (one dimension of a bundle) and seed-bound `Session`s with locate queries and map images ([ADR 0017](adr/0017-map-rendering-in-the-public-api.md)), and world saves; later search.                | biome, structure, world, data   |
| `spyglass-cli`       | The `spyglass` command: locate, world info and render; search later. Human-readable output, or `--json` for scripts.                                                                                                        | spyglass                        |
| `spyglass-viewer`    | Native map app (`egui` + `wgpu`, [ADR 0019](adr/0019-gui-stack-and-msrv-1-95.md)): tiles rendered in the background, zoom levels, selectable layers, locate from the map, saves, export.                                    | spyglass                        |

A crate may depend only on crates listed above it in this table, never on one below it. Planned crates are created in the milestone that needs them. Front ends depend only on `spyglass` ([ADR 0014](adr/0014-public-api-world-and-session.md)).

## Front ends

spyglass is library-first. Everything a user can do goes through the `spyglass` crate's public API, and every front end is a thin layer over it:

- **Rust library:** depend on `spyglass` directly.
- **Command line and scripts:** `spyglass` (from `spyglass-cli`). Every command can emit JSON ([specs/cli-json.md](specs/cli-json.md)), so shell scripts and other languages can drive it.
- **GUI:** `spyglass-viewer`.

Nothing in the CLI or viewer computes worldgen results itself. If the viewer can show it, the API can return it, so scripts can do anything the GUI can.

`spyglass-extract` builds bundles: the version manifest, the jar cache, Java discovery, the data generator and the `spyglass-extract` command. It depends on `spyglass-data`, and nothing but `xtask` depends on it ([ADR 0016](adr/0016-extractor-crate-and-read-only-data.md)).

`xtask/` (project commands, [ADR 0003](adr/0003-cargo-xtask-for-project-commands.md)) is tooling, not product. It may depend on these crates; none of them may depend on it.

## Correctness

Output is checked against the real game through the differential oracle. See [specs/oracle.md](specs/oracle.md).
