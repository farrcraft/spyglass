# m03. World spawn origin

- Status: Draft
- Milestone: [03. Structure placement and locate queries](../../roadmap/completed/03-structure-placement.md)
- Started: —
- Completed: —

## Goal

The world spawn's climate search for any seed, matching the oracle: the chunk the game starts its spawn search from. Covers the spawn half of the milestone's second exit criterion, as amended (the final block position needs terrain and moves to milestone 07).

The second of three plans for milestone 03, after [m03-placement](m03-placement.md), whose sin/cos decision it reuses. Independent of it otherwise.

## Facts established up front

Checked against `bundles/26.3/` and the 26.3 jar (`javap`) on 2026-10-25.

- **The path.** `MinecraftServer.setInitialSpawn` takes `ChunkGenerator.getOrigin(randomState)` as a `ChunkPos`, then uses `getSpawnHeight` and the `WORLD_SURFACE` heightmap at the chunk centre as a fallback. It then searches an 11 × 11 spiral of chunks with `PlayerSpawnFinder.getSpawnPosInChunk`, which needs generated blocks. Only the origin is climate-only.
- **The search.** `NoiseSpawnFinder.findSpawnPosition(List<SpawnTargetPoint>, DensitySamplerSet)`: the fitness at (0, 0), then `radialSearch` with (2048, 512) and then (512, 32). Each step uses `Math.sin`/`Math.cos` of a `double` angle (2π), and `getSpawnPositionAndFitness` snaps x and z to quart positions (`QuartPos.fromBlock` then `toBlock`). It sums `SpawnTargetPoint.sampleFitness` (minimum over targets), plus a distance penalty with `Mth.square` and 2048, all as `long`.
- **Targets are data.** `SpawnTargetPoint` maps density functions to `Climate.Parameter`s, read from `spawn_target` in each `noise_settings/*.json`. `DensitySamplerSet.sampleValue(fn, x, y, z)` evaluates them as `f32`.

## Approach

1. Load `spawn_target` from the noise settings: density-function references and parameter ranges. (Drafted as "quantized like the biome parameter list"; in fact the game's codec truncates, unlike the [report values](../../specs/data-sources.md#climate-parameter-values).)
2. Port `NoiseSpawnFinder` in `spyglass-biome` (it's climate fitness over the router's functions, which that crate already binds), with the sin/cos from [m03-placement](m03-placement.md).
3. API: `spawn_origin()` returns the block position the search found and its chunk. Document that the world spawn itself is this chunk plus a terrain-dependent search, deferred to milestone 07.
4. Oracle: dump `getOrigin` per seed, with the search's phases and sampled fitness. The final spawn needs a live `ServerLevel` (`PlayerSpawnFinder.getSpawnPosInChunk`), so it moves to milestone 07 with the rest of terrain.

## Tasks

- [x] Disassemble `radialSearch`, `getSpawnPositionAndFitness` and `SpawnTargetPoint.sampleFitness` fully: the operation order, `float` vs `double`, the y used, and the comparisons on ties in fitness. Recorded in [spawn.md](../../specs/spawn.md).
- [x] Load spawn targets from noise settings (`Parameter::from_codec`, `SpawnTargets::load`). Unit tests on hand-written JSON.
- [x] Port the search, with unit tests on hand-written gradients.
- [x] Oracle: `spawn <seed>` (origin, the search's phases and sampled fitness) and add it to `golden`. The final spawn is deferred (see Approach).
- [x] Ignored golden test (`crates/spyglass-biome/tests/spawn.rs`): the origin equals the game's for every seed of the golden set.
- [x] Spec: [spawn.md](../../specs/spawn.md), including what's deferred.
- [x] `docs/specs/oracle.md`: the spawn golden file.

## Verification

Done on 2026-10-25 for 26.3:

- The golden test passes for all 10 seeds: origin, found position and fitness, each phase rerun from the game's previous one, and the fitness at 12 sampled positions per seed.
- Unit tests: 6 in `spyglass_biome::spawn`, 1 in `spyglass_biome::parameters`.
- None of the 10 golden seeds and none of 3,000 random seeds is flagged platform-dependent. About 4 ms per seed, binding included.
- Real worlds: the saved spawn of all four local 26.3 worlds (the two Seed Check worlds and two others) is in the origin chunk itself.
- `cargo xtask check` passes.

## Durable outputs

- [x] [spawn.md](../../specs/spawn.md), listed in `docs/specs/README.md`
- [x] `docs/specs/oracle.md`: spawn golden files
- [x] `docs/architecture.md`: `spyglass-biome`'s role and dependencies
- [x] `docs/getting-started.md`: running the spawn comparison

## Notes

- 2026-10-25: Plan drafted.
- 2026-10-25: Started. 26.3's `spawn_target` is a list of maps from density-function ids to parameters (not parameter points), read by the game's own codec, so values are truncated by `quantizeCoord`, not rounded like the report's. The found position is the unsnapped `(x, 0, z)`; only sampling snaps to the quart.
- 2026-10-25: Trig: spawn truncates with `(int)` where rings round. At angle 0, `cos * radius` is exactly an integer, so a plain 4-ulp check would flag every ring's first sample; angle 0 is exact on every platform and is exempt. A sensitive sample only flags the result if it, or its other position, would beat the best.
- 2026-10-25: Matched the oracle on the first run. Completed.
