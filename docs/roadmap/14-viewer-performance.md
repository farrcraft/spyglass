# 14. Viewer performance

- Status: In progress
- Depends on: [13](completed/13-viewer-redesign.md)
- Plans: [m14-viewer-performance](../plans/m14-viewer-performance.md)

## Goal

The map fills the window quickly enough that waiting for it isn't part of using the viewer, at the zoom levels and layers people use most. The time goes down, not just the wait being hidden: most of the cost is expected to be in the library, so `spyglass render` and confirmed search should get faster too.

## Scope

- **A baseline first:**
  - `cargo xtask bench` gains viewer-shaped work: one 256 × 256 tile at 1:4 and at 1:16, with biomes at a height, biomes at the surface, relief, and structure markers (confirmed and not).
  - The viewer can report, in a debug build or behind a flag, the time from opening a seed to a full window and the time per tile.
- **A profile** of a cold start at 1:4 with biomes at the surface. It shows how the time splits between height maps, biome lookups, structure markers and the viewer's own work: queueing, threads and texture uploads.
- **Fixes chosen by the profile.** Candidates:
  - Cheaper height maps: less repeated work within a tile, and work shared with its neighbours.
  - Sessions' caches shared across the tile workers rather than each worker filling its own.
  - The worker count, and what the pool does with jobs that leave the view.
  - Tiles just outside the window drawn ahead of time.
  - Texture uploads spread over frames without leaving finished tiles undrawn.
- **The results recorded:** the benchmark's before and after, and an ADR for any decision that outlasts the work, such as a new cache or a change in how tiles are scheduled.

Not in scope:

- Hiding the wait. Coarse-first loading and the placeholder for undrawn tiles are milestone 13's.
- GPU computation (the roadmap's stretch goal).
- Anything that changes results: every library change still matches the game exactly.

## Exit criteria

- [x] The baseline is recorded: the new benchmark lines and the viewer's time to a full window, at 1:4 with and without biomes at the surface, on the dev machine.
- [ ] Targets for those numbers are set from the baseline and the profile, written here, and met.
  - A cold 1:4 window, biomes at y 64 (`cargo xtask viewer-timing 1`): full in 0.5 s or less.
  - A cold 1:4 window, biomes at the surface (scenarios 2 and 3): full in 12 s or less, half the baseline.
  - The same while the viewer searches on every other core: no more than 15% slower than without.
  - `cargo xtask bench`, biomes at the surface on one 1:4 tile in a new session: 6.5 s or less.
- [ ] `cargo xtask oracle-test` passes for every supported version, as well as `cargo xtask check`.

## Baseline

On the dev machine (Ryzen 7 5800X, 8 cores, 16 threads), 26.3, a `dist` build, 2026-10-29. The viewer's time to a full window from a cold start, seed 0 at the origin, window 1280 × 800, with `SPYGLASS_VIEWER_TIMING=1`:

| Scenario                      | Window full |
| ----------------------------- | ----------- |
| 1:4, biomes at y 64           | 0.43 s      |
| 1:4, biomes at the surface    | 23.65 s     |
| 1:4, surface, relief and huts | 23.98 s     |
| 1:16, villages unconfirmed    | 0.45 s      |
| 1:4, surface, while searching | 28.70 s     |

`cargo xtask bench`, one 256 × 256 tile in a new session: heights at 1:4 9.1 s, biomes at the surface 9.2 s, biomes at y 64 148 ms at 1:4 and 146 ms at 1:16, structure markers 0.4 to 27 ms.

## Results

The same machine and settings, 2026-10-29, after the fixes ([the plan](../plans/m14-viewer-performance.md) has each one's numbers). Runs vary by up to about 20% on this machine.

| Scenario                      | Baseline | Now            |
| ----------------------------- | -------- | -------------- |
| 1:4, biomes at y 64           | 0.43 s   | 0.23 to 0.25 s |
| 1:4, biomes at the surface    | 23.65 s  | 8.5 to 8.7 s   |
| 1:4, surface, relief and huts | 23.98 s  | 8.5 to 10.4 s  |
| 1:16, villages unconfirmed    | 0.45 s   | 0.25 s         |
| 1:4, surface, while searching | 28.70 s  | 9.3 s          |

The last row's search runs in another process, so it shows less contention than a search in the viewer. In one process, 48 tiles of heights on 15 workers take 8.4 s alone, 15.6 s beside a 15-thread search, and 9.1 s with that search throttled while they run, as the viewer now does. `cargo xtask bench`: biomes at the surface on a 1:4 tile 5.8 s, heights alone 5.7 s.
