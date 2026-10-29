# m14. Viewer performance

- Status: In progress
- Milestone: [14. Viewer performance](../roadmap/14-viewer-performance.md)
- Started: 2026-10-29
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

- [x] Benchmark lines for viewer-shaped tiles (phase 1), and `--only` to run some of them.
- [x] `SPYGLASS_VIEWER_TIMING`: per-job times and time to a full window. `SPYGLASS_VIEWER_SETTINGS` names the settings file, so scenarios start from known settings.
- [x] Baseline for the five scenarios, recorded in Notes and in the milestone.
- [x] `density_timing` scenario for a viewer tile, and contention between the tile workers (none from shared state).
- [x] What lies outside terrain, and contention between the search and the map. Measured with the viewer's own timings (textures included) instead of samply: heights are 98% of the work.
- [x] Targets written into milestone 14's exit criteria, with today's results; waiting for agreement before the criterion is ticked.
- [x] The fixes the profile points to, each measured before and after: sparse grids by column, partial surface-level reuse, the two-part scan from the top, 128-pixel tiles.
- [x] ADR for any lasting decision: [ADR 0046](../adr/0046-the-viewers-search-yields-to-the-map.md), the viewer's search yields its cores to the map.
- [x] `cargo xtask viewer-timing`: the scenarios as a project command, so they can be run again.
- [x] `cargo xtask check`, and `cargo xtask oracle-test` for every supported version after any library change (2026-10-29, after the fixes above).
- [x] Docs: both variables and `viewer-timing` in getting-started; architecture.md's viewer row for ADR 0046; search.md for the throttle.

## Verification

- `cargo xtask bench` before and after each fix, from the same commit's dist build, numbers in Notes.
- The viewer's timings for the five scenarios, before and after, against the targets.
- `cargo xtask oracle-test` for every supported version after any change to the library or the crates below it: a faster answer that differs from the game's is a failure.
- Existing tests: tiles assembled into a view equal the view rendered at once ([render.md](../specs/render.md#testing)), and the viewer's pool computes what the library does.

## Durable outputs

- [x] The benchmark lines stay in `bench.rs`.
- [x] Any ADR the fixes need: [0046](../adr/0046-the-viewers-search-yields-to-the-map.md).
- [x] [render.md](../specs/render.md) or [terrain.md](../specs/terrain.md), if a fix changes what they say about how results are computed (not what they are). Also [density-functions.md](../specs/density-functions.md), for the y alignment.
- [x] [architecture.md](../architecture.md): the viewer's row, if scheduling changes.

## Notes

Decisions, surprises and dead ends, newest first.

- 2026-10-29: the search and the map. A search in another process barely slows the map (scenario 5: 9.3 s against 8.5 s), but in one process the cores are shared evenly: 48 tiles of heights on 15 workers take 8.8 s alone, 10.3 s beside a 4-thread search, 11.7 s beside 8, 16.2 s beside 15. A `SearchThrottle` now limits a running search's threads at work, taken up per batch of 16 seeds, and the viewer sets it to one while tiles are pending ([ADR 0046](../adr/0046-the-viewers-search-yields-to-the-map.md)). Throttled, the 15-thread search leaves the tiles at 9.05 s (8.44 s alone that run) and still evaluates seeds, 44,000 in that time against 322,000 unthrottled. An in-viewer search can't be started without a click, so the scenarios don't measure that case directly.
- 2026-10-29: textures on the UI thread take 0.2 to 0.5 ms each, so nothing outside the tile jobs matters yet; samply wasn't needed. The targets were set after the fixes, not before phase 3 as planned, since the profile pointed at clear fixes first.
- 2026-10-29: the viewer's tiles are 128 pixels, not 256. With 256, 16 tiles filled a 1:4 window on 15 workers, so the last ran alone in a second round. At 128 the window is 48 jobs of a quarter the work, and less of it lies outside the window. The cache holds 1024 tiles, the same memory. Scenarios after the library fixes, at 256 and then at 128:

  | Scenario                                  | Baseline | Library fixes | And 128-pixel tiles |
  | ----------------------------------------- | -------- | ------------- | ------------------- |
  | 1. 1:4, biomes at y 64                    | 0.43 s   |               | 0.23 s              |
  | 2. 1:4, biomes at the surface             | 23.65 s  | 15.11 s       | 8.67 s              |
  | 3. 2 with relief and swamp huts confirmed | 23.98 s  | 14.44 s       | 8.59 s              |
  | 4. 1:16, villages unconfirmed             | 0.45 s   |               | 0.26 s              |
  | 5. 2 while a search runs                  | 28.70 s  | 17.64 s       | 9.35 s              |

  The window is covered in 0.08 s rather than 0.27 s: a coarse tile is a quarter the size.

- 2026-10-29: the profile (`density_timing`'s new "64 x 64 pixels at 1:4", a sixteenth of a tile, 540 ms): `final_density` volumes 78%, aquifer setup 18% (nearly all surface levels), the scan 4%.
  - A 1:4 column's `final_density` costs as much as a lone column's (about 100 µs): at 1:4 each column is on its own cell corner, so neighbours share no cells. The chunk box around a chunk's 16 columns holds 169, and is filled for nothing.
  - Surface levels: a 64 × 64 piece made 35,000 point samples for about 4,100 quart columns. An aquifer skipped sampling only if every level in its prefill box was known; neighbouring chunks' boxes overlap in part, so nearly every one sampled its whole box again.
  - The scan reads about 245 of a column's 384 blocks (surfaces are at y 32 to 128 in seeds 0, 1 and 123456789), but every block's density was computed. Sampling only the top 256 blocks saved 21%.
  - Contention: 15 threads on 15 tiles take 15.1 s in one shared session and 16.2 s in a session each, so the shared state (`SurfaceLevels`, the corner caches) costs nothing measurable. 8 threads already take 1.4 times as long per tile as one: the cores, not the locks. The `SurfaceLevels` candidate is dropped.
  - Fixes, each bit-exact and kept: a sparse chunk's columns sampled each on its own (8.72 → 7.7 s a tile); only unknown surface levels sampled, at points (→ 7.04 s); and the scan from the top in two parts, split on `final_density`'s y alignment below the preliminary surface level (→ 5.17 s). Single columns (`height_at`) use the two-part scan too, but a back-to-back benchmark shows no change there beyond noise (±20% on short lines on this machine): a lone column's cost is mostly its aquifer setup. A 1:4 tile's heights are 0.62 to 0.68 of the baseline in that pair, a 1:1 tile's 0.91.
  - The height golden test passed with parts deliberately split 3 blocks off a cell corner: a slightly different float rarely flips a density's sign. So a direct test compares a column in parts with the whole column bit for bit, for every version and dimension; the misaligned split fails it in 26.3.
- 2026-10-29: baseline, on the dev machine (Ryzen 7 5800X, 8 cores, 16 threads; 15 tile workers), 26.3, dist build.
  - Scenarios, seed 0 at the origin, a cold start with the window at 1280 × 800 (16 tiles in view at 1:4). Run by setting up a settings file per scenario and starting the viewer with `SPYGLASS_VIEWER_SETTINGS` and `SPYGLASS_VIEWER_TIMING`. Scenario 5's search ran in another process (`spyglass search --confirm --threads 15`), since a search can't be started without a click.

    | Scenario                                  | Window covered | Window full | Per job                                      |
    | ----------------------------------------- | -------------- | ----------- | -------------------------------------------- |
    | 1. 1:4, biomes at y 64                    | 0.27 s         | 0.43 s      | biomes 1:4 221 ms, 1:16 245 ms               |
    | 2. 1:4, biomes at the surface             | 0.27 s         | 23.65 s     | heights 1:4 14.5 s, biomes 212 ms            |
    | 3. 2 with relief and swamp huts confirmed | 0.30 s         | 23.98 s     | heights 14.5 s, markers 0.7 ms, layouts 0 ms |
    | 4. 1:16, villages unconfirmed             | 0.28 s         | 0.45 s      | biomes 1:16 228 ms, markers 39 ms            |
    | 5. 2 while a search runs                  | 0.31 s         | 28.70 s     | heights 15.1 s, biomes 244 ms                |

  - Height maps are the whole wait. 16 tiles on 15 workers take two rounds: about 14.5 s with every worker busy, then the last tile alone. Biomes and markers are under a second together.
  - The benchmark, one thread: a 1:4 tile's heights take 8.7 s in a new session and 9.2 s next to a tile already done, so the session's caches save nothing between neighbours. A 1:1 tile, with as many columns, takes 1.0 s: at 1:4 the columns share no noise cells. Biomes are 145 ms at 1:4, 160 ms at 1:16; markers 0.3 to 26 ms. With every worker busy a height map takes 1.6 times as long as alone, from the cores' second threads and anything the workers share.
- 2026-10-29: drafted. What's known going in: a cold window at 1:4 with biomes at the surface takes 20 to 40 seconds for its detailed tiles (milestone 13's plan), and markers wait for a running search to end.
