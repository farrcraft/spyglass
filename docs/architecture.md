# Architecture

## Code vs data

spyglass separates what the game _does_ (algorithms, written in Rust) from the _parameters_ it does it with (extracted per version from the game jar). Algorithms change rarely between versions; parameters change constantly.

| Layer      | Code (Rust)                                                             | Data (extracted per version)               |
| ---------- | ----------------------------------------------------------------------- | ------------------------------------------ |
| RNG        | Java LCG, Xoroshiro128++, positional random factories, MD5 name seeding | —                                          |
| Noise      | Perlin/improved, octave, normal noise                                   | Noise parameters                           |
| Density    | Graph interpreter (later: flattened, SIMD-friendly evaluation)          | Density-function graphs, noise settings    |
| Biomes     | Climate sampler, nearest-biome search tree                              | Biome parameter list                       |
| Structures | Random-spread and concentric-ring placement, validity checks, jigsaw    | Structure sets, biome tags, template pools |
| Features   | Decoration order, per-feature RNG                                       | Placed/configured features                 |

## Crates

| Crate                | Role                                                                                                                         | Depends on                |
| -------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ------------------------- |
| `spyglass-rng`       | Bit-exact RNGs. Small, but every bug here poisons everything downstream.                                                     | —                         |
| `spyglass-noise`     | Noise primitives.                                                                                                            | rng                       |
| `spyglass-data`      | Version bundles, the manifest client and the extractor CLI.                                                                  | —                         |
| `spyglass-density`   | Loads and evaluates density-function graphs.                                                                                 | noise, data               |
| `spyglass-biome`     | Climate sampling and biome lookup at quart (1:4) resolution; Overworld, Nether, End.                                         | density, data             |
| `spyglass-terrain`   | Heightmaps and surface rules.                                                                                                | density, data             |
| `spyglass-structure` | Placement (seed-only, cheap), then biome validation, then terrain-dependent validity, with optional jigsaw for exact bounds. | rng, biome, terrain, data |
| `spyglass-search`    | Parallel seed search (`rayon`) with required and preferred constraints, ranked results, checks ordered cheapest first.       | rng, biome, structure     |
| `spyglass-world`     | _Planned._ Reads local world saves: `level.dat` seed, version, preset and datapacks.                                         | data                      |
| `spyglass`           | _Planned._ The public API: a session (bundle + seed + dimension, or a world save) with locate, render and search queries.    | everything above          |
| `spyglass-cli`       | _Planned._ The `spyglass` command: locate, world info, render, search. Human-readable output, or `--json` for scripts.       | spyglass                  |
| `spyglass-viewer`    | Native map app (`egui` + `wgpu`): tiled on-demand rendering, zoom levels, selectable layers.                                 | spyglass                  |

Dependencies only point down this table. A lower crate must never depend on a higher one. Planned crates are added in the milestone that needs them. Until `spyglass` exists, the viewer depends on the lower crates directly.

## Front ends

spyglass is library-first. Everything a user can do goes through the `spyglass` crate's public API, and every front end is a thin layer over it:

- **Rust library:** depend on `spyglass` directly.
- **Command line and scripts:** `spyglass` (from `spyglass-cli`). Every command can emit JSON, so shell scripts and other languages can drive it.
- **GUI:** `spyglass-viewer`.

Nothing in the CLI or viewer computes worldgen results itself. If the viewer can show it, the API can return it, so scripts can do anything the GUI can.

`xtask/` (project commands, [ADR 0003](adr/0003-cargo-xtask-for-project-commands.md)) is tooling, not product. It may depend on these crates; none of them may depend on it.

## Correctness

Output is checked against the real game through the differential oracle. See [specs/oracle.md](specs/oracle.md).
