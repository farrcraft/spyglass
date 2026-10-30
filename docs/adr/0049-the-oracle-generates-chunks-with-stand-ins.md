# 0049. The oracle generates chunks through the generator's steps, with stand-ins

- Status: Accepted
- Date: 2026-10-29

## Context

Milestone 18 ports the blocks of a chunk before features: the material rule, ore veins and carvers. Checking them needs the game's blocks for whole chunks, before features and without structures.

The game makes those blocks in its chunk steps, which only a server runs:

- **26.2:** `fillFromNoise`, `buildSurface` and `applyCarvers`.
- **26.3:** one `buildTerrain`.
- **26.4:** `buildTerrain`, after new biome statuses.

Each step is a public `ChunkGenerator` method, but each takes a `WorldGenRegion`, which needs a `ServerLevel`. The status tasks that call them (`ChunkStatusTasks`) also build a biome manager, a blender, a structure manager and a set of possible biomes from the region.

Neither existing way to call the game fits:

- **Plain calls** ([ADR 0004](0004-oracle-standalone-harness.md)) can't make a region.
- **A real server** ([ADR 0015](0015-locate-checks-on-a-real-server.md)) saves chunks only after features. Features place ores, plants and much else, so the blocks before them can't be recovered from a save.

From the bytecode, the steps use the region only to fetch neighbouring chunks (for carver biomes, and in 26.2 for the surface's possible biomes and biome manager) and for a few plain values:

- its centre and height range;
- its registries;
- whether there are old chunks to blend with.

## Decision

We will call the generator's own chunk steps on fresh `ProtoChunk`s, giving them stand-ins for the server's objects.

- **Stand-ins** are subclasses of `WorldGenRegion` and `StructureManager`, made without running any constructor: `sun.reflect.ReflectionFactory.newConstructorForSerialization`, called reflectively (`StandIns.allocate`).
  - They override only what the steps call: `getChunk`, `getCenter`, and in 26.2 `getBiomeManager`, `registryAccess`, `getMinY`, `getHeight` and `isOldChunkAround`.
  - Anything else reads a field that was never set and throws, so a version whose steps use more of the region fails loudly instead of differing silently.
- **No structures:** the stand-in structure manager has no starts, and the chunks have no references, so the beardifier is empty. Structures' blocks and terrain adaptation are out of the milestone's scope.
- **No blending:** `Blender.empty()`, as for a new world.
- **The harness repeats the status tasks' glue** around each step, version by version, in each adapter's `Chunks.java`: the biome steps, the possible-biome set, and the biome manager's seed. It's a few lines a version, read from `ChunkStatusTasks`, and the steps themselves are the game's.
- **Without carvers:** a generated data pack empties every biome's carver list, and the second run is named `<version>+no-carvers`. A carver with probability 0 isn't enough: it still starts when its random float is exactly 0.

## Consequences

- The block golden files are the game's own blocks before features, in every supported version, at about a second per seed and dimension.
- **A self-check** guards the stand-ins: wherever the [height files](../specs/oracle.md#height-golden-files)' columns overlap the chunks, air and fluids agree.
- **Fragile to version changes:** the stand-ins depend on which region methods the steps call. A new version can use more of the region or change the glue, as 26.4 did (new biome statuses, no biome manager in `buildTerrain`). A failure names the method. The fix is an override, or a new `Chunks.java` in the version's adapter.
- **An unsupported API:** `sun.reflect.ReflectionFactory` is in `jdk.unsupported`. It's exported, and serialization frameworks rely on it.
- **A copy of the glue is tested along with the game.** Where the copy is wrong, the files are wrong in the same way for spyglass. The self-check and the ports' own failures are what would show it.

## Alternatives considered

- **A real server and a save:** features are applied before chunks are saved, so ores, plants and other features hide the blocks under test.
- **Building a `ServerLevel`:** most of a server's startup in the harness, as ADR 0015 found. It's fragile across versions.
- **Copying the steps' code into the harness:** it would test a copy of the code under test.
- **Instantiating `WorldGenRegion` with null arguments:** its constructor reads them, so it fails before any step runs.
