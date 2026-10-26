# Terrain

Status: stable. Covers what the generator answers about terrain before any chunk exists, in the Default preset's three dimensions, for 26.3, in `spyglass-terrain` ([ADR 0022](../adr/0022-terrain-crate-and-volume-evaluation.md)). Implemented, and checked against the oracle's [probe files](oracle.md#probe-files) (the aquifer) and [height golden files](oracle.md#height-golden-files) (every query). Ported from the 26.3 bytecode; worldgen before 26.x (surface rules, noise router fields, `blocksMotion`) doesn't apply.

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

**Heightmaps** (`Heightmap.Types`), as the column's blocks meet them:

- `WORLD_SURFACE_WG`: any block that isn't air. Water and lava count, so over an ocean the answer is the sea level.
- `OCEAN_FLOOR_WG`: blocks in `#minecraft:blocks_motion_in_heightmap`. 26.3 tests a block tag here, not `blocksMotion()`. In a column, only the default block (stone) is in it. The tag comes from the bundle's block tags ([bundle-format.md](bundle-format.md)), nested tags resolved.

## The column

`iterateNoiseColumn(x, z)`:

1. The y range is the noise settings' range clamped to the level: from `max(noise.min_y, level min y)` up to, but not including, `min(noise.min_y + noise.height, level max y + 1)`. For the Overworld both are −64 to 319. If the range is empty, there's no column.
2. `final_density` is sampled over a 1 × H × 1 volume at `(x, z)`, the whole range at once ([density-functions.md](density-functions.md)).
3. From the top down, each block is `aquifer.computeSubstance(x, y, z, (double) density)`, or the default block if that returns nothing. A query that stops at the first block matching its heightmap returns `y + 1`.

So a column holds only the default block, the default fluid (from the [fluid picker](#the-fluid-picker)), lava and air. The density is an `f32`, widened to `f64` once, for the aquifer.

**The Nether and the End** use the same column, with legacy randomness ([density-functions.md](density-functions.md#binding-to-a-seed)) and no aquifers. Both noise ranges are 0 to 127 in a level of 0 to 255, so a column's blocks stop at 127, and above it there's air. The Nether's fluid is lava up to its sea level, 32 (y 31); the End's is air. So the Nether's `WORLD_SURFACE_WG` is its solid ceiling, 128 nearly everywhere, and the End's is 0 over the void.

**Regions.** Heights for many columns are computed chunk by chunk, as chunk generation does: `final_density` over each chunk's part of the box as one volume, and one aquifer for it. Each column gets the same blocks as on its own, because columns and chunks agree on density and the aquifer doesn't depend on its region ([below](#settled-questions)), and neighbouring columns share cell corners, surface levels and aquifer statuses, about 20 times less work for a 40 × 25 box.

**Grids.** A sparse grid of columns, as a map samples one per pixel, is computed the same way: chunk by chunk, with each chunk's volume only as large as the box around its sampled columns. A box's density doesn't depend on where it starts, for the reason columns and chunks agree, so each column again gets its own answers.

**What the column leaves out.** It evaluates the density graph with no structures and no blending. `beardifier` is 0, `blend_density` passes its input through, `blend_alpha` is 1 and `blend_offset` 0. The material rule isn't evaluated at all, so the router's `chunk_surface_level` isn't used either.

## The fluid picker

`createFluidPicker`, with the settings' `sea_level` and `default_fluid`:

- below `min(−54, sea_level)`: lava, at level −54;
- otherwise: the default fluid at the sea level.

A fluid status `(level, fluid)` gives the fluid below `level` and air at or above it.

## The aquifer

`Aquifer.NoiseBasedAquifer`, when the noise settings have `aquifers`. Otherwise (the Nether, the End) the aquifer is disabled: a positive density is the default block, and anything else is the fluid picker's status at y.

**Settings** (`aquifers`, each a density function): `barrier`, `fluid_level_floodedness`, `fluid_level_spread`, `lava`, `exclusion`, `surface_level`. They're bound like the router's functions and sampled at a point, as `f32` widened to `f64`, except where stated.

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

**`computeSurfaceLevel`:** if `exclusion` at the centre is positive, the level is far below the world (`−32512`). Otherwise, with `dist = (min + 8) − y`:

- `f` = `clampedMap(dist, 0, 64, 1, 0)` if flooded, else 0;
- `n` = `floodedness` at the centre, clamped to −1..1;
- full threshold `n − map(f, 1, 0, −0.3, 0.8)`, partial threshold `n − map(f, 1, 0, −0.8, 0.4)`.

If the full threshold is positive, the level is `g`'s. Else, if the partial one is, it's randomized: `min(minSurface, 40·floorDiv(y, 40) + 20 + floor(v / 3.0)·3)`, where `v` is `fluid_level_spread` sampled at the **grid** position `(floorDiv(x, 16), floorDiv(y, 40), floorDiv(z, 16))`, times `10.0f` in `f32`, then widened. Else it's far below the world.

`Mth.map` is `lerp(inverseLerp(v, a, b), c, d)`, computed at run time in `f64` as written.

**`computeFluidType`:** `g`'s fluid, unless the level is at most −10, isn't the far-below level and `g` isn't already lava, and `|lava(floorDiv(x, 64), floorDiv(y, 40), floorDiv(z, 64))| > 0.3`: then lava.

### Skipping

`skipSamplingAboveY` is `12·(gridY(max + 20) + 1) + 11 − 1`, where `max` is the largest surface level in the aquifer's region. Above it, `computeSubstance` returns the fluid picker's status without visiting centres. The full path would give the same answer there, as long as no surface level is below −64: then every centre a block can see is above −54, and returns `g` at its first step. So the aquifer doesn't depend on its region, and a column answers as a chunk would. spyglass skips the same way, for speed, not for different results.

## Settled questions

Open when this spec was drafted, and settled since:

- **Surface levels over a volume.** `surface_level` (a `find_top_surface`) gives the same value prefilled over the aquifer's region as at a point, so the aquifer doesn't depend on its region. Its volume path samples `upper_bound` over the volume and `density` at points, and 26.3's `upper_bound` holds no type whose volume and point values differ ([density-functions.md](density-functions.md#point-and-volume-evaluation)). An ignored test checks it over quart grids for three seeds.
- **Columns against chunks.** `final_density` gives each block the same value in a column volume as in a chunk volume: `interpolated`'s volume arithmetic depends on a block's offsets in its cell and on where the volume's lowest y falls in its cell, and both start at the noise settings' minimum y, a cell corner. The probe's column and chunk digests match spyglass's volume path for both ([oracle.md](oracle.md#probe-files)).
- **The lower bound.** `find_top_surface` never returns below its `lower_bound` (−64 in 26.3), which the skipping argument needs: it returns the lower bound or a y it reached from above without passing it.

## Testing

- **Unit tests** on hand-written density functions and aquifer settings: the slot order and ties, pressure cases, the fluid picker's boundaries, skipping against the full path, and answers independent of the order of questions.
- **The ignored aquifer test** (`spyglass-terrain/tests/probe.rs`) compares every block of the probe's aquifer columns.
- **The ignored golden test** (`spyglass-terrain/tests/heights.rs`) compares every height in the [height golden files](oracle.md#height-golden-files) of all three dimensions, both heightmaps, computed by regions, every 13th by its own column, and every 5th column of every 5th row as a grid; and every sampled block column. Run by `cargo xtask oracle-test`.
