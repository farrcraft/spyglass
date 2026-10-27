# World spawn

Status: stable. Covers the spawn origin of a noise-based dimension (the Default world preset's three), implemented in `spyglass-biome` for 26.3 and checked against the oracle's [spawn golden files](oracle.md#spawn-golden-files). The final spawn block needs generated blocks (the surface and features), not only the generator's heights, and is deferred to a later milestone ([roadmap](../roadmap/README.md)). 26.2 and the 26.4 snapshots are implemented and checked against the oracle; their differences are in _Versions_ notes and [26.2](#262).

## How the game picks the spawn

`MinecraftServer.setInitialSpawn`, for a new world:

1. **The origin**: `ChunkGenerator.getOrigin(randomState)`, a chunk. It depends only on the seed and the climate. This spec covers it.
2. **The spawn block**: an 11 × 11 spiral of chunks around the origin, trying `PlayerSpawnFinder.getSpawnPosInChunk` in each, which reads generated blocks. If nothing fits, the fallback is the origin chunk's centre at `getSpawnHeight`, or the `WORLD_SURFACE` heightmap there. This needs terrain.

So the origin is exact now, and the spawn block is in the 11 × 11 chunks around it. In the four 26.3 worlds checked (the two Seed Check worlds and two others), the saved spawn was in the origin chunk itself.

_Versions:_ 26.2's `setInitialSpawn` gets the origin from `ChunkPos.containing(randomState.sampler().findSpawnPosition())` instead of `getOrigin` ([26.2](#262)); the rest, `PlayerSpawnFinder` included, is the same in all three versions. 26.4-snapshot-5's spawn classes are byte-identical to 26.3's, but its fitness changes through `Climate.Parameter.distance` ([The search](#the-search)).

## Spawn targets

The Nether's and the End's settings have no spawn targets, so their origin is chunk (0, 0) with no climate position; the spawn golden files agree.

`NoiseGeneratorSettings.spawn_target` is a required list of `SpawnTargetPoint`s. Each is a map from a density-function id to a `Climate.Parameter` (`Codec.unboundedMap(DensityFunction.REFERENCE_CODEC, Climate.Parameter.CODEC)`):

- Keys are registry references, so two spellings of one id (`overworld/ridges`, `minecraft:overworld/ridges`) are a duplicate key, which is an error.
- A parameter is a number, a `[min, max]` pair or a `{"min", "max"}` object (`ExtraCodecs.intervalCodec`). Each end is a float in `[-2, 2]`, `min` mustn't be above `max` by `Float.compareTo`, and each is quantized by `Climate.quantizeCoord` (`f32` multiply by 10000, then truncation). That's the game's own codec on data, unlike the [report values](data-sources.md#climate-parameter-values), which are rounded.

The functions are bound to the seed as the router's are ([density-functions.md](density-functions.md)). 26.3's overworld has two targets, over `continents`, `erosion`, `ridges`, `temperature` and `vegetation`: inland (continents −0.11 to 1), with ridges either −1 to −0.16 or 0.16 to 1.

`SpawnTargets::load(resources, noise_settings)` reads them; `BiomeSource::noise_settings` names the dimension's settings. Which shape it reads follows the settings' family (ADR 0026): 26.2's noise settings (`noise.size_horizontal`) hold 26.2's shape, so a 26.3-shaped target in them is an error, as in the game.

_Versions:_ 26.2's `spawn_target` is a list of `Climate.ParameterPoint`s (`ParameterPoint.CODEC.listOf()`), not maps: [26.2](#262).

## The search

`NoiseBasedChunkGenerator.getOrigin`: with no targets, chunk (0, 0). Otherwise `NoiseSpawnFinder.findSpawnPosition`, then `ChunkPos.containing` (`x >> 4`, `z >> 4`).

**Fitness** at block `(x, z)` (`getSpawnPositionAndFitness`), all in wrapping `long` arithmetic:

1. Snap to the quart: `qx = (x >> 2) << 2`, likewise `qz`.
2. For each target, `sampleFitness`: the sum over its entries of `Mth.square(parameter.distance(quantizeCoord(f(qx, 0, qz))))`, with `f` evaluated as `f32`. Entry order doesn't matter: integer addition.
3. Climate fitness is the minimum over targets (starting from `Long.MAX_VALUE`).
4. The result is `climate * Mth.square(2048) + Mth.square(x) + Mth.square(z)` (x and z widened to `long`), at position `(x, 0, z)`: the unsnapped position.

_Versions:_ (switch `climate_half_open`) in 26.4-snapshot-5, `SpawnTargetPoint.sampleFitness` is unchanged but calls the new `Climate.Parameter.distance`, so step 2 uses half-open ranges: `distance = (t − max) + 1` when that is positive, else `max(min − t, 0)` ([biome-lookup.md](biome-lookup.md#fitness)). A climate value exactly at a target's `max` is 1 away, not 0.

**Search**: the fitness at (0, 0), then `radialSearch(2048, 512)`, then `radialSearch(512, 32)`. `radialSearch(max, step)` from the current best position `(cx, cz)`:

```text
angle = 0f; radius = step                       // f32
while radius <= max:
    x = cx + (int) (Math.sin((double) angle) * (double) radius)
    z = cz + (int) (Math.cos((double) angle) * (double) radius)
    if fitness(x, z) < best: best = that         // strictly lower
    angle += step / radius                       // f32
    if (double) angle > 2π: angle = 0f; radius += step
```

The centre stays fixed for the whole call, even when the best moves. `(int)` truncates toward zero and saturates; additions wrap.

### Trig

The sample positions call `Math.sin` and `Math.cos`, whose results depend on the platform ([ADR 0012](../adr/0012-java-trig-results-that-depend-on-the-platform.md)). The rounding step is the `(int)` truncation of `trig(angle) * radius`.

- spyglass uses `f64::sin` and `f64::cos`, and a sample is **sensitive** if moving the trig value by up to 4 ulps either way changes its truncated product. At angle 0, sine is 0 and cosine 1 on every platform, so those samples are never sensitive. Without this exception every ring's first sample would be: `1.0 * radius` is exactly an integer.
- A sensitive sample changes nothing if neither its position nor its other possible position beats the best so far: every platform then passes through the same bests. Otherwise `SpawnOrigin::platform_dependent` is set.
- Measured on 26.3: none of the 10 golden seeds and none of 3,000 random seeds is flagged.

## 26.2

From the 26.2 bytecode (`Climate$SpawnFinder`, `Climate.Sampler`, `MinecraftServer.setInitialSpawn`). The search is 26.3's; the targets and the fitness have a different shape.

**Origin**: `ChunkPos.containing(sampler.findSpawnPosition())`, where `sampler` is the `RandomState`'s `Climate.Sampler`, which holds the settings' spawn targets. With no targets it returns block (0, 0, 0), so chunk (0, 0), as in 26.3. Otherwise it runs `Climate$SpawnFinder`: the fitness at (0, 0), `radialSearch(2048, 512)`, then `radialSearch(512, 32)`, exactly [as above](#the-search), with `java.lang.Math.sin` and `cos` (the [trig](#trig) analysis applies unchanged).

**Targets**: `NoiseGeneratorSettings.spawn_target` is a list of `Climate.ParameterPoint`s. Each has all six parameters, `temperature`, `humidity`, `continentalness`, `erosion`, `depth` and `weirdness`, each a `Climate.Parameter` as in 26.3 (number, pair or object, ends in `[-2, 2]`, quantized by `quantizeCoord`), and an `offset`, a float in `[0, 1]` by `Float.compareTo` (so `-0.0` is out of range), quantized the same way. Other fields are ignored (a record codec). 26.2's overworld has two, from `OverworldBiomeBuilder.spawnTarget()`: temperature, humidity and erosion `[-1, 1]`, continentalness `[-0.11, 1]`, depth `0`, weirdness `[-1, -0.16]` or `[0.16, 1]`, offset 0.

**Fitness** at block `(x, z)` (`getSpawnPositionAndFitness`), wrapping `long` arithmetic:

```text
t = sampler.sample(x >> 2, 0, z >> 2)          // all six, at block ((x >> 2) << 2, 0, (z >> 2) << 2)
t = TargetPoint(t.temperature, t.humidity, t.continentalness, t.erosion, 0, t.weirdness)
climate = Long.MAX_VALUE
for p in targets: climate = min(climate, p.fitness(t))
return climate * Mth.square(2048L) + Mth.square((long) x) + Mth.square((long) z), at (x, 0, z)
```

- `sample` is 26.2's `f64` evaluation narrowed to `f32`, then quantized ([biome-lookup.md](biome-lookup.md#climate-sampler)); the climate functions are the router's `temperature`, `vegetation`, `continents`, `erosion`, `depth` and `ridges`. Depth is evaluated and then discarded.
- `ParameterPoint.fitness` is the biome [fitness](biome-lookup.md#fitness): all six squared distances plus `Mth.square(offset)`. Every parameter counts, a full range `[-1, 1]` included, and depth is measured against 0, not the sampled depth.

spyglass loads these as `ParameterPoint`s and samples through the dimension's `ClimateSampler`, which for 26.2's settings is this `f64` evaluation; `SpawnOrigin::chunk` is `found >> 4`, which is `ChunkPos.containing`, and (0, 0) without targets. The radial search, the trig flag and the quart snap are shared with 26.3.

For 26.2's overworld targets this is 26.3's fitness: depth `0` is at distance 0 from a target of 0, the offset is 0, and the other five ranges are 26.3's entries. Only the sampled climate values differ, by the `f64` evaluation. That the router's functions are the ones 26.3's targets name is data, not confirmed in the bytecode.

## Testing

- **Unit tests** cover the data codec for parameters, loading errors, the fitness formula (quart snapping, the minimum over targets, wrapping), the search on hand-written gradients, and the trig flag; for 26.2, the `ParameterPoint` codec and its errors, depth measured against 0, the offset term and the minimum over targets, on a router of constants.
- **The ignored golden test** `crates/spyglass-biome/tests/spawn.rs` compares against the [spawn golden files](oracle.md#spawn-golden-files): the origin chunk and the found position and fitness must be equal, each phase is rerun from the game's previous one and must be equal, and so must the fitness at every sampled position. Run it with `cargo test --release -p spyglass-biome --test spawn -- --ignored`. All 10 golden seeds match, for 26.3, 26.4-snapshot-5 and 26.2, in all three dimensions (30 files per version); none is flagged platform-dependent.
