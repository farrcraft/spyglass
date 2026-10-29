# m14. Viewer performance

- Status: Draft
- Milestone: [14. Viewer performance](../roadmap/14-viewer-performance.md)
- Started:
- Completed:

## Goal

Make the map's detailed tiles arrive sooner, measured against a baseline, without changing a result. Covers every exit criterion of milestone 14.

Milestone 13 made the wait look better: coarse tiles first fill a cold window at 1:4 with biomes at the surface in about 5 seconds. The detailed tiles still take 20 to 40 seconds for a full window, and stall while a search runs. That's the time this plan goes after.

## Approach

Three phases, each finished before the next starts: measure, profile, fix. A fix is kept only if the benchmark shows it pays and `cargo xtask oracle-test` passes for every supported version. Every timing is taken from a `dist` build ([ADR 0031](../adr/0031-flattened-density-evaluation.md)): under the default release profile, how the compiler split crates moved speeds by up to 1.7×.

### Phase 1: a baseline

- **Benchmark lines for viewer-shaped work** in `crates/spyglass/examples/bench.rs`, each one 256 × 256 tile as the viewer asks for it:
  - Biomes at a height, at 1:4 and at 1:16.
  - Biomes at the surface at 1:4: `Session::height_map`, then `surface_biome_map`.
  - Relief at 1:4: the height map alone.
  - Structure markers at 1:4 for `minecraft:swamp_hut` and `#minecraft:village`, unconfirmed and confirmed (`Session::map_features`).
  - Each in a new session, and again for a neighbouring tile in the same session, since the viewer's workers share one session per seed. The second number shows what the session's caches already save.
- **The viewer's own timings**, behind an environment variable (`SPYGLASS_VIEWER_TIMING=1`) so nothing changes for users:
  - Each job's compute time, by kind (biomes, heights, markers, layouts), measured in the worker.
  - From showing a seed to the first full window of coarse tiles, and to a full window of detailed tiles and markers.
  - Written to stderr, which the dist build's Windows subsystem keeps when stderr is redirected.
- **Recorded scenarios**, on the dev machine, each from a cold start with the window at 1280 × 800:
  1. 1:4, biomes at a height, no structures.
  2. 1:4, biomes at the surface.
  3. 1:4, biomes at the surface, relief, swamp huts confirmed.
  4. 1:16, biomes at a height, villages unconfirmed.
  5. Scenario 2 while a search runs.

### Phase 2: where the time goes

- `density_timing` (`--features timing`) gains a scenario for a viewer tile at 1:4, splitting the height map's time by column stage and density node.
- A sampling profiler (samply, which runs on all three platforms) over the viewer for scenario 3, to see what lies outside terrain: the `SurfaceLevels` mutex the workers share, marker confirmation, colourizing, texture uploads.
- Contention: scenario 5 again, with the search's threads lowered by hand, to measure what the two pools cost each other.

### Phase 3: fixes, chosen by the profile

Candidates, in the order the profile is expected to rank them. Each gets its benchmark line before and after, recorded in Notes:

- **Height maps:** work repeated within a tile (the noise cells neighbouring columns share at 1:4) or between neighbouring tiles. This change is in `spyglass-terrain` and is bit-exact or it's dropped.
- **Shared session state:** the aquifer's `SurfaceLevels` is a `Mutex<HashMap>` that every worker locks. If the profile shows waiting, try per-worker caches or a sharded map.
- **The search and the map:** today each uses every core but one. Options: the search yields cores while tiles are pending, or the tile pool runs at a higher priority. Either changes how the viewer schedules work, so it gets an ADR.
- **Scheduling:** what the pool does with jobs that left the view (dropped today only before they start), and drawing a ring of tiles just outside the window ahead of time.
- **Markers:** confirmed starts run terrain for each tile's starts. Starts near tile edges, and layouts, may be computed more than once.
- **Textures:** `TEXTURES_PER_FRAME` (12) and when a finished tile becomes a texture.

Targets are set from the baseline and the profile, then written into the milestone's second exit criterion before phase 3 starts.

## Tasks

- [ ] Benchmark lines for viewer-shaped tiles (phase 1).
- [ ] `SPYGLASS_VIEWER_TIMING`: per-job times and time to a full window.
- [ ] Baseline for the five scenarios, recorded in Notes and in the milestone.
- [ ] `density_timing` scenario for a viewer tile; samply profile of scenario 3; contention measured in scenario 5.
- [ ] Targets written into milestone 14's exit criteria.
- [ ] The fixes the profile points to, each measured before and after.
- [ ] ADR for any lasting decision: scheduling between the search and the map, a new cache, a change in how tiles are queued.
- [ ] `cargo xtask check`, and `cargo xtask oracle-test` for every supported version after any library change.
- [ ] Docs: the timing variable where the viewer's environment variables are described; architecture.md's viewer row if scheduling changes.

## Verification

- `cargo xtask bench` before and after each fix, from the same commit's dist build, numbers in Notes.
- The viewer's timings for the five scenarios, before and after, against the targets.
- `cargo xtask oracle-test` for every supported version after any change to the library or the crates below it: a faster answer that differs from the game's is a failure.
- Existing tests: tiles assembled into a view equal the view rendered at once ([render.md](../specs/render.md#testing)), and the viewer's pool computes what the library does.

## Durable outputs

- [ ] The benchmark lines stay in `bench.rs`.
- [ ] Any ADR the fixes need.
- [ ] [render.md](../specs/render.md) or [terrain.md](../specs/terrain.md), if a fix changes what they say about how results are computed (not what they are).
- [ ] [architecture.md](../architecture.md): the viewer's row, if scheduling changes.

## Notes

Decisions, surprises and dead ends, newest first.

- 2026-10-29: drafted. What's known going in: a cold window at 1:4 with biomes at the surface takes 20 to 40 seconds for its detailed tiles (milestone 13's plan), and markers wait for a running search to end.
