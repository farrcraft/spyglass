# 09. Fast density evaluation

- Status: Planned
- Depends on: [07](07-terrain-and-validity.md)
- Plans: none yet

## Goal

Make terrain cheap enough that a single column costs well under the game's, so heights, confirmed starts and jigsaw layouts can be asked for freely.

## Scope

- The flattened, SIMD-friendly density evaluation that [architecture.md](../architecture.md) plans in place of the graph interpreter: the same values, bit for bit, through both the point and the volume paths.
- Lower fixed cost per volume, which today dominates small volumes such as one column.
- Cheaper `find_top_surface`, which the aquifer's surface levels and the preliminary surface use.
- An ADR for the evaluation's design.

## Exit criteria

- [ ] Every density, height and structure oracle comparison still passes.
- [ ] `height_at` for one isolated column in a bound session takes at most 150 µs (release, one thread, 26.3), the target milestone 07 retired.
- [ ] Bench lines for columns, regions, confirmed search and jigsaw layouts, recorded before and after.
