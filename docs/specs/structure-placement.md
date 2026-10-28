# Structure placement

Status: stable. Covers the Default world preset's three dimensions, implemented in `spyglass-structure` for 26.3 and checked against the oracle's [structure golden files](oracle.md#structure-golden-files). 26.2 and the 26.4 snapshots are implemented and checked against the oracle; 26.4-snapshot-6's [corner constraints](#corner-constraints), which the vanilla data doesn't use, by the [test pack](oracle.md#the-test-pack).

The game decides where a structure starts in three steps:

1. **Placement**: whether a structure set may start in a chunk. It depends only on the seed, plus biomes for concentric rings.
2. **The biome check**: whether the structure's biome check passes at its check position. Most check positions depend on terrain.
3. **Validity**: the structure's pieces, and conditions on terrain.

This spec covers the first two, and says exactly what spyglass can decide without terrain and [with terrain](#with-terrain) from [terrain.md](terrain.md). Of validity, it covers [jigsaw assembly](#jigsaw-assembly) and the [bounds](#bounds) it gives. Classes are in `net.minecraft.world.level.levelgen.structure` unless stated otherwise.

_Versions:_ placement (random spread, concentric rings, salts, frequency reducers, exclusion zones, the locate position), the weighted structure pick and jigsaw assembly (`JigsawPlacement.addPieces` and `$Placer`: the queue, depth, `size`, pool aliases, dimension padding, free space and the expansion hack) are the same in 26.2, 26.3 and 26.4-snapshot-5, as are `BoundingBox`, template boxes, rotation and jigsaw lists, and the single, list and empty pool elements. Where a version differs, a note says so in its section.

## Loading

`StructureSets::load(bundle, preset, dimension, source)` reads every `worldgen/structure_set/*.json` in the bundle, sorted by id, and the structures they name. Loading follows each codec ([ADR 0010](../adr/0010-typed-loading-of-worldgen-json.md)), and unknown types are errors that name the resource and the JSON path.

- **Structure set** (`StructureSet`): `placement`, and `structures`, a list of `{structure, weight}` with a positive weight. List order is the order the weighted pick walks.
- **Placement**, shared fields (`AbstractSpreadingStructurePlacement`):

  | Field                        | Default     | Rule                                       |
  | ---------------------------- | ----------- | ------------------------------------------ |
  | `locate_offset`              | `[0, 0, 0]` | each in −16..=16 (`Vec3i.offsetCodec(16)`) |
  | `frequency_reduction_method` | `default`   | or `legacy_type_1`, `_2`, `_3`             |
  | `frequency`                  | 1           | 0..=1, as `f32`                            |
  | `salt`                       | required    | an int                                     |
  | `exclusion_zone`             | none        | `{other_set, chunk_count}`, count 1..=16   |

  _Versions:_ in 26.2, `StructurePlacement` is an abstract class that holds these fields and `isStructureChunk` itself. 26.3 makes it an interface and moves them, unchanged, into `AbstractSpreadingStructurePlacement`. 26.3 also adds the placement type `minecraft:dimension_origin` (`DimensionOriginStructurePlacement`), which no vanilla set uses in any supported version ([below](#dimension-origin)).

- **`minecraft:dimension_origin`** (26.3 on): no fields. Its codec is `MapCodec.unit`, so the fields above are neither required nor read, and other keys are ignored.
- **`minecraft:random_spread`**: `spacing` and `separation` in 0..=4096, with spacing strictly greater ("Spacing has to be larger than separation"). `spread_type` is `linear` (the default) or `triangular`.
- **`minecraft:concentric_rings`**: `distance` and `spread` in 0..=1023, `count` in 1..=4095, and `preferred_biomes`.
- **Structure** (`Structure`): `type` and `biomes`, plus the jigsaw fields [below](#jigsaw-starts). The supported types are the 16 vanilla ones.
- **Biome sets** (`HolderSet<Biome>`) are `"#tag"`, an id, or a list of ids. Tags are `tags/worldgen/biome/*.json`, `{"values": [...]}`. Each entry is an id, a `#tag`, or `{"id", "required"}`, and a missing entry is skipped only when `required` is false. Biomes must exist in `worldgen/biome/`. Sets are resolved to the biome source's biomes; others, like the Nether's in the Overworld, can't occur, so they're dropped.
- **Which sets apply** (`ChunkGeneratorStructureState.hasBiomesForStructureSet`): a set applies when some structure's biomes intersect the source's. Other sets are kept with `possible: false`, because an exclusion zone may name one. In 26.3, 18 of the 21 sets apply to the Overworld. `end_cities`, `nether_complexes` and `nether_fossils` don't.

  _Versions:_ the data differs, not the loading. 26.2 has 20 sets: `abandoned_camp` (one set of jigsaw structures, one per biome) is new in 26.3, which also adds `dappled_forest` to `#has_structure/trial_chambers`. 26.4-snapshot-5 adds its new `ice_caves` biome to `#has_structure/mineshaft`, `#has_structure/ruined_portal_standard`, `#has_structure/trial_chambers` and `#stronghold_biased_to`.

- **Heights**: the preset's dimension type gives the level's `min_y` and `height`; its noise settings give `noise.min_y`, `noise.height` and `sea_level`. `WorldGenerationContext` uses the larger minimum and the smaller height.

## Placement

`StructurePlacement.isStructureChunk(state, x, z)` is the placement test, then the frequency test, then the exclusion zone. Integer arithmetic wraps; `floorDiv` rounds down. The seeding methods are in [rng.md](rng.md#worldgen-seeding-worldgenrandom).

**Random spread** (`RandomSpreadStructurePlacement.getPotentialStructureChunk`):

- The cell is `(floorDiv(x, spacing), floorDiv(z, spacing))`. A legacy random is seeded with `setLargeFeatureWithSalt(seed, cellX, cellZ, salt)`.
- `n = spacing − separation`. The x offset and then the z offset are each `nextInt(n)` for linear spread, or `(nextInt(n) + nextInt(n)) / 2` for triangular.
- The candidate chunk is `(cellX · spacing + dx, cellZ · spacing + dz)`, and the test passes when it is `(x, z)`.

**Frequency** applies only when `frequency < 1`. Each method's arguments are passed as `shouldGenerate(seed, salt, x, z, frequency)`:

| Method          | Game method                             | Passes when                                                                                                                                         |
| --------------- | --------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------- |
| `default`       | `probabilityReducer`                    | `setLargeFeatureWithSalt(seed, salt, x, z)` then `nextFloat() < frequency`. Note the argument order: salt, x and z land in the x, z and salt slots. |
| `legacy_type_1` | `legacyPillagerOutpostReducer`          | a legacy random seeded with `(long)((x >> 4) ^ (z >> 4) << 4) ^ seed`, then `nextInt()` discarded, then `nextInt((int)(1.0f / frequency)) == 0`.    |
| `legacy_type_2` | `legacyArbitrarySaltProbabilityReducer` | `setLargeFeatureWithSalt(seed, x, z, 10387320)` then `nextFloat() < frequency`.                                                                     |
| `legacy_type_3` | `legacyProbabilityReducerWithDouble`    | `setLargeFeatureSeed(seed, x, z)` then `nextDouble() < (double) frequency`.                                                                         |

**Exclusion zone** (`ChunkGeneratorStructureState.hasStructureChunkInRange`): the chunk is excluded if the other set's `isStructureChunk` holds anywhere within `chunk_count` chunks on both axes. This uses placement only, never biomes, and the other set needn't apply to the dimension.

**Locate position** (`getLocatePos`): the chunk's minimum block x and z, at y 0, plus `locate_offset`.

### Dimension origin

From the 26.3 and 26.4-snapshot-6 bytecode (`DimensionOriginStructurePlacement`). `isStructureChunk(state, x, z)` is true only at `state.getDimensionOrigin()`. There's no frequency test, salt or exclusion zone. The origin is set by `ChunkGenerator.createState` from `getOrigin(randomState)`. For a noise generator, that's the chunk containing `NoiseSpawnFinder.findSpawnPosition` over the noise settings' spawn targets, or chunk (0, 0) if there are none. This is the chunk [spawn.md](spawn.md) computes as the spawn origin, so spyglass works it out once per seed, and only for a dimension that has such a set. Where that chunk is platform-dependent ([spawn.md](spawn.md)), so is the set's.

`ChunkGenerator.findNearestMapStructure` handles only concentric rings and random spread, so `/locate` never finds a structure placed this way, and neither does spyglass's ([locate.md](locate.md)). The lower 48 bits don't decide it, so searches can't prefilter on it ([below](#the-lower-48-bits)).

### Concentric rings

`ChunkGeneratorStructureState.generateRingPositions` computes the positions once per seed, in order:

1. A legacy random seeded with the level seed (`concentricRingsSeed`). It doesn't depend on the dimension's origin.
2. `angle = nextDouble() · π · 2`. Start with ring 0, position 0 within the ring, and `spread` as given.
3. For each of `count` positions:
   1. The distance is `(double)(4·distance + distance·ring·6) + (nextDouble() − 0.5) · (distance · 2.5)`.
   2. The ring point is `x = (int) Math.round(Math.cos(angle) · d)` and `z = (int) Math.round(Math.sin(angle) · d)`.
   3. `fork()` the random for this position's biome search. Then `angle += 2π / spread`.
   4. After `spread` positions in a ring, move to the next ring. Then `spread += 2·spread / (ring + 1)`, `spread = min(spread, count − i)` (where i is the position's index) and `angle += nextDouble() · π · 2`.
4. Each ring point then moves to a preferred biome, with `findBiomeHorizontal` from block `(16x + 8, 0, 16z + 8)`, radius 112, increment 1, through the forked random. The match's chunk is the position; without a match the ring point stays.

`findBiomeHorizontal` without `findClosest` scans the full square of quarts within `radius >> 2`, z outer and x inner, at one height. The first match is kept. Each later match replaces it when `nextInt(matches so far + 1) == 0`, and the count grows with every match.

**Platform-dependent positions.** `Math.cos` and `Math.sin` aren't reproducible across platforms. A ring point is flagged `platform_dependent` when moving either value by up to 4 ulps changes its rounded result ([ADR 0012](../adr/0012-java-trig-results-that-depend-on-the-platform.md)).

**Tie-dependent positions.** A tied biome cell can match the preferred biomes for one candidate and not another, and the game's answer depends on lookup history ([biome-lookup.md](biome-lookup.md#ties)). spyglass uses each cell's first candidate. It then replays the search for every combination of the disagreeing cells, up to 12 of them, from the same random state. A position is flagged `tie_dependent` if any replay moves it. Beyond 12 such cells it's flagged without replaying.

_Versions:_ 26.2's `findBiomeHorizontal` takes `randomState.sampler()` and reads `BiomeSource.getNoiseBiome(qx, qy, qz, sampler)`. 26.3 and 26.4-snapshot-5 take the `RandomState` and read through `BiomeSource.createCachingResolver(randomState)`, a climate sampler with caches enabled. The loops, the reservoir draw and every unique cell's biome are the same. Only a tied cell's answer can differ, since it depends on lookup history, and spyglass replays tied cells anyway.

## Choosing a structure

In each placement chunk, `ChunkGenerator.createStructures` takes a single-entry set's structure directly. For several entries it does a weighted pick:

1. Seed one legacy random with `setLargeFeatureSeed(seed, chunkX, chunkZ)`.
2. Draw `n = nextInt(total weight)`, and walk the remaining entries in list order, subtracting each weight until `n` goes negative.
3. Try that structure. If it produces no valid start, remove it and draw again from the same random.

Each try builds its own context random (`GenerationContext.makeRandom`): a legacy random with `setLargeFeatureSeed(seed, chunkX, chunkZ)`, independent of the pick.

_Versions:_ 26.4-snapshot-5 moves the pick into `StructureSet.tryGenerateStartInChunk`, with the same draws in the same order, and `ChunkGenerator.createStructures` still skips a set when the chunk already stores a start for one of its structures. `Structure.generate` loses its `int references` parameter (references are set after generation) and returns `null` for no start instead of `StructureStart.INVALID_START`. It's non-null exactly when the pieces aren't empty, which is 26.3's `StructureStart.isValid()`, so the same tries fail.

spyglass runs the same loop with a three-way answer for each try ([below](#what-spyglass-can-decide)). A `Rejected` structure is removed. A `Passes` structure ends the loop. A `Candidate` is recorded, and the loop goes on as if it failed. So a `Selection` lists the structures the game may start, in the order it tries them: candidates, then possibly one that passes. If the last one passes, the start is certain.

## The biome check

`Structure.findValidGenerationPoint` is `findGenerationPoint(context)` filtered by `GenerationContext.isValidBiome(stub)`. That resolves the biome at `QuartPos.fromBlock` of the stub's x, **y** and z, and tests it against the structure's `biomes`. Each type computes its stub (its check position) differently. Several also first require some valid biome somewhere in a column, which is climate-only:

- **Terrain-column test** (`couldValidBiomeExistInTerrainColumn(x, z)`): some quart from block `minY − 1` to `maxY` at `(x, z)` is in the set. The chunk-centre pre-check (`couldValidBiomeExistOnTopOfChunkCenter`) is this test at `(minX + 8, minZ + 8)`.
- **Structure-column test** (`couldStructureExistInColumn(x, z, minY, maxY)`): the same over the level's own height.

Heights are the generator's ([terrain.md](terrain.md#queries)). "First occupied" is `getFirstOccupiedHeight`, one below the heightmap's `getBaseHeight`.

_Versions:_ 26.4-snapshot-5 has no `getBaseHeight`. `getFirstOccupiedHeight` is one below `getFirstFreeHeight`, which it reimplements on the base column; [terrain.md](terrain.md#queries) says when its answer differs. `Structure`, `JigsawPlacement`, ruined portals and mineshafts call it where 26.3 calls `getBaseHeight`.

| Type                                            | Check position                                | y from                                                                                   | Climate-only conditions first                                                                                                                    | Other rejections (with terrain)                                    |
| ----------------------------------------------- | --------------------------------------------- | ---------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------ |
| stronghold                                      | `(minX, 0, minZ)`                             | constant                                                                                 | none                                                                                                                                             | none (its pieces are retried until valid)                          |
| fortress                                        | `(minX, 64, minZ)`                            | constant                                                                                 | none                                                                                                                                             | none                                                               |
| buried treasure, ocean ruin, shipwreck          | chunk centre                                  | first occupied, `OCEAN_FLOOR_WG`; beached shipwrecks `WORLD_SURFACE_WG`                  | chunk-centre pre-check                                                                                                                           | none                                                               |
| desert pyramid, jungle temple, swamp hut, igloo | chunk centre                                  | first occupied, `WORLD_SURFACE_WG`                                                       | chunk-centre pre-check                                                                                                                           | pyramid and temple: [lowest corner](#with-terrain) below sea level |
| ocean monument                                  | chunk centre                                  | first occupied, `OCEAN_FLOOR_WG`                                                         | every biome in `getBiomesWithin(minX + 9, seaLevel, minZ + 9, 29)` is in `#required_ocean_monument_surrounding`; then the chunk-centre pre-check | none                                                               |
| woodland mansion, end city                      | `(minX + 7, lowest corner, minZ + 7)`         | the lowest first occupied `WORLD_SURFACE_WG` of a 5 × 5 box rotated by a random rotation | terrain-column test at `(minX + 7, minZ + 7)`                                                                                                    | lowest corner below 60                                             |
| jigsaw                                          | the start piece's bounding-box centre         | the start height, plus the heightmap's first free height if projected                    | projected only: structure-column test at the centre                                                                                              | dimension padding (exact, below); `size` 0                         |
| mineshaft                                       | `(minX + 8, 50 + dy, minZ)`                   | `dy` from the piece layout; mesa: terrain                                                | none                                                                                                                                             | none                                                               |
| ruined portal                                   | `(minX, y, minZ)`                             | `findSuitableY`, from the template's centre and four block columns                       | none                                                                                                                                             | none                                                               |
| nether fossil                                   | `(minX + nextInt(16), y, minZ + nextInt(16))` | a height provider, then a downward block scan                                            | none                                                                                                                                             | y ≤ sea level                                                      |

`getBiomesWithin(x, y, z, r)` is every quart from `(p − r) >> 2` to `(p + r) >> 2` on each axis.

_Versions:_ 26.2's `Structure$GenerationContext` has no climate sampler or biome resolver, and none of the column tests (`couldValidBiomeExistOnTopOfChunkCenter`, `couldValidBiomeExistInTerrainColumn`, `couldStructureExistInColumn`). Its check is the private static `Structure.isValidBiome(stub, context)`:

```java
BlockPos p = stub.position();
return validBiome.test(chunkGenerator.getBiomeSource().getNoiseBiome(
    QuartPos.fromBlock(p.getX()), QuartPos.fromBlock(p.getY()), QuartPos.fromBlock(p.getZ()), randomState.sampler()));
```

That's the same quart as 26.3's. So in 26.2 the only climate-only condition the game runs is the monument's `getBiomesWithin`; the table's others aren't steps. Surface types go straight to `onTopOfChunkCenter`. The mansion and the end city draw the rotation first, then take the 5 × 5 box at `(minX + 7, minZ + 7)` (`getLowestYIn5by5BoxOffset7Blocks`). A projected jigsaw goes straight to its first free height. The outcomes are the same: each skipped test asks whether some quart of a column holds a valid biome, and the final check tests one quart of that column, since a first occupied height is in `minY − 1..=maxY`. The mansion's earlier rotation draw is unobservable, because each try has its own context random. The exception, which vanilla terrain never meets (inferred, not confirmed in the bytecode): a projected jigsaw whose check y falls below the level, such as a trail ruin (start height `absolute −15`) over a first free height below `minY + 15`, could start in 26.2 where 26.3 rejects it.

Pieces are built after the check (`GenerationStub` builds them lazily), so their random draws don't affect it. Mineshafts are the exception: they lay out their pieces first.

### Jigsaw starts

`JigsawStructure.findGenerationPoint` and `JigsawPlacement.addPieces`, up to the stub, draw from the context random in this order:

1. `P = (minX, start_height.sample(random), minZ)`.
   - `start_height` is a bare vertical anchor (`absolute`, `above_bottom` or `below_top`), `constant`, or `uniform`.
   - `uniform` is `nextInt(max − min + 1) + min`, or `min` alone if the range is empty.
   - Anchors resolve against `WorldGenerationContext`: `above_bottom` is `minY + o`, and `below_top` is `minY + height − 1 − o`.
2. The rotation is `Rotation.values()[nextInt(4)]`: none, clockwise 90, clockwise 180, counterclockwise 90.
3. The start element is `start_pool`'s weight-expanded list, `[nextInt(size)]`. An empty pool or an empty element means no start. Each pool entry repeats `weight` times (1..=150), in file order. Pool aliases that rename the start pool aren't supported; `PoolAliasLookup` has its own random, separate from the context random.
4. With `start_jigsaw_name` (in 26.3, only the ancient city's `city_anchor`):
   - The element's jigsaw blocks are shuffled and stably sorted by `selection_priority`, highest first. The shuffle is `Util.shuffle`: for `i` from the length down to 2, swap `i − 1` with `nextInt(i)`.
   - The first block whose `name` matches gives `offset = jigsawPos − P`. No match means no start.
   - Otherwise `offset = 0`.
5. The piece goes at `P − offset`. The check's x and z are the centre of the element's box at that position, `(maxX + minX) / 2` and `(maxZ + minZ) / 2`, with Java's truncating division.
6. y depends on projection:
   - **Not projected:** the check's y is `P.y`.
   - **Projected:** the structure-column test at the centre comes first. Then y is `P.y + getFirstFreeHeight(centre, heightmap) + offset.y`, which needs terrain.
7. With `dimension_padding` (an int for both, or `{bottom, top}`), the piece moves so its box's minimum y is at `y − 1`, the ground level delta. There's no start if the box then reaches below `minY + bottom` or above `maxY − top`.
8. From 26.4-snapshot-6, if the start element has a [corner constraint](#corner-constraints), there's no start (no stub) unless the moved box passes it. No draws.

_Versions:_ 26.2 has no structure-column test in step 6 ([above](#the-biome-check)). In 26.4-snapshot-5, anchors resolve against `VerticalAnchor.Context.from(generator, level)` instead of `WorldGenerationContext`, with the same minimum, height and sea level. It adds the anchor `relative_to_sea_level` (`seaLevel + offset`), which no structure uses.

**Elements:**

- `single_pool_element` and `legacy_single_pool_element` are one template. The box is the template's corners `(0, 0, 0)` and `size − 1`, rotated about the origin and moved to the position.
- `list_pool_element` is the box around its elements' boxes, and uses its first element's jigsaw blocks.
- `feature_pool_element` is a 1 × 1 × 1 box.

The rotation is `StructureTemplate.transform` with mirror none and pivot 0. Clockwise 90 maps `(x, z)` to `(−z, x)`, counterclockwise 90 maps it to `(z, −x)`, and 180 maps it to `(−x, −z)`.

**Templates** are `data/minecraft/structure/<path>.nbt`, gzip-compressed NBT ([ADR 0013](../adr/0013-structure-templates-and-nbt.md)). spyglass reads these fields:

- `size`.
- `palette`, or a list of them in `palettes`. Each entry names its block by `id` and holds its state's properties in `properties`, as `NbtUtils.readBlockState` reads them.
- `blocks`: `pos`, `state` (a palette index) and an optional `nbt` compound.
- Jigsaw blocks are the `minecraft:jigsaw` entries (`JigsawBlockInfo.parse`). Their state's `orientation` is `<front>_<top>`, `north_up` if absent. Their `nbt` holds `name`, `pool` and `target` (identifiers, default `minecraft:empty`), `joint` (`rollable` or `aligned`; default `aligned` for a horizontal front, `rollable` for up or down), `placement_priority` and `selection_priority` (default 0).
- Order: the game sorts each block list by y, then x, then z. Jigsaw blocks have block-entity data, so they're all in one list, and keep that order.
- Palette choice: with several palettes, `StructurePlaceSettings.getRandomPalette` picks one with a random seeded from `Mth.getSeed(pos)`, not the context random.
- Data version: the game data-fixes a template saved by an older version. spyglass doesn't, so a template's `DataVersion` must equal the bundle's data version. All of 26.3's do.
- A missing template is empty (`StructureTemplateManager.getOrCreate`): size zero and no palettes, so its box is the corners `(0, 0, 0)` and `(−1, −1, −1)`, and it has no jigsaw blocks. 26.3's `ancient_city/walls/no_corners` pool names one, `intact_horizontal_wall_stairs_5`.

_Versions:_ 26.2's `NbtUtils.readBlockState` reads the block from **`Name`** and its properties from **`Properties`**; 26.3 and 26.4-snapshot-5 read `id` and `properties`. Every palette entry in 26.2's templates uses the 26.2 keys, so finding jigsaw blocks (`minecraft:jigsaw`) and their `orientation` depends on reading them. Nothing else spyglass reads differs: top-level keys, block entries and jigsaw `nbt` keys are the same, and each version's templates carry its own data version (26.2: 4903; 26.4-snapshot-5: 5125). In 26.4-snapshot-5 a missing template is `StructureTemplate.EMPTY` (`getOrEmpty`), with the same empty box and no jigsaw blocks.

## What spyglass can decide

For each try, `Starts::check` answers, from the climate alone (`Starts::new`) or with the seed's terrain (`Starts::with_terrain`, [below](#with-terrain)):

- **`Rejected`**: no start, whatever the terrain. Either a climate-only condition above fails for every candidate of every tied cell involved, or the check is at a terrain-independent position whose biome isn't in the set, or a jigsaw start has no start piece or doesn't fit its padding.
- **`Passes`**: the check is at a terrain-independent position and its biome is in the set for every tied candidate. In 26.3's Overworld that's strongholds, ancient cities and trial chambers. A mineshaft passes too when every quart of the level's column at its check's x and z is in its set: its check's y, from the piece layout spyglass doesn't port, is somewhere in that column.
- **`Candidate`**: everything else. Either the check position needs terrain (surface types, projected jigsaws, mineshafts, ruined portals), or a tied cell could go either way. Every climate-only condition has passed.

With terrain, the same answers hold with the real check position wherever spyglass computes it (every Overworld type but mineshafts, whose layout isn't ported): `Rejected` also covers terrain's own rejections, and a candidate remains only behind a tied cell or for a mineshaft whose column is mixed. `Starts::stub` gives the check position itself.

A lookup counts as in a set if every candidate of the cell is, and out if none is. Mixed cells make the answer `Candidate`.

_Versions:_ in 26.2 the climate-only conditions aren't game steps ([above](#the-biome-check)), but each is implied by the final check, so `Rejected` holds. For a projected jigsaw that's so only while the check's y is in the level, as it is on vanilla terrain (inferred, not confirmed in the bytecode).

`Passes` covers the biome check only. Without terrain, validity that depends on pieces or terrain can still reject a start, as the table lists. Of the 870 starts the golden set calls certain without terrain, the game made all 870. With terrain, 5,986 of its 6,056 placement chunks have a decided outcome, every one the game's ([Testing](#testing)).

## With terrain

Implemented in `Starts::with_terrain`, except mineshafts. Each type's check with the generator's heights ([terrain.md](terrain.md#queries)), in the order of its terrain queries and context-random draws (`R`). After the biome check, a start is valid if it has pieces (`StructureStart.isValid`); no type checks anything else. Corners are `getCornerHeights(x, dx, z, dz)`: first occupied `WORLD_SURFACE_WG` at `(x, z)`, `(x, z + dz)`, `(x + dx, z)`, `(x + dx, z + dz)`, one block outside a `dx`-wide footprint.

- **Surface and ocean types** (`onTopOfChunkCenter`): the chunk-centre pre-check, then the check at `(minX + 8, first occupied, minZ + 8)` with the type's heightmap. No draws before the check.
- **Desert pyramid, jungle temple** (`SinglePieceStructure`): first the pre-check, then the lowest of the corners at `(minX, minZ)` with `(dx, dz)` = (21, 21) for the pyramid and (12, 15) for the temple. Below the sea level, no start. Then as the surface types.
- **Woodland mansion:** the terrain-column test; then the rotation, `Rotation.values()[R.nextInt(4)]`; then the corners from `(minX + 7, minZ + 7)` with `(dx, dz)` = (5, 5) for none, (−5, 5) clockwise 90, (−5, −5) 180, (5, −5) counterclockwise 90. The lowest is the check's y; below 60, no start.
- **Ruined portal:**
  1. The setup: with several, `f = R.nextFloat()`, subtracting each `weight / total` (`f32`, list order) until `f < 0`. The air pocket draws only if its probability is strictly between 0 and 1 (none in 26.3).
  2. The template: `R.nextFloat() < 0.05` picks `giant_portal_1..3` with `R.nextInt(3)`, else `portal_1..10` with `R.nextInt(10)`. Then the rotation (`R.nextInt(4)`), and the mirror (`R.nextFloat() < 0.5`: none, else front-back).
  3. The box at `(minX, 0, minZ)` with pivot `(size.x / 2, 0, size.z / 2)`; its centre `min + (max − min + 1) / 2`; `Y0` = the first occupied height there (`OCEAN_FLOOR_WG` for `on_ocean_floor`, else `WORLD_SURFACE_WG`).
  4. `y` by placement: `on_land_surface`, `on_ocean_floor`: `Y0`; `partly_buried`: `Y0 − ySpan + R.nextInt(7) + 2`; `in_mountain`: between 70 and `Y0 − ySpan`; `underground`: between `minY + 15` and `Y0 − ySpan`; `in_nether`: with the air pocket `R.nextInt(69) + 32`, else `R.nextFloat() < 0.5` gives `R.nextInt(3) + 27` and otherwise `R.nextInt(72) + 29`. "Between `a` and `b`" is `R.nextInt(b − a + 1) + a` if `a < b`, else `b` with no draw. `minY` is the level's, and `ySpan` the template's height.
  5. `findSuitableY`: the block columns (`getBaseColumn`) at the box's four corners (min x min z, max x min z, min x max z, max x max z); from `y` down while above `minY + 15`, the first height where three of them hold a block the placement's heightmap counts (`OCEAN_FLOOR_WG` for `on_ocean_floor`, else `WORLD_SURFACE_WG`). If there's none, `minY + 15`. The check is at `(minX, y, minZ)`. The Overworld's set has the Nether's portal too: its check runs on the Overworld's terrain, and its biome test fails.
- **Mineshaft** (not implemented: the piece layout isn't ported): the pieces first (`R.nextDouble()` discarded, then the layout). Normal: moved below the sea level with `R`, as now. Mesa: `h` = `getBaseHeight` (not first occupied) of `WORLD_SURFACE_WG` at the layout's centre; the target is the sea level if `h ≤ seaLevel`, else `R.nextInt(h − seaLevel + 1) + seaLevel`, and `dy` = target − the centre's y.
- **Shipwreck:** no terrain beyond the check: the game's later height fix applies only to templates over 32 blocks, and 26.3 has none.
- **Projected jigsaw:** `y = start height + first free height` at the centre, then [as before](#jigsaw-starts).

_Versions:_

- **26.2 ruined portals** have `air_pocket_probability` 0.5 for `on_land_surface` (the standard, jungle and mountain setups) and for the Nether, where 26.3 has 0. That's data: the air pocket draw in step 1 happens, and the rest follows as written.
- **26.4-snapshot-5 ruined portals** (`RuinedPortalStructure`): `Y0` is `getFirstFreeHeight − 1` with the same heightmap, where 26.3 has `getBaseHeight − 1` ([terrain.md](terrain.md#queries)). `findSuitableY` tests the base column (`NoiseColumn`) directly, with the same draws, corner order, floor and three-of-four rule:

  ```java
  // 26.3
  Predicate<BlockState> p = (onOceanFloor ? OCEAN_FLOOR_WG : WORLD_SURFACE_WG).isOpaque();
  hit = p.test(column.getBlock(y));
  // 26.4-snapshot-5
  hit = onOceanFloor ? column.isSolid(y) : !column.isEmpty(y);
  ```

  `isSolid` is the default block and `isEmpty` is air. A base column holds only the default block, fluids and air, and the 26.3 predicates count the default block for both heightmaps and fluids for `WORLD_SURFACE_WG` only, so the answers are the same (inferred, not confirmed in the bytecode).

- **26.4-snapshot-5 mineshafts** (`MineshaftStructure`): the mesa's `h` is `getFirstFreeHeight` of `WORLD_SURFACE_WG`, where 26.3 has `getBaseHeight`.

So with terrain every Overworld type's check is at a known position, but for mineshafts until their layout is ported. The only answers that stay open are tied biome cells, platform-dependent ring positions, and mineshafts in mixed columns.

## Jigsaw assembly

Implemented in `Jigsaw::assemble` (`spyglass-structure`), with the pools `JigsawPools` loads. `JigsawPlacement.addPieces` after the start piece ([above](#jigsaw-starts)), continuing to draw from `R`. Below, `y` is the check's y.

**The start.** The start piece moves so its box's minimum y is `y − 1` (the ground level delta), projected or not. Dimension padding is checked only when the field is in the JSON: decoding never gives `DimensionPadding.ZERO`, which the game tests by identity, so an explicit `0` still checks against the bare world limits. With `size` 0 no piece is added, so there's no start.

**Free space.** `max_distance_from_center` (an int `n` in 1..=128 for both, or `{horizontal, vertical}`) gives a box around the start's centre `(cx, cz)`: x from `cx − h` to `cx + h`, z likewise, and y from `max(y − v, minY + bottom)` to `min(y + v, maxY − top)`, all inclusive, with the padding's `bottom` and `top` (0 without padding). If the y range is reversed, its ends swap (`AABB` orders its corners). The free space is that box minus the start piece's box. The game keeps it as a voxel shape and tests a piece's box shrunk by a quarter block, so a piece fits exactly when every block of its box is free; placing it removes its box. Each piece keeps the free space it was placed in, shared with its siblings, except that a parent jigsaw whose target is inside its own piece's box uses that box as free space (made once per piece, shared by all its jigsaws that need it).

**The queue.** Pieces wait in a queue (`SequencedPriorityIterator`) ordered by their parent jigsaw's `placement_priority`, highest first, first in first out among equals. The start piece's children are placed first, at depth 0; then each piece from the queue, until it's empty.

**One piece's children.** The piece's jigsaw blocks at its position and rotation, shuffled and sorted as for the start. For each parent jigsaw `J`, in that order:

1. `J`'s pool, after [aliases](#pool-aliases), and that pool's `fallback` (not aliased). If either is missing, or empty and not `minecraft:empty`, skip `J`.
2. Candidates: the pool's weight-expanded elements, shuffled (`Util.shuffle` of a copy), unless the piece is at depth `size`; then the fallback's, shuffled, appended. Each shuffle draws in full before any candidate is tried.
3. For each candidate: an empty element ends `J`. Else for each rotation in a shuffled copy of the four (3 draws): the candidate's jigsaw blocks at the origin with that rotation, shuffled and sorted (draws), and its box at the origin; then each of those jigsaws `C` that attaches to `J`, placed and tested against the free space as below. The first that fits is added to the start's pieces, queued at depth + 1 if that's at most `size`, with `J`'s placement priority, and `J` is done.

**Attaching.** `C` attaches to `J` (`JigsawBlock.canAttach`) if `C`'s front is opposite `J`'s, their tops are equal unless `J`'s joint is `rollable`, and `C`'s name is `J`'s target. A feature element's jigsaw has no name and matches any target. Orientation rotates with the piece: horizontal directions turn with the rotation, up and down stay.

_Versions:_ (switch `feature_jigsaw_named_bottom`) in 26.2, `FeaturePoolElement.getShuffledJigsawBlocks` names its jigsaw `minecraft:bottom` (from the NBT its constructor builds in `fillDefaultJigsawNBT`), and `JigsawBlock.canAttach` needs an exact match:

```java
// 26.2
return front(J) == front(C).getOpposite() && (J.jointType() == ROLLABLE || top(J) == top(C))
    && J.target().equals(C.name());
// 26.3, 26.4-snapshot-5: C's name is null for a feature element
return front(J) == front(C).getOpposite() && (J.jointType() == ROLLABLE || top(J) == top(C))
    && (C.name() == null || J.target().equals(C.name()));
```

So in 26.2 a feature element attaches only to a parent whose target is `minecraft:bottom`. With vanilla data the results are the same: every 26.2 jigsaw whose pool or fallback holds a feature element targets `minecraft:bottom`. (Some of 26.3's abandoned-camp jigsaws target `minecraft:down`, which needs the null rule.) In 26.4-snapshot-5's data, `village/snowy/houses/snowy_shepherds_house_1` has a jigsaw `name` and `target` fixed to `minecraft:building_entrance` (26.3 has it misspelled, with the `l` and `d` swapped), so that house's entrance can now attach to streets.

**Placing.** `T` is `J`'s position plus `J`'s front step, and `C₀` is `C`'s position at the origin. The candidate goes at `T − C₀`, then moves in y so its box's minimum y is:

- both pieces `rigid`: `parentMinY + (J.y − parentMinY) − C₀.y + front.y`, which puts `C` at `T`;
- otherwise: `FFH − C₀.y`, where `FFH` is the first free `WORLD_SURFACE_WG` height at `J`'s x and z, asked at most once per parent jigsaw.

Then, with `use_expansion_hack` and an origin box at most 16 high, the box's top is raised to `minY + max(e + 1, maxY − minY)`. `e` is the largest, over the candidate's jigsaws at the origin whose target is inside its origin box, of their pool's and its fallback's `getMaxSize` (the tallest non-empty element, unrotated; 0 for a missing pool), with aliases. The raised box is the one tested and kept.

**Elements.** Every element but the empty one has a `projection`, `rigid` or `terrain_matching`; a list element's is its own. A single or legacy single element may have `override_liquid_settings`, `apply_waterlogging` or `ignore_waterlogging`; any other value fails the pool's loading. Only `place` reads it, through `getSettings`, so it changes how blocks are written but no piece, box or draw, and spyglass checks it and ignores it. A single (or legacy single) element is its template's box and jigsaw blocks. A list element is the box around its elements' boxes, with its first element's jigsaw blocks. A feature element is one block, with one jigsaw at it: front down, top south, `rollable`, no name, pool and target `minecraft:empty`, no draws. The empty element has neither.

### Corner constraints

_Versions:_ 26.4-snapshot-6 on (`CornerConstraint`). Unused in vanilla.

Every element type but the empty one takes an optional `corner_constraint`: a list of 1 to 10 density predicates (`CornerConstraint.DensityPredicate`), all of which must hold. Each has:

- `density_function`: a reference or an inline function;
- `acceptable_density`: an inclusive range of doubles, written as one number, `[min, max]` or `{"min_inclusive", "max_inclusive"}`, `min` not above `max` by `Double.compareTo`;
- `padding`: 0 to 8, default 0;
- `corners`: `top`, `bottom` or `all` (the default);
- `required_corners`: an inclusive range of ints in 1 to 8, in the same three forms; at most 4 for `top` or `bottom`.

A predicate tests a box grown by `padding` on every side (`inflatedBy`). Its corners, in order: x max before min, inside y (top, then bottom, for `all`), inside z max before min. At each, the function's point value (`RandomState.getSampler`, uncached) is widened to `double` and counted if it's in the range by `Double.compareTo` (`−0.0` is below `+0.0`, NaN above everything). The predicate fails as soon as the count passes the maximum, or the count plus the corners left can't reach the minimum.

The start is tested after its move and the padding check (step 8 [above](#jigsaw-starts)). A child is tested after it fits the free space **and has been taken from it**: a child the constraint rejects still blocks the space, and the placer goes on to the next of the candidate's jigsaws, as after a collision. Only the outermost element's constraint counts; a list element's members' are ignored. The functions bind with the dimension's random source and [anchors](density-functions.md#gradient-anchors), per seed; spyglass binds each once per dimension at load (`JigsawPools`, and `StructureSets` for start pools) so a check can't fail to.

### Pool aliases

`pool_aliases` (in 26.3 only the trial chambers') resolve once per start, with their own random: `LegacyRandomSource(seed).forkPositional().at(minX, sampled start y, minZ)`. Bindings in list order: `direct` maps an alias to a pool; `random` picks one target by weight (`nextInt(total)`, then the entry that index falls in, in list order); `random_group` picks a group the same way and resolves its bindings in order with the same random. Every jigsaw block's pool goes through the result. The start pool would too, but spyglass doesn't support aliasing it (none of 26.3's structures does).

The weighted lists load as `WeightedList.nonEmptyCodec` reads them: at least one entry, each weight a non-negative int, the weights summing to at most 2,147,483,647 (`WeightedRandom.getTotalWeight`); anything else is a load error, as in the game. A list whose weights are all 0 loads in the game and throws when a start picks from it; spyglass refuses it at load instead.

## Bounds

A start's box (`StructureStart.getBoundingBox`) is the box around its pieces' boxes, grown by 12 on every side if the structure's `terrain_adaptation` isn't `none` (`Structure.adjustBoundingBox`). In 26.3's Overworld that's strongholds and every jigsaw. Boxes are as generated, before placement moves any piece. spyglass gives them for jigsaw structures (`Layout`).

Not covered: single-piece structures (whose boxes would come from `makeBoundingBox`: the pyramid's 21 × 15 × 21 at y 64, the temple's 12 × 10 × 15, the swamp hut's 7 × 7 × 9, rotated by the piece's random direction; the buried treasure's one block at `(minX + 9, 90, minZ + 9)`; the monument's 58 × 23 × 58 from `(minX − 29, 39, minZ − 29)`), mansions, strongholds, mineshafts and ocean-ruin clusters.

## The Nether and the End

From the 26.3 bytecode, checked against the structure golden files for both dimensions ([Testing](#testing)).

- **Which sets apply** works as in the Overworld. The Nether gets `nether_complexes` (fortress weight 2, bastion 3), `nether_fossils` and `ruined_portals`; the End gets `end_cities`. No ring placement applies in either, and none of their sets has an exclusion zone or frequency.
- **Biomes don't depend on y** in either dimension ([biome-lookup.md](biome-lookup.md#the-ends-source)), so every biome check is decided by its x and z from the climate alone. Terrain gives only the stub's y and terrain's own rejections.
- **Columns** hold blocks from y 0 to 127 (both noise ranges), in a level of 0 to 255. A Nether column is netherrack, lava up to y 31 and air from y 32; an End column is end stone and air.
- **Fortress:** always passes. Its biome tag is every Nether biome.
- **Bastion:** a jigsaw at constant height 33, not projected. Rotation and start element are drawn, and it's decided by the biome at its start piece's centre.
- **Nether fossil:**
  1. `x = minX + R.nextInt(16)`, `z = minZ + R.nextInt(16)`, then the height provider (26.3: `R.nextInt(94) + 32`, from 32 to `below_top 2` = 125 in the generator's range).
  2. Scan the base column at `(x, z)`: while `y > sea level` (32), take `top` at y, step down one, and stop if `top` is air and the block below is soul sand or has a sturdy top face.
  3. If the scan ended at or below sea level, there's no start; the stub is `(x, y, z)`.
  4. Its biome check is soul sand valley at `(x, z)`.

  In 26.3's columns this is the first y from the start − 1 down to 33 with netherrack at y and air above.

  _Versions:_ 26.4-snapshot-5's scan tests the base column directly, `while (y > seaLevel) { y--; if (column.isEmpty(y + 1) && column.isSolid(y)) break; }`, with the height from `VerticalAnchor.Context`. Soul sand never appears in a base column, so this is the same first y (inferred, not confirmed in the bytecode).

- **Ruined portal (`in_nether`):** as [above](#with-terrain), with one setup and no air pocket. Lava counts for `findSuitableY`, so in the Nether its y is always 27 to 100 (26.2's Nether setup has an air pocket probability of 0.5, so its draw happens, and y stays in that range). Its tag is every Nether biome, so it always passes. Without terrain, spyglass decides a ruined portal whose biomes are all of the source's (it passes wherever its check is) or none of them (it fails), as it does mineshafts whose column is all in their biomes.
- **The shared `ruined_portals` set** applies in both the Overworld and the Nether. In the Nether the six Overworld portals fail their biome checks and the pick draws again, so every placement chunk gets a Nether portal.
- **End city:** the mansion's footprint rule, as in the table. A corner over the void has base height 0, so its first occupied height is −1, below 60. The main island is `the_end`, which isn't in its biomes.

## Slime chunks

`Slime.checkSlimeSpawnRules`: chunk `(x, z)` is a slime chunk when `WorldgenRandom.seedSlimeChunk(x, z, seed, 987234911)` then `nextInt(10) == 0` ([rng.md](rng.md#worldgen-seeding-worldgenrandom)).

## The lower 48 bits

Seed search ([search.md](search.md)) skips whole groups of seeds using results that depend only on the seed's lower 48 bits. These do:

- random-spread placement, every frequency reducer, the weighted structure pick, exclusion zones against random-spread sets, and slime chunks.

Each of them seeds a legacy random ([rng.md](rng.md#legacy-legacyrandomsource)), whose `setSeed` keeps only the lower 48 bits of its argument, and builds that argument from the world seed with additions, multiplications and XORs, which never carry from high bits into low ones. `setLargeFeatureSeed`'s `a` and `b` are drawn from a legacy random seeded with the world seed itself, so they too depend only on its lower 48 bits. So two seeds that agree in their lower 48 bits get the same placement chunks, frequency results, picks and slime chunks.

These don't, and need the whole seed: concentric rings (their positions move towards preferred biomes), exclusion zones against a concentric-ring set, and every biome check, because the climate noise is seeded through Xoroshiro from all 64 bits.

The biome check only ever removes starts, so a count of placement chunks is an upper bound on the count of starts for every seed with the same lower 48 bits.

## Testing

- **Unit tests** cover codec defaults, ranges and errors on hand-written bundles; placement and the frequency reducers on hand-computed draws; `Math.round` and the trig flag; rotation, template boxes, the shuffle and height providers; ruined portals' transform, setup pick and air pocket draws.
- **The ignored golden test** `crates/spyglass-structure/tests/golden.rs` compares against the [structure golden files](oracle.md#structure-golden-files):
  - Placement chunks of every applicable set, and slime chunks, must be equal in every region.
  - Ring positions must be equal unless flagged.
  - At every check position the game used, the game's biome must be among spyglass's candidates. Where the cell isn't tied, the check results must be equal.
  - Every jigsaw check position must equal the game's stub: exactly, or in x and z where projected.
  - Every start the game made must be among the selection's structures.
  - With terrain (`structures_with_terrain_match_the_oracle`): every stub spyglass computes must equal the game's exactly, and where the game has no stub spyglass must have none, unless a tied condition stands before it. Every outcome spyglass decides (a certain start, or none) must be the game's. For 26.3 that's 1,640 stubs, all equal, with mineshafts' 787 left unknown, and 5,986 outcomes decided of 6,056, all equal.
  - With terrain and pools, in the same test: every jigsaw start the game made must assemble to the game's pieces, box for box in order, and to the game's bounds. For 26.3 that's 301 starts and 47,534 pieces, the trial chambers' aliases included.
  - For 26.4-snapshot-6's corner constraints, the [test pack](oracle.md#the-test-pack) adds a plains village on a frequent spread whose town centres (bottom corners) and streets (all corners, and top corners with padding) are constrained, its streets reached through a pool alias. Over the ten seeds the game rejects 4,164 starts by their constraint and makes 1,544, which assemble to the game's pieces, rejected streets included.

  Run it with `cargo test --release -p spyglass-structure -- --ignored`. For 26.3 it compares 6,056 placement chunks, 2,427 check positions, 682 jigsaw positions and 1,280 ring positions in the Overworld. 304 rings are flagged, and the 61 that differ are all among them.

- **Both tests run in the Nether and the End too.** For 26.3:
  - The Nether: 51,605 placement chunks and 26,142 check positions equal; 405 starts decided without terrain; with terrain, 26,142 stubs equal and 51,604 outcomes decided, the one left open behind a tie; 132 bastions assembled to the game's 12,530 pieces.
  - The End: 510 placement chunks equal; with terrain, 17 end city stubs equal and all 510 outcomes decided.
  - Dropping the nether fossil scan's test for air above its floor fails 33,070 checks.
