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
