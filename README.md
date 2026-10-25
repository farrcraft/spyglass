# spyglass

A Rust-native, offline equivalent of the seed-map tools Chunkbase offers: biome maps, structure finders and seed search for Minecraft: Java Edition, aiming for bit-exact output on current versions.

> Status: scaffolding only. Nothing works yet.

Not affiliated with or endorsed by Mojang or Microsoft.

## Why

[cubiomes](https://github.com/cubitect/cubiomes) and Cubiomes Viewer are the standard open-source tools, but they hand-port per-version tables and lag behind new releases. Chunkbase is closed-source. spyglass keeps the **algorithms** in Rust and **extracts per-version parameters automatically** from the game's own jar. A new version should mean "run the extractor, diff it, fix the code that actually changed", not re-porting tables by hand.

## Sources of truth

Everything comes from Mojang's own distribution:

1. **Version manifest:** `https://piston-meta.mojang.com/mc/game/version_manifest_v2.json` lists releases and snapshots with links to server jars and official mappings.
2. **Jar data:** since 1.18 most worldgen lives in JSON under `data/minecraft/worldgen/` (noise settings, density functions, noise parameters, structure sets, biome tags).
3. **Data generator:** `java -DbundlerMainClass=net.minecraft.data.Main -jar server.jar --all` dumps registries and reports, including the overworld biome parameter list in recent versions.
4. **Readable code:** decompile with Vineflower plus the official mappings. (TODO: confirm that 26.x jars ship unobfuscated.)

**Licensing:** Mojang's data and code are never committed or redistributed. The extractor runs locally against a jar the user downloads themselves, the same approach Amidst used. Generated bundles go in `/bundles`, which is gitignored.

## Code vs data

| Layer | Code (Rust) | Data (extracted per version) |
|---|---|---|
| RNG | Java LCG, Xoroshiro128++, positional random factories, MD5 name seeding | — |
| Noise | Perlin/improved, octave, normal noise | Noise parameters |
| Density | Graph interpreter (later: flattened, SIMD-friendly evaluation) | Density-function graphs, noise settings |
| Biomes | Climate sampler, nearest-biome search tree | Biome parameter list |
| Structures | Random-spread and concentric-ring placement, validity checks, jigsaw | Structure sets, biome tags, template pools |
| Features | Decoration order, per-feature RNG | Placed/configured features |

## Crates

| Crate | Role |
|---|---|
| `spyglass-rng` | Bit-exact RNGs. Small, but every bug here poisons everything downstream. |
| `spyglass-noise` | Noise primitives. |
| `spyglass-data` | Version bundles, the manifest client and the extractor CLI. |
| `spyglass-density` | Loads and evaluates density-function graphs. |
| `spyglass-biome` | Climate sampling and biome lookup at quart (1:4) resolution; Overworld, Nether, End. |
| `spyglass-terrain` | Heightmaps and surface rules. |
| `spyglass-structure` | Placement (seed-only, cheap), then biome validation, then terrain-dependent validity, with optional jigsaw for exact bounds. |
| `spyglass-search` | Parallel seed search (`rayon`), filters ordered cheapest first. |
| `spyglass-viewer` | Native map app (`egui` + `wgpu`): tiled on-demand rendering, zoom levels, overlay layers. |

## Differential oracle

Correctness is tested against the real game, not assumed. A small server-side harness (likely a Fabric mod) loads a seed and calls the game's own functions directly, which is much faster than `/locate`. It dumps golden files: biome grids, structure starts and bounds, and heightmaps. CI runs the Rust crates against these for a fixed set of seeds per supported version.

## Prior art to study

- [Pumpkin](https://github.com/Pumpkin-MC/Pumpkin): Rust server whose `pumpkin-world` reportedly ports vanilla's noise router and density functions. Assess its parity and whether parts can be reused before writing terrain from scratch.
- [slime_seed_finder](https://github.com/Badel2/slime_seed_finder): Rust seed finder with a WebAssembly biome viewer; reference for search architecture and the 48→64-bit seed-extension trick.
- [Azalea](https://github.com/azalea-rs/azalea) / [Valence](https://github.com/valence-rs/valence): templates for generating per-version code and data from the game.
- `fastanvil`: reads region files, which is useful for comparing against real saves.
- [cubiomes](https://github.com/cubitect/cubiomes) / [Cubiomes Viewer](https://github.com/cubitect/cubiomes-viewer): the reference implementation for algorithms and features.

## Roadmap

1. Extractor + oracle harness for one current version.
2. RNG, noise and biomes, bit-exact against the oracle.
3. Structure placement, slime chunks, spawn point.
4. Viewer MVP.
5. Seed search with a filter language.
6. Terrain heights, structure validity, jigsaw.
7. Nether and End, then multiple versions.

Stretch: GPU (`wgpu` compute) structure-placement search over the lower 48 bits of the seed, with CPU expansion of the top 16 bits for biome checks.

**Out of scope for now:** Bedrock Edition (closed-source, different RNG and placement) and pre-1.18 versions (different biome system).
