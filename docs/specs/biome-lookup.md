# Biome lookup

Status: stable. Covers the Default world preset's three dimensions, implemented in `spyglass-biome` for 26.3 and checked cell by cell against the oracle's [golden biome files](oracle.md#golden-biome-files). 26.2 and the 26.4 snapshots are implemented and checked against the oracle; their differences are in _Versions_ notes.

A multi-noise biome source (`MultiNoiseBiomeSource`) picks a biome at quart (1:4) resolution: it samples six climate values, quantizes them, and finds the nearest parameter point in the dimension's parameter list. Classes are in `net.minecraft.world.level.biome` unless stated otherwise.

_Versions:_ 26.4-snapshot-5 computes the same quart biomes but stores a chunk's biomes per block. A new `BIOMES` chunk status (`ChunkStatusTasks.upscaleBiomes`, `CachedChunkBiomeResolver`) applies `BiomeManager`'s zoom, seeded with `BiomeManager.obfuscateSeed(seed)`, to the quart biomes, so each stored block biome is what 26.3's `BiomeManager.getBiome` gives at that block. The arithmetic matches; that the neighbours' quart biomes are always still there when it runs is not confirmed in the bytecode. The quart lookup this spec describes is unchanged; how saves store biomes is in [world-saves.md](world-saves.md).

## Which source

`BiomeSource::load(bundle, preset, dimension)` follows the world preset, as a new world does:

1. `worldgen/world_preset/<preset>`: `dimensions.<dimension>.generator` must have `type: minecraft:noise`, a `biome_source` of `type: minecraft:multi_noise` with a `preset` or of `type: minecraft:the_end` ([below](#the-ends-source)), and `settings` naming noise settings by id. Inline settings and inline biome lists aren't supported yet.
2. `worldgen/multi_noise_biome_source_parameter_list/<preset>` is `{"preset": <id>}`. The parameter list itself is built in code (`MultiNoiseBiomeSourceParameterList.Preset`), so spyglass reads the data generator's report for that id: `reports/biome_parameters/<namespace>/<path>.json` ([data-sources.md](data-sources.md#data-generator)).
3. The climate router comes from the noise settings ([density-functions.md](density-functions.md#the-climate-sampler)).

`BiomeSource::overworld` is `minecraft:normal`, `minecraft:overworld`: noise settings `minecraft:overworld`, parameter list `minecraft:overworld`. The Nether is noise settings `minecraft:nether` and parameter list `minecraft:nether`; the End is noise settings `minecraft:end` and the End's source. Both bind with legacy randomness ([density-functions.md](density-functions.md#binding-to-a-seed)).

## The End's source

From the 26.3 bytecode (`TheEndBiomeSource`). The End's preset names biome source `minecraft:the_end`, with no fields. Its biomes, in order: `the_end`, `end_highlands`, `end_midlands`, `small_end_islands`, `end_barrens`. It has no parameter list and no ties.

For quart `(qx, qy, qz)`: `bx = qx << 2`, `bz = qz << 2`, section `sx = bx >> 4`, `sz = bz >> 4`.

- If `(long)sx² + (long)sz² ≤ 4096`, the biome is `the_end`.
- Otherwise `e` is the router's `erosion` (`end/islands`) at block `((sx·2 + 1)·8, qy << 2, (sz·2 + 1)·8)`, the point path, widened to `f64`. Then `e > 0.25` is `end_highlands`, else `e ≥ −0.0625` is `end_midlands`, else `e < −0.21875` is `small_end_islands`, else (and for NaN) `end_barrens`.

`end/islands` doesn't depend on y, so neither does the End's biome.

_Versions:_ in 26.2, `e` is `erosion.compute(new SinglePointContext(x, y, z))` at the same block, with the climate sampler's `erosion`: the `f64` value of the `f64` density evaluation, never narrowed (`ClimateSampler::v1_21`'s erosion function, not `sample`). The island test and thresholds are 26.3's, so the answer differs from narrowing only where a threshold falls between the `f64` value and its `f32` rounding. No golden End cell does: narrowing would also match all 42,598,400; a unit test covers the difference.

## The Nether

The Nether's source is `multi_noise` with the `minecraft:nether` preset, five biomes, read and searched as the Overworld's. Its router gives only `temperature` and `vegetation` ([noise.md](noise.md#the-nether-biome-noises)), with `y_scale` 0, and 0 for the other four, so its biomes don't depend on y either.

## Parameter list

The report is `{"biomes": [...]}`. Each entry is `{"biome": id, "parameters": {...}}` with `temperature`, `humidity`, `continentalness`, `erosion`, `depth` and `weirdness`, each a `[min, max]` pair or one number for both, and a number `offset`. 26.3's overworld list has 7,594 entries over 56 biomes. **List order matters**: it decides the answer for a tied cell (below).

Each value is recovered as its quantized long by round-to-nearest of `f32(value) × 10000` in `f64`, never by the game's truncating `quantizeCoord` ([data-sources.md](data-sources.md#climate-parameter-values)). The loader checks the codec's ranges: parameters in `[-2, 2]` with `min <= max`, offset in `[0, 1]`.

_Versions:_ the lists differ only in data, which the report carries; the point shape and the report format are the same in all three versions. 26.2's `OverworldBiomeBuilder` has an empty slot where 26.3's has `dappled_forest`. 26.4-snapshot-5's `addUndergroundBiomes` narrows `dripstone_caves`' and `sulfur_caves`' humidity to `[-1, 0.7]` and appends `ice_caves` after `deep_dark`. 26.4-snapshot-5 also adds a codec check to inline parameter lists (`ParameterList.validate`): two entries with different biomes, equal offsets and all six parameters overlapping are an error, where ranges overlap if equal or if `max(min₁, min₂) < min(max₁, max₂)`, so touching ranges don't. spyglass reads the report, not that codec.

## Climate sampler

`Climate.Sampler.sample(qx, qy, qz)` evaluates the router's six climate functions at block `(4·qx, 4·qy, 4·qz)` and quantizes each with `quantizeCoord(f) = (long)(f · 10000.0f)` ([density-functions.md](density-functions.md#the-climate-sampler)). The router's `vegetation` feeds humidity and `ridges` feeds weirdness. A `Climate.TargetPoint` holds the six longs in the order temperature, humidity, continentalness, erosion, depth, weirdness.

_Versions:_ 26.4-snapshot-5's sampler is byte-identical to 26.3's. 26.2's `Climate.Sampler.sample` evaluates each function in `f64` and narrows it once:

```text
ctx = new SinglePointContext(4·qx, 4·qy, 4·qz)
target = Climate.target((float) temperature.compute(ctx), (float) humidity.compute(ctx),
                        (float) continentalness.compute(ctx), (float) erosion.compute(ctx),
                        (float) depth.compute(ctx), (float) weirdness.compute(ctx))
```

`compute` is the `f64` density evaluation ([density-functions.md](density-functions.md)), with the router's cache markers and holders stripped by `RandomState`. The functions, the block, the order and `quantizeCoord` are 26.3's. spyglass's `ClimateSampler` for 26.2's noise settings does this ([ADR 0026](../adr/0026-how-game-versions-coexist.md)). 26.2's chunk generation samples through `NoiseChunk.cachedClimateSampler`, which flat-caches values at y = 0; that it gives the same values as the point path, since only depth's y gradient depends on y and it isn't cached, is not confirmed in the bytecode.

## Fitness

`Climate.Parameter.distance(t)` is `t - max` above the range, `min - t` below it, and 0 inside. `Climate.ParameterPoint.fitness(target)` is the sum of the six squared distances (`Mth.square(long)`) plus the squared offset, in wrapping `long` arithmetic. The offset acts as a seventh distance against a target of 0, as in `ParameterPoint.parameterSpace` and `TargetPoint.toParameterArray`.

The biome is the one whose parameter point has the minimum fitness.

_Versions:_ (switch `climate_half_open`) 26.4-snapshot-5's `Climate.Parameter.distance` adds 1 above the range:

```text
above = (t − max) + 1                 // wrapping long
below = min − t
distance = above > 0 ? above : Math.max(below, 0)
```

So ranges are half-open, `[min, max)`: `t == max` is 1 (was 0), `t > max` is `t − max + 1`, and below and inside are unchanged. A single-number parameter (`min == max`) is never at distance 0. The offset term doesn't call `distance` and is unchanged. 26.2's `distance` is 26.3's.

## Ties

Several points can share the minimum. Points of the same biome tying isn't a tie. Points of different biomes are: the game's `Climate.RTree` warm-starts each search from the thread's previous result and only replaces it with a strictly closer point, so its answer depends on lookup history ([oracle.md](oracle.md#comparing-biomes-exact-except-ties)). About 1 cell in 1,200 is tied in the golden set.

spyglass searches exactly and returns a `Lookup`:

- `Unique(biome)`: one biome at the minimum.
- `Tied(set)`: two or more biomes at the minimum, in list order of each biome's first point at the minimum.

**The answer for a tied cell is the biome of the first point at the minimum, in list order.** That's what `Climate.ParameterList.findValueBruteForce` returns, and it's deterministic and independent of threads and lookup order. Any member of the set is what the game might show; `Lookup::is_tied` lets front ends say so.

_Versions:_ 26.2's `Climate.RTree` has 6 children per node, 26.3's and 26.4-snapshot-5's 19 (`ParameterList` passes the count). The tree's shape can change which tied biome the game's warm-started search returns, never a unique cell's, so spyglass's tie sets and tie answer need no change. 26.2's golden tie sets match exactly (60,434 tied cells). In 26.4-snapshot-5, with half-open ranges (switch `climate_half_open`), a value on the boundary between two neighbouring ranges is in only one of them, so ties are far rarer: 1,008 tied cells in the golden set against 60,361 for 26.3, about 1 cell in 70,000.

## Search

The reference is brute force over every point with `ParameterPoint::fitness`. The fast search is a bounding-box tree over the seven dimensions (as the oracle's `TieSearch`, not `Climate.RTree`):

- Leaves hold up to 8 points. Each inner node splits its points at the median, along the dimension (sorted by box centre) that minimises the children's total box span. The split only affects speed.
- The search visits the nearer child first and **prunes only strictly farther boxes**, since a box at exactly the best distance can still hold a tie. It collects every point at the minimum.
- Box distances use the same arithmetic as fitness, which can only prune correctly without overflow. When any target value is outside `±2^30`, the search falls back to brute force. Climate values never come near that.

_Versions:_ for 26.4-snapshot-5, box distances use its `distance` with the `+1` ([Fitness](#fitness)), as `Climate.RTree$Node.distance` does; the `±2^30` fallback still holds.

The tree and brute force are compared on random lists with frequent ties, and on every climate target in the probe files.

## Region fill

`Biomes::biomes_in(min, size)` returns the box's lookups, x fastest, then z, then y. It goes column by column, so each column's y-independent climate is computed once ([density-functions.md](density-functions.md#remembered-values)). That's exact, so the region fill and `biome_at` give the same answer on every cell. Tests check this on random regions and on a sample of golden cells.

`Biomes::biome_grid(origin, qy, step, size)` returns the lookups on a horizontal grid of quarts `step` apart at one height, x fastest, for map rendering ([render.md](render.md)). Each cell is `biome_at`'s answer.

In 26.3's overworld, five climate functions don't depend on y at all, and depth is a y gradient plus a y-independent offset, so a column costs the 2D work once.

## Testing

Committed tests use hand-written reports ([ADR 0008](../adr/0008-game-derived-test-values-stay-local.md)): quantization round trips, fitness, ties and the tie answer, and tree against brute force. The ignored tests in `spyglass-biome/tests/golden.rs` check every cell of every golden file by the tie rule, in all three dimensions. For 26.3: the Overworld's 63,897,600 cells (52,297 tied), the Nether's 42,598,400 (8,064 tied) and the End's 42,598,400, all equal. For 26.2 the same counts with 52,370 tied in the Overworld and 8,064 in the Nether, all equal; for 26.4-snapshot-5, 1,008 and none. They also require the tie set to equal the golden set exactly, and the number of tied cells to match. Further ignored tests compare the tree with brute force on the probed climate targets, and the region fill with `biome_at`. See [oracle.md](oracle.md#rust-side-comparison) for running them.
