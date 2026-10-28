# 09. Fast density evaluation

- Status: Completed
- Depends on: [07](07-terrain-and-validity.md)
- Plans: [m09-fast-density](../../plans/completed/m09-fast-density.md)

## Goal

Make terrain cheap enough that a single column costs well under the game's, so heights, confirmed starts and jigsaw layouts can be asked for freely.

## Scope

- The flattened, SIMD-friendly density evaluation that [architecture.md](../../architecture.md) plans in place of the graph interpreter: the same values, bit for bit, through both the point and the volume paths.
- Lower fixed cost per volume, which today dominates small volumes such as one column.
- Cheaper `find_top_surface`, which the aquifer's surface levels and the preliminary surface use.
- Every evaluator family, not only 26.3's: 26.2's `f64` family ([ADR 0026](../../adr/0026-how-game-versions-coexist.md)) has no volume paths or shared caches and builds a `NoiseChunk` per column, and benches several times slower than 26.3 (7× for one-thread search, 8× for `height_at` on a bound session).
- An ADR for the evaluation's design.

## Exit criteria

- [x] Every density, height and structure oracle comparison still passes.
- [x] `height_at` for one isolated column in a bound session takes at most 235 µs (the `dist` profile, one thread, 26.3): 227 to 233 µs. Revised from 150 µs with the user on 2026-10-28, since 150 µs would need fewer positions evaluated than the game evaluates ([ADR 0032](../../adr/0032-density-programs-only-where-they-pay.md)).
- [x] Bench lines for columns, regions, confirmed search and jigsaw layouts, recorded before and after.
