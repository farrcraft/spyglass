# Carvers

Status: draft. Covers 26.3's, 26.4's and 26.2's carvers (`net.minecraft.world.level.levelgen.carver` and `NoiseBasedChunkGenerator.generateCarvers`): which carvers reach a chunk, the caves and canyons they carve into a mask, and how the mask is applied to the chunk after the [material rule](material.md). Implemented in `spyglass-blocks`. It's checked against the oracle's [block golden files](oracle.md#block-golden-files) with carvers: every block of every supported version's golden chunks matches, except the blocks that depend on a [tied biome](material.md#ties) and [one](#open-questions) in 26.2.

Ported from the 26.3, 26.4 and 26.2 bytecode.

## Data

A biome's `carvers` is a list of carvers: ids in `worldgen/carver`, inline carvers, or a `#tag` of `tags/worldgen/carver` (its ids in the order the tag first names them). A single id stands for a list of one. Its order matters: it numbers the carvers for their seeds.

**`cave`:**

| Field                              | Type                                                       |
| ---------------------------------- | ---------------------------------------------------------- |
| `probability`                      | float, 0 to 1                                              |
| `y`                                | [height provider](providers.md#height-providers)           |
| `count`                            | [int provider](providers.md#int-providers), at least 0     |
| `thickness`                        | [float provider](providers.md#float-providers), at least 0 |
| `weird_thickness_bias`             | bool, default false                                        |
| `room_vertical_radius_multiplier`  | float provider                                             |
| `horizontal_radius_multiplier`     | float provider                                             |
| `vertical_radius_multiplier`       | float provider                                             |
| `start_vertical_radius_multiplier` | float provider, default 1                                  |
| `floor_level`                      | float provider, within −1 to 1                             |

**`canyon`:** `probability`, `y`, `vertical_rotation` (float provider), and `shape`: `distance_factor`, `thickness`, `horizontal_radius_factor` and `y_scale` (float providers), `width_smoothness` (at least 1), `vertical_radius_default_factor` and `vertical_radius_center_factor` (floats).

The Nether's carver is a `cave` in 26.3.

## Which carvers reach a chunk

For chunk `C`, `generateCarvers` visits every chunk `S` within 8 chunks of it, x outer and z inner, from −8 to 8:

1. **Carver biome:** the biome source's biome at quart `(S.minX >> 2, 0, S.minZ >> 2)`, at y 0 and uncached (`getBiomeGenerationSettingsForCarver`). It isn't the biome the chunk stores there.
2. **Each carver `i` of that biome, in order:** a `WorldgenRandom` over a legacy random gets `setLargeFeatureSeed(seed + i, S.x, S.z)`. If `isStartChunk` (`nextFloat() ≤ probability`), the carver carves from `S` into `C`'s mask with the same random.

**The mask** (`CarvingMask`) covers `minY + 1` to `minY + genDepth − 1 − 7` of the generation context ([providers.md](providers.md#height-providers)), the chunk's 16 × 16 columns. A carver's ellipsoids never reach the mask's bottom y.

## Shapes

`Mth.sin` and `Mth.cos` read a table of 65,536 `(float) Math.sin(i / 10430.378350470453)`. In 26.3, `sin(x)` is the entry at `(int) ((long) (x · 10430.378350470453) & 65535)`, and `cos(x)` the same with 16,384 added before the cast. From 26.4 (switch `rounded_trig`), the index is rounded (`+ 0.5` before the cast), a negative `x` is taken as `−x` (with `sin` negated), and the table's entries 0, 16,384, 32,768 and 49,152 are exactly 0, 1, 0 and −1. Angles are `f32`, widened to `f64` for the table.

**`carveEllipsoid(C, x, y, z, h, v, skip)`:**

1. If `|x − C.middleX|` or `|z − C.middleZ|` is above `16 + 2h`, nothing. NaN goes on.
2. Local x from `max(floor(x − h) − C.minX − 1, 0)` to `min(floor(x + h) − C.minX, 15)`, and z likewise; y from `min(floor(y + v) + 1, mask max)` down to above `max(floor(y − v) − 1, mask min)`.
3. With `fx = (blockX + 0.5 − x) / h` and `fz` likewise, columns where `fx² + fz² ≥ 1` are left out (NaN stays in). In the others, each y with `fy = (y − 0.5 − y₀) / v` is carved unless `skip(fx, fy, fz, y)`.

**`canReach(C, x, z, step, count, thickness)`:** `dx² + dz² − (count − step)² ≤ (thickness + 2 + 16)²`, with `dx` and `dz` from `C`'s middle and the reach added in `f32`.

### Caves

`carve`, with `range = (4 · 2 − 1) · 16 = 112`:

1. `count` caves. Each draws, in order: `x = S.minX + nextInt(16)`, `y` from the height provider, `z = S.minZ + nextInt(16)`, then the horizontal, vertical, start vertical radius multipliers and the floor level. `skip` is `fy ≤ floor`, or `fx² + fy² + fz² ≥ 1`.
2. One tunnel. If `nextInt(4) == 0`, first a room: the room vertical multiplier `m`, `t = 1 + nextFloat() · 6`, and an ellipsoid at `(x + 1, y, z)` with `h = 1.5 + (double) (sin(1.5707963705062866) · t)` and `v = h · m`; then `nextInt(4)` more tunnels.
3. Each tunnel draws `yaw = nextFloat() · 6.2831855f`, `pitch = (nextFloat() − 0.5) / 4`, its thickness, `steps = 112 − nextInt(28)` and a seed `nextLong()`, and runs from step 0 with the start vertical multiplier.

**Thickness:** the provider's sample, times `nextFloat() · nextFloat() · 3 + 1` when `weird_thickness_bias` and `nextInt(10) == 0`.

**A tunnel** (`createTunnel`, from `step` to `count`, vertical scale `s`) draws from a legacy random of its seed: `split = nextInt(count / 2) + count / 4`, and `steep = nextInt(6) == 0`. Then at each step `i`:

1. `h = 1.5 + (double) (sin(π · i / count) · thickness)` and `v = h · s`.
2. It moves by `(cos(yaw) · cos(pitch), sin(pitch), sin(yaw) · cos(pitch))`, the products in `f32`.
3. `pitch` is multiplied by 0.92 if steep, else 0.7, then gets `pitchChange · 0.1`; `yaw` gets `yawChange · 0.1`; `pitchChange · 0.9` and `yawChange · 0.75`; then `pitchChange += (nextFloat() − nextFloat()) · nextFloat() · 2` and `yawChange += (…) · 4`.
4. At `i = split`, if `thickness > 1`: two tunnels from here, each drawing a seed `nextLong()` then a thickness `nextFloat() · 0.5 + 0.5`, at `yaw ∓ 1.5707964f` and `pitch / 3`, from step `i`, vertical scale 1. Then this one ends.
5. If `nextInt(4) != 0`: if it can't reach `C`, it ends; otherwise an ellipsoid with radii `h` and `v` times the horizontal and vertical multipliers.

### Canyons

`carve` draws `x`, `y`, `z` as caves do, `yaw = nextFloat() · 6.2831855f`, `pitch` from `vertical_rotation`, `y_scale`, `thickness`, `count = (int) (112 · distance_factor)`, then a seed `nextLong()`. From a legacy random of that seed (`doCarve`):

- **Width factors:** one per y of the generation depth, from `f = 1`: at y 0, and wherever `nextInt(width_smoothness) == 0`, `f = 1 + nextFloat() · nextFloat()`; the factor is `f²`.
- **Each step `i` from 0:** `h = 1.5 + (double) (sin(π · i / count) · thickness)`, `v = h · y_scale`, then `h` times `horizontal_radius_factor` and `v = (default + center · (1 − |0.5 − i / count| · 2)) · v · randomBetween(0.75, 1)`. It moves as a tunnel does, then `pitch · 0.7 + pitchChange · 0.05`, `yaw + yawChange · 0.05`, `pitchChange · 0.8`, `yawChange · 0.5`, and the same draws as a tunnel's. If `nextInt(4) != 0`: if it can't reach `C`, it ends; otherwise an ellipsoid, skipping where `(fx² + fz²) · widths[y − minY − 1] + fy² / 6 ≥ 1`.

## Applying the mask

`applyCarvingMask` visits the mask's runs of carved positions, columns by `(x << 4) + z`, and each run from the top down:

1. A position whose block is in `#minecraft:uncarvable` is left alone.
2. Carving grass or mycelium marks the run.
3. The position gets the aquifer's substance at density 0 (`computeSubstance`, the [terrain column's](terrain.md#the-column) aquifer): air, water or lava.
4. In a marked run, dirt under the position becomes the material rule's top material (`MaterialSystem.topMaterial`), if the rule gives one: the rule at the dirt with stone depths of 1 above and below, water above at `y + 1` if the substance is a fluid, and the chunk's current gradients.

**Ties.** Where a start chunk's carver biome is tied and its candidates have different carvers, every position any candidate's carvers carve, and the one below it, is marked uncertain, as are top materials whose biome is tied.

## Versions

- **26.4:** see [below](#264-the-carving-mask-with-the-rule).
- **26.2:** see [below](#262-carving-into-the-chunk).

### 26.4: the carving mask with the rule

The carvers, which reach a chunk and their shapes are 26.3's, with 26.4's `Mth.sin` and `Mth.cos` ([Shapes](#shapes)). The mask is built before the chunk is filled, and the [terrain builder](material.md#264) applies it with the rule: a carved solid position gets the aquifer's substance at density 0 unless the rule's block is `#uncarvable`, and stays the rule's block where the substance is solid. **Dirt under an opening:** carving the top block of stone (stone depth 1) to air or a fluid flags the column; the flag is kept through carved air and fluids and dropped at an uncarved one. At the next solid position that keeps the rule's block, a dirt block is tried again with a stone depth of 1, and the flag is cleared there either way. There's no top material as in 26.3.

**The mask's bounds:** from 1 above the bottom to 7 below the top, of the level (`LevelHeightAccessor`; switch `carving_mask_level_bounds`), where 26.4-snapshot-5 used the vertical anchors' context. In the Nether, whose noise is 128 tall in a 256-block level, snapshot-5's mask stops at y 120 and snapshot-6's at 248.

### 26.2: carving into the chunk

26.2's carvers are `ConfiguredWorldCarver`s in `worldgen/configured_carver`: a `type` (`cave`, `nether_cave` or `canyon`) and a `config` with `probability`, `y`, `yScale` (a float provider), `lava_level` (an anchor), `replaceable` (a block id, list or `#tag`) and an optional `debug_settings`, which only debug builds read. Caves add `horizontal_radius_multiplier`, `vertical_radius_multiplier` and `floor_level`; canyons `vertical_rotation` and a `shape` without `y_scale`. Which carvers reach a chunk, and their random, are 26.3's.

**Shapes.** 26.3 moved 26.2's code constants into data without changing a draw or a result, so 26.2's carvers are 26.3's shapes:

- **`cave`:** `count` is `nextInt(nextInt(nextInt(15) + 1) + 1)`, which is `very_biased_to_bottom` from 0 to 14; `thickness` is `nextFloat() · 2 + nextFloat()`, the `trapezoid` from 0 to 3 with plateau 1, with the weird thickness bias; `yScale` is the room vertical multiplier; the start vertical multiplier is 1.
- **`nether_cave`:** bound 10 (0 to 9), thickness `(nextFloat() · 2 + nextFloat()) · 2`, the trapezoid from 0 to 6 with plateau 2 (doubling commutes with `f32` rounding), no bias, and a start vertical multiplier of 5.
- **`canyon`:** the config's `yScale` is the shape's `y_scale`.

**Carving** happens as each ellipsoid reaches a position, straight into the chunk, in the order the carvers run. The chunk's carving mask only remembers positions:

1. A position already in the mask is skipped; otherwise it's added.
2. **`nether_cave`:** if the block is `replaceable`, it becomes lava at or below the generation context's bottom + 31, cave air above. Nothing else.
3. **Others:** grass or mycelium marks the ellipsoid's column. A block that isn't `replaceable` stays. The rest get lava at or below `lava_level`, else the aquifer's substance at density 0 (staying if that's solid). In a marked column, dirt below gets the rule's top material, as from 26.3, with 26.2's [surface rules](material.md#262-surface-rules).

There's no `#uncarvable`.

## Open questions

- **One 26.2 Nether block.** At seed −4893098694207956070, (−1000064, 64, −1000044), the game's chunk with carvers has air where spyglass has cave air. The game's own chunk without carvers and its `getBaseColumn` have netherrack there, and no 26.2 code path writes plain air in the Nether: its only carver fills with cave air or lava. Every other carved block of that chunk matches. The golden comparison names it as the only exception.

## Testing

`spyglass-blocks`' `material` test compares every block of the golden chunks with carvers. In 26.3's golden set it matches all 73.4 million blocks but 386 that depend on tied biomes, as the run without carvers does; 26.2's likewise, but for the [open question](#open-questions); both 26.4 snapshots' all of them.
