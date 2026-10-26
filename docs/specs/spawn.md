# World spawn

Status: stable. Covers the spawn origin of a noise-based dimension (the Default world preset's three), implemented in `spyglass-biome` for 26.3 and checked against the oracle's [spawn golden files](oracle.md#spawn-golden-files). The final spawn block needs generated blocks (the surface and features), not only the generator's heights, and is deferred to a later milestone ([roadmap](../roadmap/README.md)).

## How the game picks the spawn

`MinecraftServer.setInitialSpawn`, for a new world:

1. **The origin**: `ChunkGenerator.getOrigin(randomState)`, a chunk. It depends only on the seed and the climate. This spec covers it.
2. **The spawn block**: an 11 × 11 spiral of chunks around the origin, trying `PlayerSpawnFinder.getSpawnPosInChunk` in each, which reads generated blocks. If nothing fits, the fallback is the origin chunk's centre at `getSpawnHeight`, or the `WORLD_SURFACE` heightmap there. This needs terrain.

So the origin is exact now, and the spawn block is in the 11 × 11 chunks around it. In the four 26.3 worlds checked (the two Seed Check worlds and two others), the saved spawn was in the origin chunk itself.

## Spawn targets

The Nether's and the End's settings have no spawn targets, so their origin is chunk (0, 0) with no climate position; the spawn golden files agree.

`NoiseGeneratorSettings.spawn_target` is a required list of `SpawnTargetPoint`s. Each is a map from a density-function id to a `Climate.Parameter` (`Codec.unboundedMap(DensityFunction.REFERENCE_CODEC, Climate.Parameter.CODEC)`):

- Keys are registry references, so two spellings of one id (`overworld/ridges`, `minecraft:overworld/ridges`) are a duplicate key, which is an error.
- A parameter is a number, a `[min, max]` pair or a `{"min", "max"}` object (`ExtraCodecs.intervalCodec`). Each end is a float in `[-2, 2]`, `min` mustn't be above `max` by `Float.compareTo`, and each is quantized by `Climate.quantizeCoord` (`f32` multiply by 10000, then truncation). That's the game's own codec on data, unlike the [report values](data-sources.md#climate-parameter-values), which are rounded.

The functions are bound to the seed as the router's are ([density-functions.md](density-functions.md)). 26.3's overworld has two targets, over `continents`, `erosion`, `ridges`, `temperature` and `vegetation`: inland (continents −0.11 to 1), with ridges either −1 to −0.16 or 0.16 to 1.

`SpawnTargets::load(resources, noise_settings)` reads them; `BiomeSource::noise_settings` names the dimension's settings.

## The search

`NoiseBasedChunkGenerator.getOrigin`: with no targets, chunk (0, 0). Otherwise `NoiseSpawnFinder.findSpawnPosition`, then `ChunkPos.containing` (`x >> 4`, `z >> 4`).

**Fitness** at block `(x, z)` (`getSpawnPositionAndFitness`), all in wrapping `long` arithmetic:

1. Snap to the quart: `qx = (x >> 2) << 2`, likewise `qz`.
2. For each target, `sampleFitness`: the sum over its entries of `Mth.square(parameter.distance(quantizeCoord(f(qx, 0, qz))))`, with `f` evaluated as `f32`. Entry order doesn't matter: integer addition.
3. Climate fitness is the minimum over targets (starting from `Long.MAX_VALUE`).
4. The result is `climate * Mth.square(2048) + Mth.square(x) + Mth.square(z)` (x and z widened to `long`), at position `(x, 0, z)`: the unsnapped position.

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

## Testing

- **Unit tests** cover the data codec for parameters, loading errors, the fitness formula (quart snapping, the minimum over targets, wrapping), the search on hand-written gradients, and the trig flag.
- **The ignored golden test** `crates/spyglass-biome/tests/spawn.rs` compares against the [spawn golden files](oracle.md#spawn-golden-files): the origin chunk and the found position and fitness must be equal, each phase is rerun from the game's previous one and must be equal, and so must the fitness at every sampled position. Run it with `cargo test --release -p spyglass-biome --test spawn -- --ignored`. All 10 golden seeds match.
