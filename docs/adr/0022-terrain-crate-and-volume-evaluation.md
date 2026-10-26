# 0022. Terrain in its own crate, with the game's volume evaluation

- Status: Accepted
- Date: 2026-10-25

## Context

Most Overworld structures can't be confirmed without terrain: their biome check samples at a height the terrain gives, and some reject starts on terrain alone ([specs/structure-placement.md](../specs/structure-placement.md#with-terrain)). The game answers these before any chunk exists. `ChunkGenerator.getBaseHeight` and `getBaseColumn` evaluate one column of `final_density` and the aquifer, and nothing else: no material rule, no carvers, no structures ([specs/terrain.md](../specs/terrain.md)). The oracle's height files confirm it: the generator's columns hold only stone, air, water and lava.

Three things make this hard in 26.3:

- **The game evaluates terrain as a volume, not point by point.** Columns and chunks call `sampleVolume` on `final_density`. For `interpolated` the volume path does different float arithmetic from the point path (z, then x, then an accumulating y, against lerp3 in x, y, z). A `mul` of two non-constant operands, `div`, `min`, `max` and `noise` also differ in signed zeros, NaN, or the low bits of octave coordinates. `spyglass-density` ports only the point path, which was enough for the climate because its two paths agree ([specs/density-functions.md](../specs/density-functions.md#point-and-volume-evaluation)).
- **The game compiles `min` and `max` by their operands' ranges** (`Interval`). An operand that's always on the winning side becomes the whole value, which changes results in the same corner cases. So bit-exactness needs the game's range arithmetic too.
- **Cost.** Terrain binds about 30 more noises per seed than the climate, and a column costs the game about half a millisecond. Search binds a session per seed, and most queries never look at terrain.

The architecture already plans `spyglass-terrain` between density and structure.

## Decision

We will:

- Create **`spyglass-terrain`** (depending on rng, noise, density and data). It holds the generator's column: `final_density` sampled as a column volume, the aquifer and the fluid picker, and the queries built on them: base height for `WORLD_SURFACE_WG` and `OCEAN_FLOOR_WG`, first free and first occupied heights, and block columns. `spyglass-structure` depends on it, and decides starts with terrain when a terrain is supplied.
- Give **`spyglass-density` a volume path** beside the point path, mirroring `sampleVolume` type by type where the game's differs, and compute each node's range as `Interval` does, to compile `min` and `max` as the game does. The point path stays what the climate and the aquifer use.
- **Bind terrain lazily** in `spyglass`'s `Session`: the first query that needs a height binds it. Climate-only work, including most of search, doesn't pay for it.
- Leave the **material rule** (surface blocks, ore veins, bedrock) out of terrain, because no height or column query evaluates it.

No new dependencies.

## Consequences

- Heights can be compared bit for bit with the game's generator, and the oracle compares exactly what structures consume.
- Two evaluation paths in `spyglass-density` must both stay exact. The probe compares the point path at points and the volume path through chunk digests and heights; a type ported for one path and not the other is an error at load or bind time, not a silent fallback.
- Porting `Interval` adds arithmetic that exists only to choose samplers. Spline and `pow` bounds are intricate, and an error there shows up only in rare signed-zero or NaN cases, so it gets its own unit tests.
- Real chunks differ from these heights near structures (the beardifier) and where the material rule or carvers change blocks. Callers get the generator's answer, which is what structures use. A surface map or the final spawn block would need the material rule and features, and are out of this decision.
- `spyglass-structure` gains a dependency on `spyglass-terrain`, as the architecture planned.

## Alternatives considered

- **One evaluation path (points only).** Simpler, but it can't match the game's terrain: `interpolated` alone differs in the low bits of nearly every block, and heights depend on where density crosses zero.
- **Volume path only.** Climate lookups, the aquifer and `find_top_surface`'s density are sampled at points by the game, so a volume-only port would be wrong for them instead.
- **Terrain inside `spyglass-density` or `spyglass-structure`.** The aquifer and the column queries are neither density functions nor structure logic, and the Nether and End (milestone 08 on the [roadmap](../roadmap/README.md)) reuse them without structures.
- **Binding terrain with every session.** Simpler lifetimes, but it would slow every seed in a search by the cost of 30 noises, for queries that never ask for a height.
