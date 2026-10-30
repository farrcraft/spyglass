# m18. Block golden files

- Status: Completed
- Milestone: [18. Surface rules, ore veins, caves and carvers](../../roadmap/18-surface-veins-and-carvers.md)
- Started: 2026-10-29
- Completed: 2026-10-29

## Goal

The game's own blocks for whole chunks, as generation leaves them before features and structures, in every supported version and dimension, so the ports in [m18-material-rule](m18-material-rule.md) and [m18-carvers](../m18-carvers.md) have something exact to match. It's the reference behind the milestone's first exit criterion. This plan ports nothing itself.

## Approach

**What the game does, per version.** Read from the normalized bytecode in `target/version-diff/<version>/` (`cargo xtask version-diff --code`):

- **26.2:** three chunk steps, each a public `ChunkGenerator` method:
  - `fillFromNoise`: the noise, the aquifer and ore veins;
  - `buildSurface(WorldGenRegion, StructureManager, RandomState, ChunkAccess)`: `SurfaceRules`;
  - `applyCarvers(WorldGenRegion, seed, RandomState, BiomeManager, StructureManager, ChunkAccess, …)`: the carvers, which cut into the chunk's blocks.
- **26.3:** one public step, `buildTerrain(chunk, blender, randomState, structureManager, biomeManager, region, biomes)`. It fills from noise, runs the `levelgen.material` package's `MaterialSystem` (rules, conditions, and ore veins as `OreVeinRule`), then `generateCarvers`. Carvers no longer touch blocks: they mark a `CarvingMask`, and `applyCarvingMask` gives each marked position what the aquifer and the material rule say.
- **26.4 snapshots:** as 26.3, with carving in `generateCarvingMask` and `buildTerrain` split into a public wrapper and a private body.

So "after the material rule" and "after carvers" are separate steps only in 26.2. The files record two runs of every chunk instead, the same in every version:

- **`terrain`:** the full step or steps, which is what the game generates.
- **`no-carvers`:** the same, with a data pack that sets every configured carver's `probability` to 0. It's the material rule alone, and the carvers' part is the difference between the two. The harness already loads packs ([the test pack](../../specs/oracle.md#the-test-pack), [ADR 0030](../../adr/0030-a-test-pack-for-forms-vanilla-doesnt-use.md)). This pack is generated into the run directory, not committed, since it rewrites vanilla data.

**Calling the steps without a server.** Each step takes a `WorldGenRegion`, which only a `ServerLevel` can build. From the bytecode, the steps use the region only to fetch neighbouring chunks: for the carver biome of each chunk within 8 in 26.3 and 26.4, and for carving and the surface's neighbours in 26.2. The spike confirms this per version. The harness passes a stand-in:

- **The region:** a subclass of `WorldGenRegion`, instantiated without running its constructor (`sun.reflect.ReflectionFactory.newConstructorForSerialization`, from `jdk.unsupported`). Its `getChunk` and the few other methods the steps call return fresh `ProtoChunk`s at the asked position, with biomes filled as the game would fill them. Any other method throws, so a version that starts using more of the region fails loudly rather than silently.
- **No structures:** a fresh `ProtoChunk` has no structure starts or references, so the `Beardifier` is empty and no terrain adapts to structures. That's the milestone's scope.
- **No blending:** `Blender.empty()`.
- **The `StructureManager`:** a stand-in made the same way, if the spike shows the steps need one. With no references, its answers are empty.

This is a new way for the harness to call the game, between the plain function calls of [ADR 0004](../../adr/0004-oracle-standalone-harness.md) and the real server of [ADR 0015](../../adr/0015-locate-checks-on-a-real-server.md), so it gets an ADR. The alternatives:

- **The real server:** its chunks are saved only after features, which change blocks, so the material rule's and the carvers' blocks can't be separated from them.
- **Copying the steps' loops into the harness:** it would test a copy.
- **Building a `ServerLevel`:** fragile, as ADR 0015 found.

**Where it lives** ([ADR 0027](../../adr/0027-oracle-harness-across-versions.md), [ADR 0029](../../adr/0029-oracle-adapters-extend-a-base.md)):

- **The core:** chunk lists, the file format and the no-carvers pack.
- **Each adapter:** the stand-ins and the step calls, behind one `GameApi` method, `ChunkBlocks generateChunk(Game, RandomState, ChunkPos)`. 26.2's adapter calls three steps, 26.3's one, and 26.4's overrides what changed.

**The files.** `oracle-out/<version>/blocks/<dimension path>/<seed>.json`, one per golden seed and dimension, as for heights:

```json
{
  "format": 1,
  "harness": 1,
  "version": "26.3",
  "data_version": 5023,
  "seed": 0,
  "dimension": "minecraft:overworld",
  "min_y": -64,
  "height": 384,
  "chunks": [
    {
      "chunk": [0, 0],
      "terrain": [ [["minecraft:bedrock", 1], ["minecraft:deepslate", 7], ["minecraft:grass_block[snowy=false]", 1], ["minecraft:air", 300]] ],
      "no-carvers": [ … ]
    }
  ]
}
```

- **Columns:** each chunk has 256, x fastest within z. Each column is a list of runs from `min_y` up, as in the [height files](../../specs/oracle.md#height-golden-files).
- **Blocks:** a state is its id, then its non-default properties in brackets, sorted by name.
- **Chunks:**
  - a 4 × 4 block of chunks around the origin;
  - 2 × 2 at each of `far-pos`, `far-neg`, `border-pos` and `border-neg`.

  That's 32 chunks a file. Carvers reach 8 chunks, so caves cross every block's edges.

- **Sizes and cost:** recorded in the Notes once measured. `heights` takes 90 seconds; this should be of the same order.

**The Rust side.** `spyglass-oracle` gains a reader, `blocks::GoldenChunks`, with the runs expanded to a block per position, and a helper that names the first difference: the chunk, block position, expected and found. The comparison tests come with the ports, and each goes in `xtask/src/oracle_test.rs`'s list.

## Tasks

- [x] **Spike:** in a scratch adapter method, for 26.2, 26.3 and 26.4-snapshot-6, run the steps on one Overworld chunk with stand-in region and structure manager. List every region method they call. Record what was found in the Notes.
- [x] **ADR:** "The oracle generates chunks through the generator's own steps", covering the stand-ins, the no-carvers pack, and the alternatives above.
- [x] **The no-carvers pack:** generated into `target/oracle/<version>/run/`, from the bundle's carvers, with `probability` 0. Check that `terrain` and `no-carvers` differ in some chunks and agree outside carved positions.
- [x] **Harness:** `GameApi.generateChunk`, the stand-ins in each adapter, `Blocks.java` in the core (chunk list, runs, state names, two runs per chunk, threads), and `cargo xtask oracle blocks [--threads N]`. Add it to `golden`, and to `ci`'s outputs.
- [x] **Rust reader** in `spyglass-oracle`, with a unit test on a hand-written file, and the first-difference helper.
- [x] **Self-check:** in every golden column, the bottom of `no-carvers` agrees with the [height files](../../specs/oracle.md#height-golden-files) where both describe the same thing: air and fluids where the column has them, and solid where it's solid. An ignored test. It catches a stand-in that changed the terrain.
- [x] **Docs:** oracle.md's "Block golden files" section, its command table, and the golden set's time. [adding-a-version.md](../../adding-a-version.md): the stand-ins are a place a new version can break.
- [x] Generate the files for 26.2, 26.3, 26.4-snapshot-5 and 26.4-snapshot-6. `cargo xtask check`.

## Verification

- The self-check against the height files, for every version and dimension.
- The two runs differ in carved positions only, a check in the harness.
- A second run of `oracle blocks` writes the same bytes.

## Durable outputs

- [x] ADR: the oracle generates chunks through the generator's steps, with stand-in regions.
- [x] oracle.md: block golden files.
- [x] adding-a-version.md: the stand-ins.

## Notes

- 2026-10-29: **Results.**
  - **The files:** for 26.2, 26.3, 26.4-snapshot-5 and 26.4-snapshot-6, the 10 golden seeds in three dimensions, 32 chunks each, with and without carvers. About 50 seconds per version on 16 cores, and about 45 MB per run.
  - **Self-check:** the 73.4 million blocks of each version's runs without carvers agree with spyglass's columns on air and fluids, apart from what the material rule does there (below).
  - **Carving check:** the runs differ only at solid blocks, about 800,000 per version.
  - **Determinism:** a second run of 26.3 wrote the same bytes.
- 2026-10-29: **What the material rule does beyond choosing solid blocks**, found by the self-check, identically in all four versions:
  - frozen-ocean icebergs (packed ice and snow blocks over water and air);
  - eroded badlands pillars (terracotta, red sand and dirt over air);
  - the Nether's `hole` rule, which makes a floor below y 32 into lava.

  Carving also turns dirt into grass next to what it opens. 26.2's Nether carver carves to `cave_air`.

- 2026-10-29: **For m18-material-rule.** At seed 1's Nether, x 29,999,940 and z 29,999,929, the game's `getBaseColumn` has netherrack from y 5 to 27 where its own chunk, and spyglass's column, have lava. So the game's column path and chunk path can differ far from the origin, and the chunk path is what the blocks need. `cargo xtask oracle column <seed> <x> <z> --dimension D` prints both side by side.
- 2026-10-29: **Changes from the approach.**
  - **Two files instead of two runs in one file:** the run without carvers is its own version, `<version>+no-carvers`, like the test pack's. A pack changes the registries, so it's a separate harness run anyway.
  - **No `probability` 0:** a carver with probability 0 still starts when its random float is 0. The pack empties the biomes' carver lists instead.
  - **26.2's adapter loads packs now.** It refused them before, since the test pack doesn't load in 26.2.
- 2026-10-29: **Spike.** The steps call these on the region:
  - **26.3 and 26.4:** only `getChunk` (carver biomes of neighbours).
  - **26.2:** also `getCenter`, `getBiomeManager`, `registryAccess`, `getMinY`, `getHeight` and `isOldChunkAround`.

  The structure manager needs only `startsForStructure`. Each version's glue differs:
  - **26.3:** possible biomes come from 3 × 3 chunks, and `buildTerrain` takes a biome manager.
  - **26.4:** the biome statuses are new (`NOISE_BIOMES`, then `BIOMES` upscaled from neighbours), possible biomes come from the chunk alone, and there's no biome manager.
  - **26.2:** the carver step gets the level's biome manager.
