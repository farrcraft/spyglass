# Oracle approaches

How to get ground-truth worldgen output from the real game. Background for the oracle ADR. Facts were checked on 2026-10-24 against 26.3.

## What's changed

From 26.x the server jar ships **unobfuscated**: classes and members keep their real names, and the version JSON no longer lists `server_mappings`. Older tools assumed obfuscated jars, which made it costly to call game code directly without a mod loader's remapping. That cost is gone, and it reopens a standalone-harness option.

26.3 requires Java 25.

## Options

### A. Fabric mod in a dedicated server

A server-side mod hooks server start, then samples the world's biome source and chunk generator directly.

- Gets the exact state the game uses: datapacks loaded, `RandomState` created by the server itself. Highest confidence.
- Mature tooling: Fabric Loom (Gradle), and loader 0.20.1 supported 26.3 at release.
- Costs: Gradle and Loom setup, server start time on every run, and mod-loader support for new versions and snapshots. That support arrived quickly for 26.3; how fast it arrives for snapshots is to be measured.
- NeoForge (26.3.0.x) is an equivalent alternative, but Fabric is lighter for a server-only tool.

### B. Standalone Java harness on the server jar

A plain Java program, compiled with the unobfuscated server jar on the classpath, that bootstraps the game and calls the biome source without starting a server.

- No loader, no Gradle required (plain `javac` could work), and no wait for loader support: works on a snapshot the day it ships.
- Fast iteration, with no server process or world on disk, if bootstrap is light enough.
- Risk: the harness has to rebuild what server startup does (registry loading from the vanilla datapack, `RandomState` creation). If that means reproducing most of server startup, the advantage over A shrinks and correctness confidence drops.
- Touches more internal API directly, so it may need fixes on version bumps. A mod faces some of the same churn.

### C. Real server and region files

Run an unmodified server, pre-generate chunks, then read region files (e.g. with `fastanvil`).

- Zero custom game code, and checks the full pipeline, including features.
- Very slow, and only gives what's stored in chunks (biomes are stored at quart resolution, so this does work for biomes). Better suited to heightmaps and blocks, once terrain work starts, than to bulk biome sampling.

### D. Commands over RCON

`/locate`, `/place` and similar against a running server.

- No custom code, but slow and coarse: one answer per command. Useful for spot checks, not for golden files.

## Spike results (2026-10-24, 26.3)

A spike of option B was built and checked against real worlds, using option C's idea with saves that already existed. A wasn't prototyped; see the end of this section.

### B works with very little game code

About 80 lines of Java, compiled with `javac --release 25` against `server-26.3.jar` and the 39 libraries from the bundler. No mod loader, no server, no world on disk. It replicates exactly what the server does:

1. `SharedConstants.tryDetectVersion()` and `Bootstrap.bootStrap()`.
2. The same sequence as `WorldLoader`: `RegistryLayer.createRegistryAccess()`, then `TagLoader.loadTagsForExistingRegistries` on the vanilla pack (`ServerPacksSource.createVanillaPackSource().fullResources()`), then `RegistryDataLoader.load(..., WORLD_REGISTRIES, ...)` into the `WORLD` layer.
3. `WorldPresets.createNormalWorldDimensions(access)`, which is what a new Default world gets, then `RandomState.create(noises, seed, settings)`.
4. Biomes through the same path chunk generation uses (`ChunkGenerator.doCreateBiomes`): a `SamplerContext` with caches and a `DensityBufferPool`, `createClimateSampler`, then `BiomeSource.createResolverForChunk` per chunk.

26.3 has reworked this API (`SamplerContext`, `DensitySamplerSet`, `BiomeResolver`, `RegistryLayer.WORLD`, `VanillaPackResources.fullResources()`), so code written from memory of 1.21 doesn't compile. Reading the unobfuscated jar with `javap` was enough to find the real API each time.

Cost: bootstrap about 8.5 s and registry loading about 1.2 s, once per run. After that, about 1.5 million biome cells per second on one thread through the chunk path, and about 130,000 per second through the uncached point path (`createUncachedResolver`). Both paths gave identical answers on every cell tested.

### Validated against real worlds: exact, apart from exact ties

Two worlds created in 26.3 with the Default preset (`vanilla` plus Fabric API's tag-only convention pack) were compared cell by cell against the biomes stored in their region files:

| World seed             | Generated chunks | Cells compared | Mismatches | Explained by exact ties |
| ---------------------- | ---------------: | -------------: | ---------: | ----------------------: |
| `-4893098694207956070` |           29,416 |     15,060,992 |      5,804 |             5,804 (all) |
| `2514887864703934023`  |            8,199 |     12,593,664 |      4,608 |             4,608 (all) |

Every mismatch is a cell where two or more biomes are at **exactly the same** minimum fitness. `Climate.ParameterPoint.fitness` is integer arithmetic, so exact ties are possible. Both the world's biome and the harness's are among the tied candidates. About 1 cell in 2,000 is tied (35 of 75,281 sampled).

At a tie, the game's answer depends on **lookup history**. `Climate.RTree` keeps each thread's last result as the starting candidate, and its search only replaces a candidate that's strictly closer. Reversing the order the harness visits cells changed the mismatch count from 5,804 to 6,350. Chunks generate on worker threads in no fixed order, so the game itself can give tied cells different biomes in different runs. **There's no single correct answer for a tied cell.** Correctness for spyglass therefore means: an exact match wherever the minimum is unique, and one of the tied biomes where it isn't. The oracle has to report ties so comparisons can apply that rule.

### Other findings

- **Save layout changed in 26.x.** The seed and generator settings are no longer in `level.dat`; they're in `data/minecraft/world_gen_settings.dat` (`data.seed`, `data.dimensions`). Regions are under `dimensions/minecraft/<dimension>/region/`. `level.dat` still has `Data.DataVersion` (5023 for 26.3) and `Data.DataPacks.Enabled`.
- **Saves are excellent ground truth for biomes.** Every chunk at or past the biomes step stores the biome it generated at quart resolution. A short Python NBT reader was enough. Any 26.3 world a contributor has played gives millions of reference cells for free.
- **Running Mojang's code needs no EULA step.** Bootstrapping and registry loading don't start a server, so there's no `eula.txt`. A dedicated server for option A would need one, accepted by whoever runs it.

### Why A wasn't prototyped

A's main advantage was confidence: the game builds its own state. B now has that confidence directly, since it reproduces stored world output exactly, ties aside. A's remaining advantage is a live `ServerLevel` for things that need one: structure starts with full validity checks, jigsaw assembly and placed features. Those come later, and B may cope with them too: `StructureTemplateManager` and placement state are built from the same registries and resources. A stays the fallback if B can't build what those milestones need.

### Remaining questions

- How much extra state B needs for structure starts and validity, and for terrain.
- How to detect ties cheaply in the harness. Brute force over 7,594 parameter points per cell is too slow for full grids. A second tree search that returns everything at the minimum distance should work.
- How quickly the setup code breaks on snapshots. Measure on the next 26.4 snapshot.
