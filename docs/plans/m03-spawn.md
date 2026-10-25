# m03. World spawn origin

- Status: Draft
- Milestone: [03. Structure placement and locate queries](../roadmap/03-structure-placement.md)
- Started: —
- Completed: —

## Goal

The world spawn's climate search for any seed, matching the oracle: the chunk the game starts its spawn search from. Covers the spawn half of the milestone's second exit criterion, as amended (the final block position needs terrain and moves to milestone 07).

The second of three plans for milestone 03, after [m03-placement](completed/m03-placement.md), whose sin/cos decision it reuses. Independent of it otherwise.

## Facts established up front

Checked against `bundles/26.3/` and the 26.3 jar (`javap`) on 2026-10-25.

- **The path.** `MinecraftServer.setInitialSpawn` takes `ChunkGenerator.getOrigin(randomState)` as a `ChunkPos`, then uses `getSpawnHeight` and the `WORLD_SURFACE` heightmap at the chunk centre as a fallback. It then searches an 11 × 11 spiral of chunks with `PlayerSpawnFinder.getSpawnPosInChunk`, which needs generated blocks. Only the origin is climate-only.
- **The search.** `NoiseSpawnFinder.findSpawnPosition(List<SpawnTargetPoint>, DensitySamplerSet)`: the fitness at (0, 0), then `radialSearch` with (2048, 512) and then (512, 32). Each step uses `Math.sin`/`Math.cos` of a `double` angle (2π), and `getSpawnPositionAndFitness` snaps x and z to quart positions (`QuartPos.fromBlock` then `toBlock`). It sums `SpawnTargetPoint.sampleFitness` (minimum over targets), plus a distance penalty with `Mth.square` and 2048, all as `long`.
- **Targets are data.** `SpawnTargetPoint` maps density functions to `Climate.Parameter`s, read from `spawn_target` in each `noise_settings/*.json`. `DensitySamplerSet.sampleValue(fn, x, y, z)` evaluates them as `f32`.

## Approach

1. Load `spawn_target` from the noise settings: density-function references and parameter ranges, quantized like the biome parameter list ([data-sources.md](../specs/data-sources.md#climate-parameter-values)).
2. Port `NoiseSpawnFinder` in `spyglass-biome` (it's climate fitness over the router's functions, which that crate already binds), with the sin/cos from [m03-placement](completed/m03-placement.md).
3. API: `spawn_origin()` returns the block position the search found and its chunk. Document that the world spawn itself is this chunk plus a terrain-dependent search, deferred to milestone 07.
4. Oracle: dump `getOrigin` per seed, and the final spawn too, so milestone 07 has its golden data.

## Tasks

- [ ] Disassemble `radialSearch`, `getSpawnPositionAndFitness` and `SpawnTargetPoint.sampleFitness` fully: the operation order, `float` vs `double`, the y used, and the comparisons on ties in fitness.
- [ ] Load spawn targets from noise settings. Unit tests on hand-written JSON.
- [ ] Port the search, with unit tests on a hand-built sampler.
- [ ] Oracle: `spawn <seed>` (origin and final spawn) and add both to `golden`.
- [ ] Ignored golden test: the origin equals the game's for every seed of the golden set.
- [ ] Spec: the spawn search, in `docs/specs/biome-lookup.md` or a new `docs/specs/spawn.md`, including what's deferred.
- [ ] `docs/specs/oracle.md`: the spawn golden file.

## Verification

- The golden test passes for all 10 seeds, including the two real worlds, whose saved spawn is in the chunk spiral around the origin (a sanity check, not a test).

## Durable outputs

- [ ] Spawn spec section or file
- [ ] `docs/specs/oracle.md`: spawn golden file

## Notes

- 2026-10-25: Plan drafted.
