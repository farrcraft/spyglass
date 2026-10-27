# Terrain

Status: stable. Covers what the generator answers about terrain before any chunk exists, in the Default preset's three dimensions, for 26.2, 26.3 and 26.4-snapshot-5, in `spyglass-terrain` ([ADR 0022](../adr/0022-terrain-crate-and-volume-evaluation.md)). Implemented, and checked against the oracle's [probe files](oracle.md#probe-files) (the aquifer) and [height golden files](oracle.md#height-golden-files) (every query) for all three. Ported from the 26.3 bytecode; 26.4-snapshot-5's differences are _Versions_ notes, and 26.2, another algorithm family ([ADR 0026](../adr/0026-how-game-versions-coexist.md)), has its own [section](#262-terrain), ported from the 26.2 bytecode.

Structures decide where they start, and at what height, before their chunks are generated. They ask the chunk generator for heights and block columns, which it computes from the density graph and the aquifer alone. That's this spec. Real chunks differ from these answers: structures' terrain adaptation (the beardifier), the material rule (surface blocks, ore veins, frozen-ocean holes, swamp water), carvers and features all come later. Classes are in `net.minecraft.world.level.levelgen` unless stated otherwise.

## Queries

`NoiseBasedChunkGenerator`, for a level of the dimension type's height:

| Query                                     | Answer                                                                                                                               |
| ----------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------ |
| `getBaseHeight(x, z, heightmap)`          | [The column](#the-column), scanned from the top: one above the first block the heightmap counts. If none, the **level's** minimum y. |
| `getBaseColumn(x, z)`                     | Every block of the column. Outside it, air.                                                                                          |
| `getFirstFreeHeight(x, z, heightmap)`     | `getBaseHeight`.                                                                                                                     |
| `getFirstOccupiedHeight(x, z, heightmap)` | `getBaseHeight − 1`.                                                                                                                 |
| `getSpawnHeight`                          | 64.                                                                                                                                  |

_Versions:_ (switch `solid_only_heights` for 26.4) 26.2's queries are the same, with its own [column](#262-columns). 26.4-snapshot-5 (`ChunkGenerator`, `NoiseBasedChunkGenerator`) removes `getBaseHeight`; structures call `getFirstFreeHeight`, which reads the column (`getBaseColumn`, a `levelgen.NoiseColumn`):

- `WORLD_SURFACE` and `WORLD_SURFACE_WG`: one above the top block that isn't air (solid or fluid), `topBlockY() + 1`.
- Every other heightmap: one above the top **solid** block, `findTopSolidBlockY() + 1`. Fluids never count, so `MOTION_BLOCKING` and `MOTION_BLOCKING_NO_LEAVES` now ignore water and lava, where 26.3 counted them. No block tag is read.
- An empty column (no such block, or an empty y range) gives the column's minimum y, `max(noise.min_y, level min y)`, where 26.3 gives the level's. They are equal in the vanilla dimensions.
- `getFirstOccupiedHeight` is `getFirstFreeHeight − 1`; `getSpawnHeight` is still 64.

Vanilla structure code asks the generator for `WORLD_SURFACE_WG` and `OCEAN_FLOOR_WG`, whose answers don't change; that no other type reaches it is not confirmed in the bytecode, and a jigsaw's `project_start_to_heightmap` could name one in data.
**Heightmaps** (`Heightmap.Types`), as the column's blocks meet them:

- `WORLD_SURFACE_WG`: any block that isn't air. Water and lava count, so over an ocean the answer is the sea level.
- `OCEAN_FLOOR_WG`: blocks in `#minecraft:blocks_motion_in_heightmap`. 26.3 tests a block tag here, not `blocksMotion()`. In a column, only the default block (stone) is in it. The tag comes from the bundle's block tags ([bundle-format.md](bundle-format.md)), nested tags resolved.

_Versions:_ (switch `heightmap_blocks_motion`) 26.2 tests `blocksMotion()` for `OCEAN_FLOOR_WG` (and `MOTION_BLOCKING*`), not a tag; on a column's blocks (the default block, ore-vein blocks, water, lava, air) the answers are the same. 26.4-snapshot-5 counts every solid position, without the tag (above).

## The column

`iterateNoiseColumn(x, z)`:

1. The y range is the noise settings' range clamped to the level: from `max(noise.min_y, level min y)` up to, but not including, `min(noise.min_y + noise.height, level max y + 1)`. For the Overworld both are −64 to 319. If the range is empty, there's no column.
2. `final_density` is sampled over a 1 × H × 1 volume at `(x, z)`, the whole range at once ([density-functions.md](density-functions.md)).
3. From the top down, each block is `aquifer.computeSubstance(x, y, z, (double) density)`, or the default block if that returns nothing. A query that stops at the first block matching its heightmap returns `y + 1`.

So a column holds only the default block, the default fluid (from the [fluid picker](#the-fluid-picker)), lava and air. The density is an `f32`, widened to `f64` once, for the aquifer.

_Versions:_ 26.2 builds the column through a `NoiseChunk`, in `f64`, with ore veins ([below](#262-columns)). 26.4-snapshot-5 has no default block in the noise stage (`default_block` is gone from the noise settings). `getBaseColumn` creates a `NoiseChunk` over the same 1 × H × 1 volume (no beardifier, no blending) and calls `prepareColumn`:

1. `final_density` is sampled over the volume, as in 26.3.
2. Top-down, `findColumnMaxY` finds the highest y where `computeSubstance(x, y, z, (double) density)` isn't air.
3. Bottom-up to that y, each position is stored: `SOLID` (a marker, the game's `null`) where the aquifer returns nothing, else the fluid or air it returns.

The aquifer is asked twice for the blocks it visits on both passes. It doesn't depend on the order of questions, so the answers are 26.3's. A solid position gets its block only in chunk generation, from the dimension's material rule (`ChunkTerrainBuilder.fillColumn`); 26.4's `overworld`, `overworld_caves` and `overworld_floating_islands` material rules now end with stone for that, as `nether` ends with netherrack and `end` is end stone. A position the rule leaves empty stays air.

**The Nether and the End** use the same column, with legacy randomness ([density-functions.md](density-functions.md#binding-to-a-seed)) and no aquifers. Both noise ranges are 0 to 127 in a level of 0 to 255, so a column's blocks stop at 127, and above it there's air. The Nether's fluid is lava up to its sea level, 32 (y 31); the End's is air. So the Nether's `WORLD_SURFACE_WG` is its solid ceiling, 128 nearly everywhere, and the End's is 0 over the void.

**Regions.** Heights for many columns are computed chunk by chunk, as chunk generation does: `final_density` over each chunk's part of the box as one volume, and one aquifer for it. Each column gets the same blocks as on its own, because columns and chunks agree on density and the aquifer doesn't depend on its region ([below](#settled-questions)), and neighbouring columns share cell corners, surface levels and aquifer statuses, about 20 times less work for a 40 × 25 box.

_Versions:_ 26.2's aquifer exclusion test depends on the `NoiseChunk`'s flat-cache window, which differs between a chunk and a column query ([below](#262-aquifer)). To give the generator's column answers, a region must evaluate that test with each column's own window. spyglass computes 26.2's regions and grids cell by cell: the columns of one cell get the same one-cell `NoiseChunk` and the same aquifer from `iterateNoiseColumn`, so they share them, and surface levels are shared for the seed (they're the same through every `NoiseChunk`).

**Grids.** A sparse grid of columns, as a map samples one per pixel, is computed the same way: chunk by chunk, with each chunk's volume only as large as the box around its sampled columns. A box's density doesn't depend on where it starts, for the reason columns and chunks agree, so each column again gets its own answers.

**What the column leaves out.** It evaluates the density graph with no structures and no blending. `beardifier` is 0, `blend_density` passes its input through, `blend_alpha` is 1 and `blend_offset` 0. The material rule isn't evaluated at all, so the router's `chunk_surface_level` isn't used either.

_Versions:_ 26.2's column does include ore veins ([below](#262-columns)). 26.4-snapshot-5's column leaves solid positions without a block (above).

## The fluid picker

`createFluidPicker`, with the settings' `sea_level` and `default_fluid`:

- below `min(−54, sea_level)`: lava, at level −54;
- otherwise: the default fluid at the sea level.

A fluid status `(level, fluid)` gives the fluid below `level` and air at or above it.

## The aquifer

`Aquifer.NoiseBasedAquifer`, when the noise settings have `aquifers`. Otherwise (the Nether, the End) the aquifer is disabled: a positive density is the default block, and anything else is the fluid picker's status at y.

**Settings** (`aquifers`, each a density function): `barrier`, `fluid_level_floodedness`, `fluid_level_spread`, `lava`, `exclusion`, `surface_level`. They're bound like the router's functions and sampled at a point, as `f32` widened to `f64`, except where stated.

_Versions:_ 26.2 has no `aquifers` object: `aquifers_enabled` turns the aquifer on, its inputs are router fields sampled in `f64`, and the exclusion is a test in code ([below](#262-aquifer)). 26.4-snapshot-5's aquifer is 26.3's; the nothing it returns for solid is the `SOLID` marker, and the floodedness clamp uses `Math.clamp` (the same values for bounds ±1).

**Randomness.** The seed's positional factory (as for noises, [density-functions.md](density-functions.md#binding-to-a-seed)), then `fromHashOf("minecraft:aquifer").forkPositional()`. Each aquifer cell `(gx, gy, gz)` takes `at(gx, gy, gz)` and draws its centre's offsets as `nextInt(10)`, `nextInt(9)`, `nextInt(10)`, for x, y and z in that order.

**Grid.** Cells are 16 × 12 × 16 blocks: `gridX(x) = x >> 4`, `gridY(y) = floorDiv(y, 12)`, `gridZ(z) = z >> 4`. A cell's centre is `(16·gx + ox, 12·gy + oy, 16·gz + oz)`. A block looks at the cells from `gridX(x − 5)`, `gridY(y + 1) − 1`, `gridZ(z − 5)`, two along x, three along y and two along z.

**Region.** An aquifer is built for a region of blocks (a column, or a chunk), and remembers each cell's centre and status. Its cells run from `gridX(minX − 5)` to `gridX(maxX − 5) + 1`, `gridY(minY + 1) − 1` to `gridY(maxY + 1) + 1`, and likewise for z.

**Surface level.** `surfaceLevel(x, z)` snaps x and z down to multiples of 4, then takes `Mth.floor` of `surface_level` at `(x', 0, z')`, remembered. When the aquifer is built, the quart columns from `fromGridX(minCellX, 0)` to `fromGridX(maxCellX, 9)` (and z alike) are sampled at once, as a volume at y 0 with steps of 4, which fills those entries; others are sampled at a point when asked for. That agrees with point sampling, so results don't depend on the region (below). spyglass keeps the levels for the seed, shared by all its aquifers: an aquifer whose prefill box is all known samples nothing, which saves most of a nearby column's aquifer setup.

### `computeSubstance(x, y, z, density)`

Returns nothing (the default block) or a block state:

1. A positive density: nothing.
2. `g` = the fluid picker's status at the block. Above `skipSamplingAboveY` (below), return `g` at y. If `g` gives lava at y, return lava.
3. Find the nearest four of the 12 candidate centres, by squared integer distance, visiting x outermost, then y, then z. A centre takes the first slot whose distance is at least its own (`>=`), so on ties the later centre wins the better slot, pushing the others down. The fourth slot only decides when the game schedules fluid updates.
4. `s1` = the nearest centre's fluid status, and `state = s1` at y. With `similarity(a, b) = 1.0 − (double)(b − a) / 25.0`, if `similarity(d1, d2) ≤ 0`, return `state`.
5. If `state` is water and the fluid picker gives lava at `y − 1`, return `state`.
6. Otherwise, the barrier noise is sampled at most once per call, and only when a pressure needs it. Return nothing if `density + sim12 · pressure(s1, s2) > 0`; or, if `sim13 = similarity(d1, d3)` is positive, `density + (sim12 · sim13) · pressure(s1, s3) > 0`; or, if `sim23 = similarity(d2, d3)` is positive, `density + (sim12 · sim23) · pressure(s2, s3) > 0`, in that order. Else return `state`.

**Pressure** between statuses `a` and `b` at y:

- Lava against water at y (`a.at(y)` and `b.at(y)`, so a status whose level is at or below y is air there), either way round: 2.0.
- Equal levels: 0.0.
- Otherwise, with `avg = 0.5 · (double)(la + lb)`, `e = (double) y + 0.5 − avg`, and `o = (double)|la − lb| / 2.0 − |e|`:
  - if `e > 0`: `q = 0.0 + o`, and `r = q / 1.5` if positive, else `q / 2.5`;
  - otherwise: `q = 3.0 + o`, and `r = q / 3.0` if positive, else `q / 10.0`.
- If `r < −2` or `r > 2`, the barrier term `s` is 0; otherwise (NaN included) it's the barrier noise at the block. The pressure is `2.0 · (s + r)`.

### A centre's fluid status

`computeFluid(x, y, z)` at a cell's centre, with `g` the fluid picker's status there:

1. Visit 13 chunk offsets, in this order: (0, 0), (−2, −1), (−1, −1), (0, −1), (1, −1), (−3, 0), (−2, 0), (−1, 0), (1, 0), (−2, 1), (−1, 1), (0, 1), (1, 1). For each, `raw = surfaceLevel(x + 16·ox, z + 16·oz)` and `adj = raw + 8`.
   - At (0, 0), if `y − 12 > adj`, return `g`.
   - If `y + 12 > adj`, or at (0, 0): if the fluid picker's status at `(sx, adj, sz)` isn't air at `adj`, then (0, 0) marks the centre flooded, and `y + 12 > adj` returns that status.
   - Keep the minimum `raw` (not `adj`).
2. The level is `computeSurfaceLevel`, below, and the fluid is `computeFluidType`.

**`computeSurfaceLevel`:** if `exclusion` at the centre is positive, the level is far below the world (`−32512`). (_Versions:_ 26.2 tests the deep dark in code instead, [below](#262-aquifer).) Otherwise, with `dist = (min + 8) − y`:

- `f` = `clampedMap(dist, 0, 64, 1, 0)` if flooded, else 0;
- `n` = `floodedness` at the centre, clamped to −1..1;
- full threshold `n − map(f, 1, 0, −0.3, 0.8)`, partial threshold `n − map(f, 1, 0, −0.8, 0.4)`.

If the full threshold is positive, the level is `g`'s. Else, if the partial one is, it's randomized: `min(minSurface, 40·floorDiv(y, 40) + 20 + floor(v / 3.0)·3)`, where `v` is `fluid_level_spread` sampled at the **grid** position `(floorDiv(x, 16), floorDiv(y, 40), floorDiv(z, 16))`, times `10.0f` in `f32`, then widened. Else it's far below the world.

`Mth.map` is `lerp(inverseLerp(v, a, b), c, d)`, computed at run time in `f64` as written.

**`computeFluidType`:** `g`'s fluid, unless the level is at most −10, isn't the far-below level and `g` isn't already lava, and `|lava(floorDiv(x, 64), floorDiv(y, 40), floorDiv(z, 64))| > 0.3`: then lava.

### Skipping

`skipSamplingAboveY` is `12·(gridY(max + 20) + 1) + 11 − 1`, where `max` is the largest surface level in the aquifer's region. Above it, `computeSubstance` returns the fluid picker's status without visiting centres. The full path would give the same answer there, as long as no surface level is below −64: then every centre a block can see is above −54, and returns `g` at its first step. So the aquifer doesn't depend on its region, and a column answers as a chunk would. spyglass skips the same way, for speed, not for different results.

_Versions:_ 26.2 computes `skipSamplingAboveY` by the same formula, from the largest preliminary surface level over its region (the whole chunk, also for a column query: `maxPreliminarySurfaceLevel` over the region's grid, in steps of 4). The same argument holds for it. Its exclusion test still depends on the region ([below](#262-aquifer)).

## Settled questions

Open when this spec was drafted, and settled since:

- **Surface levels over a volume.** `surface_level` (a `find_top_surface`) gives the same value prefilled over the aquifer's region as at a point, so the aquifer doesn't depend on its region. Its volume path samples `upper_bound` over the volume and `density` at points, and 26.3's `upper_bound` holds no type whose volume and point values differ ([density-functions.md](density-functions.md#point-and-volume-evaluation)). An ignored test checks it over quart grids for three seeds.
- **Columns against chunks.** `final_density` gives each block the same value in a column volume as in a chunk volume: `interpolated`'s volume arithmetic depends on a block's offsets in its cell and on where the volume's lowest y falls in its cell, and both start at the noise settings' minimum y, a cell corner. The probe's column and chunk digests match spyglass's volume path for both ([oracle.md](oracle.md#probe-files)).

  _Versions:_ 26.4-snapshot-5 makes this simpler: `upscale`'s volume value depends only on a block's offsets in its cell, so any aligned box agrees ([density-functions.md](density-functions.md#upscale)). In 26.2 the final density is the same in a column and a chunk (it depends only on the offsets in the cell), but the aquifer's exclusion test isn't ([below](#262-aquifer)).

- **The lower bound.** `find_top_surface` never returns below its `lower_bound` (−64 in 26.3), which the skipping argument needs: it returns the lower bound or a y it reached from above without passing it.

## 26.2 terrain

From the 26.2 bytecode (`NoiseBasedChunkGenerator`, `NoiseChunk`, `Aquifer.NoiseBasedAquifer`, `OreVeinifier`, `OverworldBiomeBuilder`), and checked against the oracle: every block of 26.2's [aquifer probe cases](oracle.md#262s-probe-cases), and every height and block column of its height golden files, ore veins included. Density evaluation is [density-functions.md's 26.2 section](density-functions.md#262-evaluation). In `spyglass-terrain` it's its own path behind the same queries, chosen by the noise settings' shape (`noise.size_horizontal`).

**Settings.** 26.2's noise settings have `default_block` and `default_fluid` as block states (`{"Name": ..., "Properties": ...}`), `aquifers_enabled` and `ore_veins_enabled` instead of `aquifers` and the material rule's vein rules, and the fifteen-entry router ([density-functions.md](density-functions.md#262-router)). `OCEAN_FLOOR_WG` tests `blocksMotion()` (switch `heightmap_blocks_motion`). Legacy randomness with aquifers or ore veins isn't supported, as in 26.3; no vanilla dimension has both.

### 26.2 columns

`iterateNoiseColumn(x, z)`, for `getBaseHeight` and `getBaseColumn`:

1. The y range is clamped as in 26.3. With `cellHeight` and `cellWidth` from the noise settings' `size_vertical` and `size_horizontal`, `cellMinY = floorDiv(min y, cellHeight)` and `cellCountY = floorDiv(height, cellHeight)`; if that is 0 or less, there's no column. The column covers whole cells, `[cellMinY·cellHeight, (cellMinY + cellCountY)·cellHeight)`, the same range for the vanilla dimensions.
2. A one-cell `NoiseChunk` at `(floorDiv(x, cellWidth)·cellWidth, floorDiv(z, cellWidth)·cellWidth)`, with the beardifier 0 and no blending. Its aquifer is built for the chunk holding that corner (`SectionPos.blockToSectionCoord`), if `aquifers_enabled`; else it's the disabled aquifer.
3. From the top cell down, and in each cell from the top block down, at `y = (cellMinY + cellY)·cellHeight + dy`: the block is the first answer of `aquifer.computeSubstance(context, density)`, then, if `ore_veins_enabled`, the ore veinifier; if neither gives one, `default_block`. A query stops at the first block its heightmap counts and returns `y + 1`.

The density is the `NoiseChunk`'s interpolated `final_density` (plus the beardifier) in `f64`, passed to the aquifer without narrowing. It depends only on the block's offsets in its cell, so a column and a chunk agree.

**Ore veins.** The veinifier (`OreVeinifier.create(vein_toggle, vein_ridged, vein_gap, oreRandom)`) is asked only where the aquifer returned nothing. `oreRandom` is the seed's positional factory, then `fromHashOf("minecraft:ore").forkPositional()`, as for the aquifer. Its functions are evaluated with the positioned `NoiseChunk` as the context, so `vein_toggle`'s and `vein_ridged`'s `interpolated` take the incremental path ([density-functions.md](density-functions.md#262-noisechunk)). At block `(x, y, z)`, all in `f64`:

1. `t = vein_toggle`. The vein type is copper if `t > 0` (NaN is iron): copper is `copper_ore`, `raw_copper_block` and `granite` (its filler) from y 0 to 50; iron is `deepslate_iron_ore`, `raw_iron_block` and `tuff` from −60 to −8, both ends included. Outside the type's range: nothing.
2. With `e` the distance to the nearer end of the range, if `|t| + clampedMap(e, 0, 20, −0.2, 0) < (double) 0.4f`: nothing (a NaN sum passes, as `dcmpg` does).
3. `r = oreRandom.at(x, y, z)`. If `r.nextFloat() > 0.7f`: nothing.
4. If `vein_ridged ≥ 0`: nothing.
5. If `(double) r.nextFloat() < clampedMap(|t|, 0.4f, 0.6f, 0.1f, 0.3f)` (bounds widened floats) and `vein_gap > (double) −0.3f` (evaluated only if the first holds): the raw-ore block if `r.nextFloat() < 0.02f`, else the ore. Otherwise the filler.

So `vein_ridged` and `vein_gap` are evaluated only when reached. The types and their blocks are code (`OreVeinifier.VeinType`), not data. Every vein block is a full block that blocks motion, so heights don't change; only `getBaseColumn`'s solid blocks do. A height query can skip the veinifier.

The Nether and the End have neither aquifers nor ore veins (`aquifers_enabled` and `ore_veins_enabled` false). The End's cells are 8 × 4 × 8.

### 26.2 aquifer

The algorithm is 26.3's: grid, randomness, the nearest four centres and their slot rule, similarity, pressure, the barrier's laziness, `computeFluid`, `computeFluidType`, the fluid picker, the disabled aquifer and `skipSamplingAboveY`. The aquifer is built per chunk (a `ChunkPos` and the noise y range), also for a column query. The differences:

| Input         | 26.2                                                                                                                                                                                    |
| ------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| barrier       | the router's `barrier` at the block, `f64`, with the positioned `NoiseChunk` as the context (vanilla's is a plain noise)                                                                |
| floodedness   | `Mth.clamp(fluid_level_floodedness at SinglePointContext(x, y, z), −1.0, 1.0)`, `f64`                                                                                                   |
| spread        | `fluid_level_spread at SinglePointContext(floorDiv(x, 16), floorDiv(y, 40), floorDiv(z, 16)) · 10.0`, in `f64` (26.3: `· 10.0f` in `f32`, then widened)                                 |
| lava          | `abs(lava at SinglePointContext(floorDiv(x, 64), floorDiv(y, 40), floorDiv(z, 64))) > 0.3`, `f64`                                                                                       |
| surface level | `Mth.floor(preliminary_surface_level at SinglePointContext(x & ~3, 0, z & ~3))`, `f64`, remembered per `NoiseChunk`, with no prefill. It is the same at a point as anywhere, as in 26.3 |
| exclusion     | the deep-dark test below                                                                                                                                                                |

The functions are the router's as mapped by the `NoiseChunk` ([density-functions.md](density-functions.md#262-noisechunk)). The region's grid is the chunk's: x and z as for 26.3's region of the chunk's 16 × 16 blocks, and y from `gridY(minY + 1) − 1` to `gridY(minY + height + 1) + 1`, one above 26.3's top for the same range. That only sizes the caches. `skipSamplingAboveY` uses `adjustSurfaceLevel(maxPreliminarySurfaceLevel(...))`, which adds 8, as 26.3's formula does.

**The deep-dark test.** In `computeSurfaceLevel`, at the centre `(cx, cy, cz)`: `OverworldBiomeBuilder.isDeepDarkRegion(erosion, depth, SinglePointContext(cx, cy, cz))`, which is `erosion < (double) −0.225f && depth > (double) 0.9f` (`−0.22499999403953552`, `0.8999999761581421`), `erosion` first. If it holds, both thresholds are −1.0, so the level is far below the world, as 26.3's positive `exclusion` gives.

`erosion` and `depth` are the router's fields as mapped by the `NoiseChunk`: they hold the flat-cached `overworld/erosion` and `overworld/offset` (`depth` is a y gradient plus `offset`). So for a centre whose quart `(cx >> 2, cz >> 2)` is inside the `NoiseChunk`'s flat-cache window, erosion and offset are the values at `(4·(cx >> 2), 4·(cz >> 2))`; outside it they're exact. The window is blocks `[minX, minX + 19]` for a chunk and `[x0, x0 + 4·(cellWidth / 4 + 1) − 1]` (8 blocks in the Overworld) for a column query with cell corner `x0`, z alike. So a column query can answer differently from chunk generation for the same block, when a centre's quantized and exact values fall on opposite sides of −0.225 or 0.9. It's rare: building each column's aquifer on its chunk's `NoiseChunk` instead of the column's changes none of the 6.1 million values in 26.2's height golden files, so the oracle doesn't tell the two apart. spyglass uses the column's window, as the bytecode does. Dropping the test altogether changes 8 of the 320 aquifer probe cases and one block column.

## Testing

- **Unit tests** on hand-written density functions and aquifer settings: the slot order and ties, pressure cases, the fluid picker's boundaries, skipping against the full path, and answers independent of the order of questions. For 26.2: loading its settings, the deep-dark test's widened thresholds, the veinifier's ranges, draws and outcomes, and regions against columns.
- **The ignored aquifer test** (`spyglass-terrain/tests/probe.rs`) compares every block of the probe's aquifer columns: 26.3's `aquifer_column` cases, and 26.2's `noise_chunk_aquifer_column`, through the column's own `NoiseChunk`.
- **The ignored golden test** (`spyglass-terrain/tests/heights.rs`) compares every height in the [height golden files](oracle.md#height-golden-files) of all three dimensions, both heightmaps, computed by regions, every 13th by its own column, and every 5th column of every 5th row as a grid; and every sampled block column. Run by `cargo xtask oracle-test`.
