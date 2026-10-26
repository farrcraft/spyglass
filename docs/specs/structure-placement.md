# Structure placement

Status: stable. Covers the Overworld of the Default world preset, implemented in `spyglass-structure` for 26.3 and checked against the oracle's [structure golden files](oracle.md#structure-golden-files).

The game decides where a structure starts in three steps:

1. **Placement**: whether a structure set may start in a chunk. It depends only on the seed, plus biomes for concentric rings.
2. **The biome check**: whether the structure's biome check passes at its check position. Most check positions depend on terrain.
3. **Validity**: the structure's pieces, and conditions on terrain.

This spec covers the first two, and says exactly what spyglass can decide without terrain. The rest is deferred to milestone 07 ([roadmap](../roadmap/README.md)). Classes are in `net.minecraft.world.level.levelgen.structure` unless stated otherwise.

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

- **`minecraft:random_spread`**: `spacing` and `separation` in 0..=4096, with spacing strictly greater ("Spacing has to be larger than separation"). `spread_type` is `linear` (the default) or `triangular`.
- **`minecraft:concentric_rings`**: `distance` and `spread` in 0..=1023, `count` in 1..=4095, and `preferred_biomes`.
- **Structure** (`Structure`): `type` and `biomes`, plus the jigsaw fields [below](#jigsaw-starts). The supported types are the 16 vanilla ones.
- **Biome sets** (`HolderSet<Biome>`) are `"#tag"`, an id, or a list of ids. Tags are `tags/worldgen/biome/*.json`, `{"values": [...]}`. Each entry is an id, a `#tag`, or `{"id", "required"}`, and a missing entry is skipped only when `required` is false. Biomes must exist in `worldgen/biome/`. Sets are resolved to the biome source's biomes; others, like the Nether's in the Overworld, can't occur, so they're dropped.
- **Which sets apply** (`ChunkGeneratorStructureState.hasBiomesForStructureSet`): a set applies when some structure's biomes intersect the source's. Other sets are kept with `possible: false`, because an exclusion zone may name one. In 26.3, 18 of the 21 sets apply to the Overworld. `end_cities`, `nether_complexes` and `nether_fossils` don't.
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

## Choosing a structure

In each placement chunk, `ChunkGenerator.createStructures` takes a single-entry set's structure directly. For several entries it does a weighted pick:

1. Seed one legacy random with `setLargeFeatureSeed(seed, chunkX, chunkZ)`.
2. Draw `n = nextInt(total weight)`, and walk the remaining entries in list order, subtracting each weight until `n` goes negative.
3. Try that structure. If it produces no valid start, remove it and draw again from the same random.

Each try builds its own context random (`GenerationContext.makeRandom`): a legacy random with `setLargeFeatureSeed(seed, chunkX, chunkZ)`, independent of the pick.

spyglass runs the same loop with a three-way answer for each try ([below](#what-spyglass-can-decide)). A `Rejected` structure is removed. A `Passes` structure ends the loop. A `Candidate` is recorded, and the loop goes on as if it failed. So a `Selection` lists the structures the game may start, in the order it tries them: candidates, then possibly one that passes. If the last one passes, the start is certain.

## The biome check

`Structure.findValidGenerationPoint` is `findGenerationPoint(context)` filtered by `GenerationContext.isValidBiome(stub)`. That resolves the biome at `QuartPos.fromBlock` of the stub's x, **y** and z, and tests it against the structure's `biomes`. Each type computes its stub (its check position) differently. Several also first require some valid biome somewhere in a column, which is climate-only:

- **Terrain-column test** (`couldValidBiomeExistInTerrainColumn(x, z)`): some quart from block `minY − 1` to `maxY` at `(x, z)` is in the set. The chunk-centre pre-check (`couldValidBiomeExistOnTopOfChunkCenter`) is this test at `(minX + 8, minZ + 8)`.
- **Structure-column test** (`couldStructureExistInColumn(x, z, minY, maxY)`): the same over the level's own height.

| Type                                            | Check position                                | y from                                                           | Climate-only conditions first                                                                                                                    | Other rejections (milestone 07)                   |
| ----------------------------------------------- | --------------------------------------------- | ---------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------- |
| stronghold                                      | `(minX, 0, minZ)`                             | constant                                                         | none                                                                                                                                             | none (its pieces are retried until valid)         |
| fortress                                        | `(minX, 64, minZ)`                            | constant                                                         | none                                                                                                                                             | none                                              |
| buried treasure, ocean ruin, shipwreck          | chunk centre                                  | `OCEAN_FLOOR_WG`; beached shipwrecks `WORLD_SURFACE_WG`          | chunk-centre pre-check                                                                                                                           | none                                              |
| desert pyramid, jungle temple, swamp hut, igloo | chunk centre                                  | `WORLD_SURFACE_WG`                                               | chunk-centre pre-check                                                                                                                           | pyramid and temple: lowest corner below sea level |
| ocean monument                                  | chunk centre                                  | `OCEAN_FLOOR_WG`                                                 | every biome in `getBiomesWithin(minX + 9, seaLevel, minZ + 9, 29)` is in `#required_ocean_monument_surrounding`; then the chunk-centre pre-check | none                                              |
| woodland mansion, end city                      | `(minX + 7, lowest corner, minZ + 7)`         | `WORLD_SURFACE_WG` over a 5 × 5 box rotated by a random rotation | terrain-column test at `(minX + 7, minZ + 7)`                                                                                                    | lowest corner below 60                            |
| jigsaw                                          | the start piece's bounding-box centre         | the start height, plus the heightmap if projected                | projected only: structure-column test at the centre                                                                                              | dimension padding (exact, below); pieces          |
| mineshaft                                       | `(minX + 8, 50 + dy, minZ)`                   | `dy` from the piece layout; mesa: terrain                        | none                                                                                                                                             | none                                              |
| ruined portal                                   | the template's bounding-box centre            | `findSuitableY` over noise columns                               | none                                                                                                                                             | none                                              |
| nether fossil                                   | `(minX + nextInt(16), y, minZ + nextInt(16))` | a height provider, then a downward block scan                    | none                                                                                                                                             | y ≤ sea level                                     |

`getBiomesWithin(x, y, z, r)` is every quart from `(p − r) >> 2` to `(p + r) >> 2` on each axis.

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

**Elements:**

- `single_pool_element` and `legacy_single_pool_element` are one template. The box is the template's corners `(0, 0, 0)` and `size − 1`, rotated about the origin and moved to the position.
- `list_pool_element` is the box around its elements' boxes, and uses its first element's jigsaw blocks.
- `feature_pool_element` is a 1 × 1 × 1 box.

The rotation is `StructureTemplate.transform` with mirror none and pivot 0. Clockwise 90 maps `(x, z)` to `(−z, x)`, counterclockwise 90 maps it to `(z, −x)`, and 180 maps it to `(−x, −z)`.

**Templates** are `data/minecraft/structure/<path>.nbt`, gzip-compressed NBT ([ADR 0013](../adr/0013-structure-templates-and-nbt.md)). spyglass reads these fields:

- `size`.
- `palette`, or a list of them in `palettes`. Each entry names its block by `id`, as `NbtUtils.readBlockState` reads it.
- `blocks`: `pos`, `state` (a palette index) and an optional `nbt` compound.
- Jigsaw blocks are the `minecraft:jigsaw` entries. They carry `name` (default `minecraft:empty`) and `selection_priority` (default 0) in their `nbt`.
- Order: the game sorts each block list by y, then x, then z. Jigsaw blocks have block-entity data, so they're all in one list, and keep that order.
- Palette choice: with several palettes, `StructurePlaceSettings.getRandomPalette` picks one with a random seeded from `Mth.getSeed(pos)`, not the context random.
- Data version: the game data-fixes a template saved by an older version. spyglass doesn't, so a template's `DataVersion` must equal the bundle's data version. All of 26.3's do.

## What spyglass can decide

For each try, `Starts::check` answers:

- **`Rejected`**: no start, whatever the terrain. Either a climate-only condition above fails for every candidate of every tied cell involved, or the check is at a terrain-independent position whose biome isn't in the set, or a jigsaw start has no start piece or doesn't fit its padding.
- **`Passes`**: the check is at a terrain-independent position and its biome is in the set for every tied candidate. In 26.3's Overworld that's strongholds, ancient cities and trial chambers.
- **`Candidate`**: everything else. Either the check position needs terrain (surface types, projected jigsaws, mineshafts, ruined portals), or a tied cell could go either way. Every climate-only condition has passed.

A lookup counts as in a set if every candidate of the cell is, and out if none is. Mixed cells make the answer `Candidate`.

`Passes` covers the biome check only. Validity that depends on pieces or terrain can still reject a start, as the table lists. Of the 175 starts the golden set calls certain, the game made all 175.

## Slime chunks

`Slime.checkSlimeSpawnRules`: chunk `(x, z)` is a slime chunk when `WorldgenRandom.seedSlimeChunk(x, z, seed, 987234911)` then `nextInt(10) == 0` ([rng.md](rng.md#worldgen-seeding-worldgenrandom)).

## Testing

- **Unit tests** cover codec defaults, ranges and errors on hand-written bundles; placement and the frequency reducers on hand-computed draws; `Math.round` and the trig flag; rotation, template boxes, the shuffle and height providers.
- **The ignored golden test** `crates/spyglass-structure/tests/golden.rs` compares against the [structure golden files](oracle.md#structure-golden-files):
  - Placement chunks of every applicable set, and slime chunks, must be equal in every region.
  - Ring positions must be equal unless flagged.
  - At every check position the game used, the game's biome must be among spyglass's candidates. Where the cell isn't tied, the check results must be equal.
  - Every jigsaw check position must equal the game's stub: exactly, or in x and z where projected.
  - Every start the game made must be among the selection's structures.

  Run it with `cargo test --release -p spyglass-structure -- --ignored`. For 26.3 it compares 6,056 placement chunks, 2,427 check positions, 682 jigsaw positions and 1,280 ring positions. 304 rings are flagged, and the 61 that differ are all among them.
