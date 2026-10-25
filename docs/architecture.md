# Architecture

## Code vs data

spyglass separates what the game *does* (algorithms, written in Rust) from the *parameters* it does it with (extracted per version from the game jar). Algorithms change rarely between versions; parameters change constantly.

| Layer | Code (Rust) | Data (extracted per version) |
|---|---|---|
| RNG | Java LCG, Xoroshiro128++, positional random factories, MD5 name seeding | — |
| Noise | Perlin/improved, octave, normal noise | Noise parameters |
| Density | Graph interpreter (later: flattened, SIMD-friendly evaluation) | Density-function graphs, noise settings |
| Biomes | Climate sampler, nearest-biome search tree | Biome parameter list |
| Structures | Random-spread and concentric-ring placement, validity checks, jigsaw | Structure sets, biome tags, template pools |
| Features | Decoration order, per-feature RNG | Placed/configured features |

## Crates

| Crate | Role | Depends on |
|---|---|---|
| `spyglass-rng` | Bit-exact RNGs. Small, but every bug here poisons everything downstream. | — |
| `spyglass-noise` | Noise primitives. | rng |
| `spyglass-data` | Version bundles, the manifest client and the extractor CLI. | — |
| `spyglass-density` | Loads and evaluates density-function graphs. | noise, data |
| `spyglass-biome` | Climate sampling and biome lookup at quart (1:4) resolution; Overworld, Nether, End. | density, data |
| `spyglass-terrain` | Heightmaps and surface rules. | density, data |
| `spyglass-structure` | Placement (seed-only, cheap), then biome validation, then terrain-dependent validity, with optional jigsaw for exact bounds. | rng, biome, terrain, data |
| `spyglass-search` | Parallel seed search (`rayon`), filters ordered cheapest first. | rng, biome, structure |
| `spyglass-viewer` | Native map app (`egui` + `wgpu`): tiled on-demand rendering, zoom levels, overlay layers. | biome, structure, data |

Dependencies only point down this table. A lower crate must never depend on a higher one.

## Correctness

Output is checked against the real game through the differential oracle. See [specs/oracle.md](specs/oracle.md).
