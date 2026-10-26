# Terrain

Status: draft. Covers what the Overworld's generator answers about terrain before any chunk exists, for 26.3. To be implemented in `spyglass-terrain` ([ADR 0022](../adr/0022-terrain-crate-and-volume-evaluation.md)) and checked against the oracle's [height golden files](oracle.md#height-golden-files). Ported from the 26.3 bytecode; worldgen before 26.x (surface rules, noise router fields, `blocksMotion`) doesn't apply.

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
- `OCEAN_FLOOR_WG`: blocks in `#minecraft:blocks_motion_in_heightmap`. 26.3 tests a block tag here, not `blocksMotion()`. In a column, only the default block (stone) is in it.

## The column

`iterateNoiseColumn(x, z)`:

1. The y range is the noise settings' range clamped to the level: from `max(noise.min_y, level min y)` up to, but not including, `min(noise.min_y + noise.height, level max y + 1)`. For the Overworld both are −64 to 319. If the range is empty, there's no column.
2. `final_density` is sampled over a 1 × H × 1 volume at `(x, z)`, the whole range at once ([density-functions.md](density-functions.md)).
3. From the top down, each block is `aquifer.computeSubstance(x, y, z, (double) density)`, or the default block if that returns nothing. A query that stops at the first block matching its heightmap returns `y + 1`.

So a column holds only the default block, the default fluid (from the [fluid picker](#the-fluid-picker)), lava and air. The density is an `f32`, widened to `f64` once, for the aquifer.

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

**Surface level.** `surfaceLevel(x, z)` snaps x and z down to multiples of 4, then takes `floor((double) surface_level(x', 0, z'))`, at y 0. The game prefills these for its whole region with a volume sample at a step of 4. That must agree with point sampling for results not to depend on the region (open, below).

### `computeSubstance(x, y, z, density)`

Returns nothing (the default block) or a block state:

1. A positive density: nothing.
2. `g` = the fluid picker's status at the block. Above `skipSamplingAboveY` (below), return `g` at y. If `g` gives lava at y, return lava.
3. Find the nearest four of the 12 candidate centres, by squared integer distance, visiting x outermost, then y, then z. A centre takes a slot if its distance is at most the slot's (`>=`), so on ties the later centre wins the better slot, pushing the others down.
4. `s1` = the nearest centre's fluid status, and `state = s1` at y. With `similarity(a, b) = 1.0 − (double)(b − a) / 25.0`, if `similarity(d1, d2) ≤ 0`, return `state`.
5. If `state` is water and the fluid picker gives lava at `y − 1`, return `state`.
6. Otherwise, the barrier noise is sampled at most once per call, and only when a pressure needs it. Return nothing if `density + sim12 · pressure(s1, s2) > 0`, or, where the similarity is positive, `density + (sim12 · sim13) · pressure(s1, s3) > 0`, or `density + (sim12 · sim23) · pressure(s2, s3) > 0`, in that order. Else return `state`.

**Pressure** between statuses `a` and `b` at y:

- Lava against water, either way round: 2.0.
- Equal levels: 0.0.
- Otherwise, with `avg = 0.5 · (double)(la + lb)`, `e = (double) y + 0.5 − avg`, and `o = (double)|la − lb| / 2.0 − |e|`:
  - if `e > 0`: `q = 0.0 + o`, and `r = q / 1.5` if positive, else `q / 2.5`;
  - otherwise: `q = 3.0 + o`, and `r = q / 3.0` if positive, else `q / 10.0`.
- If `r` is outside −2 to 2, the barrier term `s` is 0; otherwise it's the barrier noise at the block. The pressure is `2.0 · (s + r)`.

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

## What's open

- **Volume against point.** That `final_density` gives each block the same value in a column volume as in a chunk volume, and that `surface_level` (a `find_top_surface`) gives the same value sampled over a volume as at a point. Both are needed for the oracle's heights, which come from columns, to be the heights chunks start from. [density-functions.md](density-functions.md) has the cell path.
- **The lower bound.** That `find_top_surface` never returns below its `lower_bound` (−64 in 26.3), which the skipping argument needs.

## Testing

- **Unit tests** on hand-written density functions and aquifer settings: the slot order and ties, pressure cases, the fluid picker's boundaries, skipping against the full path.
- **The ignored golden test** compares every height in the [height golden files](oracle.md#height-golden-files), both heightmaps, and every sampled block column, run by `cargo xtask oracle-test`.
