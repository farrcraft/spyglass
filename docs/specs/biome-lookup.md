# Biome lookup

Status: stable. Covers the Overworld of the Default world preset, implemented in `spyglass-biome` for 26.3 and checked cell by cell against the oracle's [golden biome files](oracle.md#golden-biome-files).

A multi-noise biome source (`MultiNoiseBiomeSource`) picks a biome at quart (1:4) resolution: it samples six climate values, quantizes them, and finds the nearest parameter point in the dimension's parameter list. Classes are in `net.minecraft.world.level.biome` unless stated otherwise.

## Which source

`BiomeSource::load(bundle, preset, dimension)` follows the world preset, as a new world does:

1. `worldgen/world_preset/<preset>`: `dimensions.<dimension>.generator` must have `type: minecraft:noise`, a `biome_source` of `type: minecraft:multi_noise` with a `preset`, and `settings` naming noise settings by id. Inline settings and inline biome lists aren't supported yet.
2. `worldgen/multi_noise_biome_source_parameter_list/<preset>` is `{"preset": <id>}`. The parameter list itself is built in code (`MultiNoiseBiomeSourceParameterList.Preset`), so spyglass reads the data generator's report for that id: `reports/biome_parameters/<namespace>/<path>.json` ([data-sources.md](data-sources.md#data-generator)).
3. The climate router comes from the noise settings ([density-functions.md](density-functions.md#the-climate-sampler)).

`BiomeSource::overworld` is `minecraft:normal`, `minecraft:overworld`: noise settings `minecraft:overworld`, parameter list `minecraft:overworld`.

## The End's source

Draft, from the 26.3 bytecode (`TheEndBiomeSource`). The End's preset names biome source `minecraft:the_end`, with no fields. Its biomes, in order: `the_end`, `end_highlands`, `end_midlands`, `small_end_islands`, `end_barrens`. It has no parameter list and no ties.

For quart `(qx, qy, qz)`: `bx = qx << 2`, `bz = qz << 2`, section `sx = bx >> 4`, `sz = bz >> 4`.

- If `(long)sx² + (long)sz² ≤ 4096`, the biome is `the_end`.
- Otherwise `e` is the router's `erosion` (`end/islands`) at block `((sx·2 + 1)·8, qy << 2, (sz·2 + 1)·8)`, the point path, widened to `f64`. Then `e > 0.25` is `end_highlands`, else `e ≥ −0.0625` is `end_midlands`, else `e < −0.21875` is `small_end_islands`, else (and for NaN) `end_barrens`.

`end/islands` doesn't depend on y, so neither does the End's biome.

## The Nether

The Nether's source is `multi_noise` with the `minecraft:nether` preset, five biomes, read and searched as the Overworld's. Its router gives only `temperature` and `vegetation` ([noise.md](noise.md#the-nether-biome-noises)), with `y_scale` 0, and 0 for the other four, so its biomes don't depend on y either.

## Parameter list

The report is `{"biomes": [...]}`. Each entry is `{"biome": id, "parameters": {...}}` with `temperature`, `humidity`, `continentalness`, `erosion`, `depth` and `weirdness`, each a `[min, max]` pair or one number for both, and a number `offset`. 26.3's overworld list has 7,594 entries over 56 biomes. **List order matters**: it decides the answer for a tied cell (below).

Each value is recovered as its quantized long by round-to-nearest of `f32(value) × 10000` in `f64`, never by the game's truncating `quantizeCoord` ([data-sources.md](data-sources.md#climate-parameter-values)). The loader checks the codec's ranges: parameters in `[-2, 2]` with `min <= max`, offset in `[0, 1]`.

## Climate sampler

`Climate.Sampler.sample(qx, qy, qz)` evaluates the router's six climate functions at block `(4·qx, 4·qy, 4·qz)` and quantizes each with `quantizeCoord(f) = (long)(f · 10000.0f)` ([density-functions.md](density-functions.md#the-climate-sampler)). The router's `vegetation` feeds humidity and `ridges` feeds weirdness. A `Climate.TargetPoint` holds the six longs in the order temperature, humidity, continentalness, erosion, depth, weirdness.

## Fitness

`Climate.Parameter.distance(t)` is `t - max` above the range, `min - t` below it, and 0 inside. `Climate.ParameterPoint.fitness(target)` is the sum of the six squared distances (`Mth.square(long)`) plus the squared offset, in wrapping `long` arithmetic. The offset acts as a seventh distance against a target of 0, as in `ParameterPoint.parameterSpace` and `TargetPoint.toParameterArray`.

The biome is the one whose parameter point has the minimum fitness.

## Ties

Several points can share the minimum. Points of the same biome tying isn't a tie. Points of different biomes are: the game's `Climate.RTree` warm-starts each search from the thread's previous result and only replaces it with a strictly closer point, so its answer depends on lookup history ([oracle.md](oracle.md#comparing-biomes-exact-except-ties)). About 1 cell in 1,200 is tied in the golden set.

spyglass searches exactly and returns a `Lookup`:

- `Unique(biome)`: one biome at the minimum.
- `Tied(set)`: two or more biomes at the minimum, in list order of each biome's first point at the minimum.

**The answer for a tied cell is the biome of the first point at the minimum, in list order.** That's what `Climate.ParameterList.findValueBruteForce` returns, and it's deterministic and independent of threads and lookup order. Any member of the set is what the game might show; `Lookup::is_tied` lets front ends say so.

## Search

The reference is brute force over every point with `ParameterPoint::fitness`. The fast search is a bounding-box tree over the seven dimensions (as the oracle's `TieSearch`, not `Climate.RTree`):

- Leaves hold up to 8 points. Each inner node splits its points at the median, along the dimension (sorted by box centre) that minimises the children's total box span. The split only affects speed.
- The search visits the nearer child first and **prunes only strictly farther boxes**, since a box at exactly the best distance can still hold a tie. It collects every point at the minimum.
- Box distances use the same arithmetic as fitness, which can only prune correctly without overflow. When any target value is outside `±2^30`, the search falls back to brute force. Climate values never come near that.

The tree and brute force are compared on random lists with frequent ties, and on every climate target in the probe files.

## Region fill

`Biomes::biomes_in(min, size)` returns the box's lookups, x fastest, then z, then y. It goes column by column, so each column's y-independent climate is computed once ([density-functions.md](density-functions.md#remembered-values)). That's exact, so the region fill and `biome_at` give the same answer on every cell. Tests check this on random regions and on a sample of golden cells.

`Biomes::biome_grid(origin, qy, step, size)` returns the lookups on a horizontal grid of quarts `step` apart at one height, x fastest, for map rendering ([render.md](render.md)). Each cell is `biome_at`'s answer.

In 26.3's overworld, five climate functions don't depend on y at all, and depth is a y gradient plus a y-independent offset, so a column costs the 2D work once.

## Testing

Committed tests use hand-written reports ([ADR 0008](../adr/0008-game-derived-test-values-stay-local.md)): quantization round trips, fitness, ties and the tie answer, and tree against brute force. The ignored tests in `spyglass-biome/tests/golden.rs` check every cell of every golden file by the tie rule. They also require the tie set to equal the golden set exactly, and the number of tied cells to match. Further ignored tests compare the tree with brute force on the probed climate targets, and the region fill with `biome_at`. See [oracle.md](oracle.md#rust-side-comparison) for running them.
