# The material rule

Status: draft. Covers 26.3's material rule (`net.minecraft.world.level.levelgen.material`), 26.4's terrain builder ([below](#264)) and 26.2's surface rules ([below](#262-surface-rules)): the blocks a chunk has after the noise fill and the rule, before carvers. Implemented in `spyglass-blocks` ([ADR 0050](../adr/0050-blocks-in-their-own-crate.md)). It's checked against the oracle's [block golden files](oracle.md#block-golden-files) without carvers: every block of every supported version's golden chunks matches, except the blocks that depend on a [tied biome](#ties).

Ported from the 26.3, 26.4 and 26.2 bytecode. Classes are in `net.minecraft.world.level.levelgen` unless stated otherwise.

## The chunk

A proto chunk over the level's height, as `buildTerrain` fills it from noise: each column is the [terrain column](terrain.md#the-column), with the default block where the column is solid. Air, water and lava come from the aquifer. That's what the chunk's own noise fill gives: the oracle's self-check confirms it at every golden block.

**The heightmap.** The chunk keeps `WORLD_SURFACE_WG`, which counts every block that isn't air. `getHeight(x, z)` is the top such block's y, or one below the level if there's none. It's primed after the fill. Each `setBlockState` then updates it as `Heightmap.update` does, with `first` the column's first available y (one above the top block):

- At or below `first − 2`: nothing changes.
- A block that isn't air, at or above `first`: `first` becomes `y + 1`.
- Air at `first − 1`, the top block: `first` becomes one above the next block down that isn't air, or the level's bottom.

**Block states.** In 26.3 and 26.4 the rule names blocks by id, which means each block's default state. 26.2's rules give `{"Name", "Properties"}` objects, as `BlockState`'s codec reads them. A state is named as the oracle's files name it: the id, then non-default properties in brackets, by name. Which properties are default comes from the bundle's `reports/blocks.json` ([bundle-format.md](bundle-format.md#layout)), read only when a state names properties. A block, property or value that the report doesn't have is an error. Air is `air`, `cave_air` or `void_air`. A fluid is `water` or `lava`; no state the vanilla rule places holds both a block and a fluid. A "stone" is a block that's neither.

## The material system

`RandomState` makes one `MaterialSystem` per seed:

- **Default block and sea level:** the noise settings' `default_block` and `sea_level`.
- **Preliminary surface:** the router's `chunk_surface_level`, bound to the seed like any density function.
- **Random:** `noiseRandom` is the seed's positional factory, `RandomState`'s `random`: Xoroshiro, or legacy with `legacy_random_source`. A named factory (`getOrCreateRandomFactory(name)`) is `random.fromHashOf(name).forkPositional()`.
- **Noises:** `RandomState.getOrCreateNoise` for each of `clay_bands_offset`, `surface`, `surface_secondary`, `badlands_pillar`, `badlands_pillar_roof`, `badlands_surface`, `iceberg_pillar`, `iceberg_pillar_roof` and `iceberg_surface`. Each 26.3 noise's `get` gives an `f32`, which the system widens to `f64`.
- **Clay bands:** 192 states, from `random.fromHashOf("minecraft:clay_bands")` ([below](#clay-bands)).

**Per-column values:**

- `surfaceDepth(x, z) = (int) ((double) surface(x, 0, z) · 2.75 + 3.0 + random.at(x, 0, z).nextDouble() · 0.25)`, with Java's truncating conversion.
- `surfaceSecondary(x, z) = (double) surface_secondary(x, 0, z)`.
- `minSurfaceLevel = floor((float) chunk_surface_level(x, 0, z)) + surfaceDepth − 8`.

### Clay bands

`generateBands(random)`:

1. All 192 bands are `terracotta`.
2. `i = 0`. While `i < 192`: `i += nextInt(5) + 1`, then if `i < 192`, band `i` is `orange_terracotta`, then `i += 1`.
3. `makeBands` with base 1 and `yellow_terracotta`, base 2 and `brown_terracotta`, base 1 and `red_terracotta`. Each makes `nextIntBetweenInclusive(6, 15)` runs. Each run has length `base + nextInt(3)` and starts at `nextInt(192)`, and is cut off at the end of the array.
4. `count = nextIntBetweenInclusive(9, 15)`, `placed = 0`, `i = 0`. While `placed < count` and `i < 192`:
   - band `i` is `white_terracotta`;
   - if `i − 1 > 0` and `nextBoolean()`, band `i − 1` is `light_gray_terracotta`;
   - if `i + 1 < 192` and `nextBoolean()`, band `i + 1` is `light_gray_terracotta`;
   - `placed += 1`, `i += nextInt(16) + 4`.

`nextIntBetweenInclusive(a, b)` is `nextInt(b − a + 1) + a`. The band at `(x, y, z)` is band `(y + round(clay_bands_offset(x, 0, z) · 4f) + 192) mod 192`, with `Math.round(float)`: halves up, NaN 0.

## Biomes

The rule reads biomes through `BiomeManager.getBiome(x, y, z)`, which picks one of the eight quarts around the block:

1. `obfuscatedSeed` is Guava's `Hashing.sha256().hashLong(seed).asLong()`: the first eight bytes, little-endian, of the SHA-256 of the seed's eight little-endian bytes ([ADR 0051](../adr/0051-sha-256-from-the-sha2-crate.md)).
2. With `i = x − 2`, `j = y − 2`, `k = z − 2`: the base quart is `(i >> 2, j >> 2, k >> 2)`, and the block's place in it is `((i & 3) / 4.0, (j & 3) / 4.0, (k & 3) / 4.0)`.
3. Corner `c` from 0 to 7 is the base quart plus 1 in x where `c & 4`, in y where `c & 2`, in z where `c & 1`. Its offset is the place, minus 1 on the same axes.
4. A corner's fiddled distance:
   - `l = obfuscatedSeed`, then `l = lcg(l, q)` for each of the corner's x, y, z, x, y, z, where `lcg(a, c) = a · (a · 6364136223846793005 + 1442695040888963407) + c`, wrapping.
   - Its x fiddle is `fiddle(l)`. Then `l = lcg(l, obfuscatedSeed)` for the y fiddle, and again for the z fiddle. `fiddle(l) = ((l >> 24) mod 1024 / 1024.0 − 0.5) · 0.9`, with a non-negative mod.
   - The distance is `(square(dz + fz) + square(dy + fy)) + square(dx + fx)`, in that order.
5. The quart is the corner with the smallest distance. A later corner replaces the best only if it's strictly closer.

The quart's biome is the chunk's generated biome there. The biome manager reads it through the region, with the quart's y clamped to the level's quarts (`ChunkAccess.getNoiseBiome`).

### Ties

A tied quart's biome in a real chunk depends on lookup history ([biome-lookup.md](biome-lookup.md#ties)). spyglass takes the tie's answer, the first in list order, and marks as uncertain every block whose block can depend on it:

- a block where the rule looked up its biome (a biome or temperature condition), and the quart was tied;
- every block of a column whose start biome ([below](#build-surface)) is tied, with eroded badlands, frozen ocean or deep frozen ocean among the candidates, since the extensions depend on it.

A neighbouring column's steepness can also depend on a pillar or iceberg that a tie decides. That isn't marked. The golden comparison checks that every difference is at a marked block. In 26.3's golden set, 22,241 of 73.4 million blocks are marked (0.03%), and 387 of them differ from the game.

## Temperature

The temperature condition and the iceberg melt use 26.3's `Biome.getTemperature(pos, seaLevel)` (`getHeightAdjustedTemperature`, without its cache), from the biome's `temperature` and `temperature_modifier`:

1. **The modifier:** `none` keeps the temperature. `frozen` gives `0.2` where `(double) (FROZEN_TEMPERATURE_NOISE(x · 0.05, z · 0.05) · 7f) + (double) BIOME_INFO_NOISE(x · 0.2, z · 0.2) < 0.3` and `BIOME_INFO_NOISE(x · 0.09, z · 0.09) < 0.8`. Otherwise it keeps the temperature.
2. **Height:** above `snowLine = seaLevel + 17`, the temperature falls by `(v + y − snowLine) · 0.05 / 40` in `f32`, with `v = TEMPERATURE_NOISE(x / 8f, z / 8f) · 8f`.

**The noises,** each from a `WorldgenRandom` over a `LegacyRandomSource`:

- `TEMPERATURE_NOISE` is `SimplexNoise(random(1234), true)`.
- `BIOME_INFO_NOISE` is `SimplexNoise(random(2345), true)`.
- `FROZEN_TEMPERATURE_NOISE` is a `NoiseStack` of three `SimplexNoise(random(3456), true)` from one random: frequencies 1, 0.5 and 0.25, amplitudes `0.14285715f`, `0.2857143f` and `0.5714286f`. Its 2D `get` sums `amplitude · noise.get(x · f, z · f)` in `f32`, in that order.

**The checks:**

- **Cold enough to snow:** a temperature that's NaN or below `0.15`.
- **Icebergs melt slightly:** a temperature above `0.1`.

## Rules

A rule gives the block for a position, or nothing. Rules and conditions come from `noise_settings.material_rule` and the `worldgen/material_rule` and `worldgen/material_condition` registries. Wherever a rule or condition is expected, a string is the id of a registry entry. Cycles are errors.

| Rule        | Fields                                                                                                                           | Gives                                                       |
| ----------- | -------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------- |
| `block`     | `result_state`                                                                                                                   | The state.                                                  |
| `sequence`  | `sequence`: at least one rule                                                                                                    | The first rule's block that isn't nothing.                  |
| `condition` | `if_true`, `then_run`                                                                                                            | `then_run`'s block where the condition holds, else nothing. |
| `bandlands` |                                                                                                                                  | The [clay band](#clay-bands) at the position.               |
| `ore_vein`  | `ore_block`, `raw_ore_block`, `filler_block`, `raw_ore_chance` (0 to 1), `density`, `richness`, `filler_gap` (density functions) | [Below](#ore-veins).                                        |

### Ore veins

At `(x, y, z)`, with `d = density(x, y, z)` and `r = getOrCreateRandomFactory("minecraft:ore").at(x, y, z)`:

1. If `d ≤ 0`: nothing. NaN goes on.
2. If `r.nextFloat() > d`: nothing.
3. If `r.nextFloat() < richness(x, y, z)` and `filler_gap(x, y, z) < 0`: `raw_ore_block` if `r.nextFloat() < raw_ore_chance`, else `ore_block`.
4. Otherwise `filler_block`.

The functions are evaluated as `f32`, bound to the seed with the dimension's [anchors](terrain.md#queries). The vein's height range and edge rounding are in the vanilla functions themselves (`overworld/ore_vein/*`), not in code.

### Conditions

`depth` is the [context](#build-surface)'s surface depth.

| Condition                   | Fields                                                                                        | Holds where                                                                                                                                                                          |
| --------------------------- | --------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `biome`                     | `biome_is`: an id or a list of ids                                                            | The block's [biome](#biomes) is one of them. Tags aren't supported yet; vanilla doesn't use them here.                                                                               |
| `noise_threshold`           | `noise`, `min_threshold`, `max_threshold`, `is_3d` (default false)                            | `min ≤ v ≤ max`, with `v` the noise at `(x, 0, z)`, or at `(x, y, z)` if `is_3d`, widened to `f64`. NaN never holds.                                                                 |
| `vertical_gradient`         | `random_name`, `true_at_and_below`, `false_at_and_above` (anchors)                            | Always at or below the first; never at or above the second; between them, where `factory(random_name).at(x, y, z).nextFloat() < map(y, a, b, 1, 0)`.                                 |
| `y_above`                   | `anchor`, `surface_depth_multiplier` (−20 to 20), `add_stone_depth`                           | `y + (stoneDepthAbove if add) ≥ anchor + depth · multiplier`.                                                                                                                        |
| `water`                     | `offset`, `surface_depth_multiplier` (−20 to 20), `add_stone_depth`                           | No water above (`waterHeight` is `Integer.MIN_VALUE`), or `y + (stoneDepthAbove if add) ≥ waterHeight + offset + depth · multiplier`.                                                |
| `temperature`               |                                                                                               | The biome is [cold enough to snow](#temperature) at the block.                                                                                                                       |
| `steep`                     |                                                                                               | `gradientX ≤ −4` or `gradientZ ≥ 4`.                                                                                                                                                 |
| `hole`                      |                                                                                               | `depth ≤ 0`.                                                                                                                                                                         |
| `above_preliminary_surface` |                                                                                               | `y ≥ minSurfaceLevel`.                                                                                                                                                               |
| `not`                       | `invert`                                                                                      | The condition doesn't hold.                                                                                                                                                          |
| `stone_depth`               | `offset`, `add_surface_depth`, `secondary_depth_range`, `surface_type` (`floor` or `ceiling`) | `d ≤ 1 + offset + (depth if add) + s`, with `d` the stone depth below for `ceiling`, else above, and `s = 0` if the range is 0, else `(int) map(surfaceSecondary, −1, 1, 0, range)`. |

`map(v, a, b, c, d) = c + (v − a) / (b − a) · (d − c)` (`Mth.map`). Anchors resolve against the dimension's heights, as density functions' do. The game caches conditions per column or per block; none of them has side effects, so the cache doesn't change results.

## Build surface

`MaterialSystem.buildSurface`, for each column, x outer and z inner (the order matters, since earlier columns' pillars and icebergs change later columns' gradients):

1. **Start:** `start = getHeight(x, z) + 1`, and the start biome is the biome at `(x, start, z)`.
2. **Pillar:** in eroded badlands, the [pillar](#eroded-badlands-pillars).
3. **Gradients:** `top = getHeight(x, z) + 1`, now. `gradientX = getHeight(min(x + 1, 15), z) − getHeight(max(x − 1, 0), z)`, and likewise for z, within the chunk.
4. **Down the column:** `stoneDepthAbove = 0`, `waterHeight = Integer.MIN_VALUE`, and `nextCeilingStoneY = Integer.MAX_VALUE`. From `top` down to the level's bottom, at each y:
   - **Air:** `stoneDepthAbove = 0` and `waterHeight = Integer.MIN_VALUE`.
   - **A fluid:** if `waterHeight` is unset, it becomes `y + 1`.
   - **Stone:**
     - If `nextCeilingStoneY ≥ y`, it's recomputed: going down from `y − 1` to one below the level, the first y that isn't stone, plus 1. If all of them are stone, it's `WAY_BELOW_MIN_Y` (−4064).
     - `stoneDepthAbove += 1`, and `stoneDepthBelow = y − nextCeilingStoneY + 1`.
     - Within the level, the rule's block, if any, replaces the block.
5. **Iceberg:** in frozen ocean or deep frozen ocean, by the start biome, the [iceberg](#frozen-ocean-icebergs), with the column's `minSurfaceLevel`.

### Eroded badlands pillars

All products in `f64` unless marked:

1. `surface = |(double) badlands_surface(x, 0, z) · 8.25|`, and `pillar = min(surface, (double) (badlands_pillar(x · 0.2, 0, z · 0.2) · 15f))`. If `pillar ≤ 0`, there's no pillar. NaN goes on.
2. `roof = |(double) badlands_pillar_roof(x · 0.75, 0, z · 0.75) · 1.5|`, and `top = floor(64 + min(pillar² · 2.5, ceil(roof · 50) + 24))`. If `start > top`, there's no pillar.
3. Going down from `top`: at the first default block, stop. At water first, there's no pillar.
4. From `top` down, while the block is air, it becomes the default block.

### Frozen-ocean icebergs

1. `surface = |(double) iceberg_surface(x, 0, z) · 8.25|`, and `pillar = min(surface, (double) (iceberg_pillar(x · 1.28, 0, z · 1.28) · 15f))`. If `pillar ≤ 1.8`, there's no iceberg.
2. `roof = |(double) iceberg_pillar_roof(x · 1.17, 0, z · 1.17) · 1.5|`, and `top = min(pillar² · 1.2, ceil(roof · 40) + 14)`. If the start biome [melts icebergs slightly](#temperature) at `(x, seaLevel, z)`, then `top −= 2`. If `top ≤ 2`, there's no iceberg.
3. `bottom = seaLevel − top − 7`, then `top += seaLevel`.
4. With `r = random.at(x, 0, z)`: `maxSnow = 2 + r.nextInt(4)`, `minSnowY = seaLevel + 18 + r.nextInt(10)`, and `snow = 0`.
5. From `max(start, (int) top + 1)` down to `minSurfaceLevel`, a block is placed:
   - over air, if `y < (int) top` and `r.nextDouble() > 0.01`;
   - or over water, if `(int) bottom < y < seaLevel` and `r.nextDouble() > 0.15`.

   The draws come only when the earlier parts hold. The block is `snow_block` while `snow ≤ maxSnow` and `y > minSnowY` (then `snow += 1`), otherwise `packed_ice`.

## Carvers

Carvers cut caves and canyons after the rule ([carvers.md](carvers.md)). Where they carve grass or mycelium, the dirt under it gets the rule's top material.

## Queries

`Session::chunk_blocks(chunk, carvers)` gives a chunk's blocks, each with whether it's [certain](#ties): with `Carvers::On`, after [carvers](carvers.md), with `Carvers::Off`, before. `Session::block_at` and `Session::block_column` generate the chunk around the block or column and answer from it. The CLI's `spyglass blocks` answers the same ([cli-json.md](cli-json.md#blocks)). The block settings are loaded on a world's first block query, and the seed's chunk generator is bound on a session's.

## Versions

### 26.4

Its noise settings have no `default_block`: the noise fill leaves solid positions undecided, and the rule decides them, with the [carving mask](carvers.md#264-the-carving-mask-with-the-rule) in the same pass. `NoiseBasedChunkGenerator.buildTerrain` builds the mask first, then `ChunkTerrainBuilder.fillChunk` fills the empty chunk column by column, z outer and x inner. The rule, its conditions, its noises and ore veins are 26.3's.

**Columns** (`NoiseChunk.prepareColumn`): the aquifer's answer at each y of the [terrain column](terrain.md#the-column), up to its top non-air y (`topIndexY`, −1 for an empty column): air, a fluid, or solid. Their **gradients** compare the chunk's columns' tops, before any pillar: `top(min(x + 1, 15), z) − top(max(x − 1, 0), z)`, and likewise for z, fluids counted. Nothing reads a heightmap: writes go straight into the chunk's sections.

**Each column:**

1. **Start:** one above the column's top. The start biome is the chunk's stored biome there (`ChunkAccess.getBiome`), at y clamped to the level, which is [the zoom's](#biomes) at that block.
2. **Pillar:** in eroded badlands, 26.3's [pillar](#eroded-badlands-pillars), as solid positions of the column: the scan for the default block stops at a solid position, and the fill marks air positions solid. The rule then decides them.
3. **Down the column** (`fillColumn`), with 26.3's context from the column's gradients, from the top index down:
   - **Air:** stone depth 0, no water above.
   - **A fluid:** the water height if unset; the fluid goes in, within the level.
   - **Solid:** the ceiling as 26.3's, by index (`getCeilingBelowIndex`; with none, `WAY_BELOW_MIN_Y` is taken as an index), stone depth + 1. Within the level, the rule's block, with `stoneDepthBelow = index − ceiling + 1`. If the position is [carved](carvers.md#264-the-carving-mask-with-the-rule) and the rule's block is none or isn't `#uncarvable`, the aquifer's substance at density 0 instead, unless that's solid. Where the rule leaves a position, it stays air.
4. **Iceberg:** in frozen ocean or deep frozen ocean, 26.3's [iceberg](#frozen-ocean-icebergs), from the start before the pillar, with the column's minimum surface level, placing nothing at carved positions.

The snapshots differ only in the carving mask's [bounds](carvers.md#264-the-carving-mask-with-the-rule).

### 26.2: surface rules

The 1.21 family's `SurfaceRules` and `SurfaceSystem` (ADR 0026), told apart by the noise settings' shape, as the terrain is ([terrain.md](terrain.md#262-terrain)). The rules are `noise_settings.surface_rule`, inline. Their rule and condition types and fields are 26.3's, without `ore_vein`, which is an error. Everything above holds, except:

- **Noises** are `NormalNoise`s in `f64` (`RandomState.getOrCreateNoise`), so nothing is narrowed: the clay band offset is `(int) Math.round(clay_bands_offset(x, 0, z) · 4.0)`, through `long`, and the pillar products are `f64`.
- **The preliminary surface** is the terrain's, interpolated over 16-block cells (`Context.getMinSurfaceLevel`). With `i = x >> 4` and `j = z >> 4`, it reads `NoiseChunk.preliminarySurfaceLevel` at the cell corners `(i·16, j·16)`, `((i+1)·16, j·16)`, `(i·16, (j+1)·16)` and `((i+1)·16, (j+1)·16)`: `floor` of the router's `preliminary_surface_level` at `(x, 0, z)` ([terrain.md](terrain.md#262-terrain)). `minSurfaceLevel = floor(lerp2((x & 15) / 16f, (z & 15) / 16f, c00, c10, c01, c11)) + surfaceDepth − 8`, with the fractions widened from `f32` and `lerp2` in `f64`.
- **The start biome** is the biome at `(x, 0, z)` under `legacy_random_source`, at `(x, start, z)` otherwise.
- **The rule** is tried only where the block is the default block itself, so the noise fill's ore veins stay. Veins still count as stone for the depths.
- **Steep** reads the gradients when it's first tested in a column, not before the column (`LazyXZCondition`). With vanilla's rules they're the same, since no rule places air, but a rule that does changes the heights in between.
- **Temperature** uses `PerlinSimplexNoise` ([noise.md](noise.md#262-simplex-octaves-perlinsimplexnoise)) for all three noises: `TEMPERATURE_NOISE` with octave 0 from legacy random 1234, `FROZEN_TEMPERATURE_NOISE` with octaves −2, −1 and 0 from 3456, and `BIOME_INFO_NOISE` with octave 0 from 2345. The frozen test is `FROZEN(x · 0.05, z · 0.05) · 7.0 + INFO(x · 0.2, z · 0.2) < 0.3` and `INFO(x · 0.09, z · 0.09) < 0.8`, all in `f64`. The height noise is `(float) (TEMPERATURE(x / 8f, z / 8f) · 8.0)`.
- **Icebergs:** where `top ≤ 2` (NaN included), there's no early return: `top` and `bottom` are both 0 and the loop runs from `max(start, 1)`. Air below y 0 can still become ice, and nothing is placed over water while `bottom` is 0.

## Testing

`spyglass-blocks`' `material` test compares every block of the golden chunks without carvers, which `cargo xtask oracle-test` runs for every version. In 26.2's golden set, 22,241 of 73.4 million blocks are marked as depending on a tie, and 387 of them differ from the game, as in 26.3's. In both 26.4 snapshots' sets, 515 are marked (ties are far rarer with half-open climate ranges) and none differ. Unit tests cover the zoom against Guava's hash, the heightmap's updates, block state names, the frozen modifier in both families, and `PerlinSimplexNoise`.
