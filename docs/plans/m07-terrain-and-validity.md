# m07. Terrain, structure validity, jigsaw

- Status: In progress
- Milestone: [07. Terrain, structure validity, jigsaw](../roadmap/07-terrain-and-validity.md)
- Started: 2026-10-25
- Completed: not yet

## Goal

Compute the Overworld's terrain heights exactly, and use them to decide which structure starts the game really makes and how big jigsaw structures are, from the library, `spyglass locate`, `spyglass search` and the viewer. One plan for the whole milestone, in phases that can each be verified on their own (phases 6 to 8 can become a second plan if this one grows). It covers every exit criterion:

- Heightmaps match the oracle for the seed set: phases 2 to 4.
- Structure starts after validity checks, and their bounds where jigsaw is used, match the oracle: phases 5 and 6.
- Evaluation is fast enough to use as a late-stage search constraint, against the targets [below](#speed-targets): phases 7 and 8.

## Facts established up front

From the code as of fa52e5c, the 26.3 bundle, and figures recorded in earlier plans.

- **26.3's noise router isn't 1.21's.** `noise_settings/overworld.json` routes `temperature`, `vegetation`, `continents`, `erosion`, `depth`, `ridges`, `final_density` and `chunk_surface_level` (an `interpolated` with 16 × 1 cells over `overworld/preliminary_surface_level`, which is a `find_top_surface` with `cell_height` 8). There's no `initial_density_without_jaggedness`, `vein_toggle` and the like. Aquifers have their own `aquifers` object (`barrier`, `exclusion`, `fluid_level_floodedness`, `fluid_level_spread`, `lava`, `surface_level`). `noise` holds only `min_y` −64 and `height` 384: cell sizes moved into each `interpolated` (`cell_size_xz` 4 and `cell_size_y` 8 for ten of them).
- **Surface rules became material rules.** `material_rule` is `minecraft:overworld`, its own registry (`worldgen/material_rule/`, `worldgen/material_condition/`): a sequence of bedrock floor, copper and iron ore veins, then `above_preliminary_surface` → `overworld/surface`, then `overworld/underground`. It uses 14 rule and condition types (`sequence`, `condition`, `block`, `biome`, `noise_threshold`, `y_above`, `stone_depth`, `steep`, `hole`, `water`, `vertical_gradient`, `bandlands`, `not`, `ore_vein`). Which of it decides a heightmap is unknown: phase 1 settles it from the bytecode.
- **What density can't do yet.** 28 of 26.3's 55 density functions load ([density-functions.md](../specs/density-functions.md)). The types `final_density` and the aquifers need beyond those: `sub`, `div`, `min`, `max`, `clamp`, the other unaries (`square`, `cube`, `half_negative`, `quarter_negative`, `squeeze`, `negate`), `range_choice`, `interval_select`, `interpolated`, `slice`, `find_top_surface`, `old_blended_noise`, `blend_density` and `beardifier`. `end_outer_islands` and `distance_to_point` are the End's (milestone 08), but should load.
- **Point evaluation isn't enough.** The density spec ports only the point path, because the climate's volume and point paths agree. `interpolated` breaks that: chunk generation evaluates its input at cell corners and interpolates, so terrain needs the game's cell-based path (`NoiseChunk`, or its 26.3 successor), ported as the bytecode does it, order of float operations included.
- **Structure checks don't see generated chunks.** They ask the generator for a column before any chunk exists (`ChunkGenerator.getBaseHeight`, `getBaseColumn`, `getFirstFreeHeight`, `getFirstOccupiedHeight`), and the `_WG` heightmaps. Phase 1 confirms what those evaluate in 26.3 (we expect: no beardifier, no blending, aquifers yes, surface part of the material rule no). The oracle compares those generator answers, not the heightmaps stored in chunks, which include structures' terrain adaptation and features.
- **What terrain decides.** From [structure-placement.md](../specs/structure-placement.md#the-biome-check): the surface types (pyramid, temple, swamp hut, igloo) check the biome at `WORLD_SURFACE_WG`, the ocean types at `OCEAN_FLOOR_WG`, mansions over a rotated 5 × 5 box, projected jigsaws (villages, outposts and most others) at the heightmap under their start piece's centre, ruined portals at `findSuitableY` over noise columns, mineshafts in mesas from terrain. Plus the other rejections in that table: pyramid and temple below sea level, mansion below 60. Every one becomes a 3D biome lookup at a known position, so nearly every `Candidate` becomes `Passes` or `Rejected`; only tied cells and platform-dependent rings stay uncertain.
- **Jigsaw today stops at the start piece.** `Jigsaw::check_position` draws the start height, rotation, start element and start jigsaw, and gives the check position; pieces beyond the start aren't assembled, and pool aliases that rename the start pool aren't supported (trial chambers use them).
- **Per-seed costs today** (release, one thread, from m06-seed-search): binding a seed's climate 87 µs; one `biome_at` about 2.4 µs; a swamp-hut check 95 µs; a swamp-hut count within 1,000 of 0,0, 1.2 ms in a new session; a mixed search 3,700 seeds a second on one thread, 24,000 on 16. Terrain adds about 30 more noises to bind per seed, so it must be bound lazily, only for seeds and queries that need it.
- **`spyglass-terrain` doesn't exist.** [architecture.md](../architecture.md#crates) plans it between density and structure, depending on density and data.

## Approach

### Layers

- **`spyglass-density`**: every density-function type 26.3's overworld router and aquifers use; a volume path beside the point path, as the game's `sampleVolume` does it, with ranges to compile `min` and `max` as the game does ([ADR 0022](../adr/0022-terrain-crate-and-volume-evaluation.md)); a `TerrainRouter` beside `ClimateRouter`, bound lazily. 26.3 has one cache type, `cache`.
- **`spyglass-terrain`** (new; depends on rng, noise, density, data): the noise column for one `(x, z)` (final density, aquifer substance), and from it the generator queries the structure code needs: `base_height(x, z, heightmap)` for `WORLD_SURFACE_WG` and `OCEAN_FLOOR_WG`, `base_column(x, z)` (solid, fluid or air per y, for ruined portals), first free and first occupied heights. Columns are cached per seed by `(x, z)`, since several structures' checks share chunks. No material rule: no height query evaluates it.
- **`spyglass-structure`**: `Starts` takes an optional terrain. With one, each type's check runs at its real position and its other terrain conditions apply; without, it answers as today. Jigsaw assembly (`JigsawPlacement.addPieces`) builds the pieces and the start's bounding box. The three-valued `Check` stays: a terrain answer is `Passes` or `Rejected` unless a tie or the platform leaves it open.
- **`spyglass`**: `Session` gains heights (`Session::height_at(x, z, heightmap)`), and a confirmed mode for `locate_structure`, `structures_in` and search. Starts carry their bounds where known.
- **Front ends**: `--confirm` (or the default, see [Decisions](#decisions)) on `spyglass locate` and `spyglass search`, bounds in human and `--json` output, a heights command; in the viewer, confirmed markers, bounds drawn at close zoom, and a height layer.

Rough API, names to change during the work:

```rust
pub enum Heightmap { WorldSurfaceWg, OceanFloorWg }
impl Session<'_> {
    pub fn height_at(&self, [x, z]: [i32; 2], heightmap: Heightmap) -> i32;
    pub fn heights_in(&self, min: [i32; 2], max: [i32; 2], heightmap: Heightmap) -> Vec<i32>;
}
pub struct LocateOptions { pub confirm: bool, /* existing fields */ }
pub struct StructureStart {
    /* existing fields */
    pub check: [i32; 3],                 // the real check position when confirmed
    pub bounds: Option<BoundingBox>,     // jigsaw, when confirmed
}
pub struct SearchOptions { pub confirm: bool, /* existing fields */ }
```

### Terrain

Ported from the 26.3 bytecode, as the density spec's types were. The work in order:

1. **Types.** Each missing density-function type, point value first, with hand-worked tests, and the probe extended to every named function at points (`sampleBlockValueUncached`, as for the climate).
2. **The cell path.** The game's chunk-level evaluator for `interpolated`, `find_top_surface` and the caches. The probe gains final density over whole chunks (a digest per chunk, as `climate_digest` does), and the cell path must match it bit for bit. The point path stays for everything that isn't interpolated.
3. **Aquifers.** `NoiseBasedAquifer` (or its 26.3 form): fluid levels per aquifer cell, barriers, lava below its level, the `exclusion` function. They decide water against air in open caves and ravines, so they decide `OCEAN_FLOOR_WG` against `WORLD_SURFACE_WG` there.
4. **Columns and heightmaps.** `iterateNoiseColumn` from the top cell down, stopping as the game stops for each query.

### Structure validity

`Starts::check` with terrain, type by type, with the other rejections in the spec's table. Each type's position and conditions are ported from its `findGenerationPoint`; the spec's table already lists them, and phase 1 fills the gaps (the mansion's corners and rotation draw order, ruined portal placement types and `findSuitableY`, the mesa mineshaft's y). Nether fossils and end cities need other dimensions' terrain, so they stay `Candidate` until milestone 08.

### Jigsaw

`JigsawPlacement.addPieces` after the start piece: the placement queue (in priority order, as 26.3 orders it), pool fallbacks, `max_depth` and `max_distance_from_center`, jigsaw block matching (`JigsawBlock.canAttach`: orientation, target, joint type), rigid against terrain-matching projection (terrain-matching pieces read the heightmap), and the free-space test with voxel shapes, which for integer boxes we can do exactly with box sets. Pool aliases (`PoolAliasLookup`, its own random) are supported, so trial chambers assemble. The start's bounds are the union of its pieces' boxes, adjusted as `Structure` adjusts them for terrain adaptation (phase 1 confirms how). Processors and block placement don't change bounds, so they're out.

Non-jigsaw structures' pieces (strongholds, mansions, monuments, mineshafts and the single-piece types) aren't part of the exit criteria. Their bounds are left out, except where a fixed template size gives them for free.

### Confirmed mode

- **Locate**: with `confirm`, each candidate is settled by terrain in search order; the list usually ends at its first hit, and certain answers equal `/locate`'s.
- **Search**: terrain is the last stage of `Session::check`, after every biome-only check, and runs only on the candidate starts that can still change the verdict or score. `Session::evaluate` with `confirm` stays the plain implementation the fast path is tested against. The query language doesn't change; `confirm` is an option, like `certain`. In confirmed mode, `possibly met` remains only for ties and platform-dependent results.

### Speed targets

Set now, revisited once after phase 4's first measurement (any change recorded in Notes with the numbers behind it). Release, one thread, on the 26.3 bundle, in a new session:

- `height_at` for one column in a bound session: at most 150 µs; binding a seed's terrain: at most 400 µs; 1,000 columns in one chunk-aligned region: at most 30 ms. (Revised after phase 4's measurement, see Notes: the first target included binding, which alone costs more than 150 µs.)
- A confirmed swamp-hut count within 1,000 of 0,0: at most 3× the unconfirmed cost (1.2 ms today, so 3.6 ms).
- A confirmed `#minecraft:village` locate from 0,0: at most 50 ms.
- The m06 witch-hut query (`require 2 minecraft:swamp_hut within 400 of 0,0`) searched with `confirm` on all cores: at least a quarter of the unconfirmed rate.

If the interpreter can't reach these, the architecture's flattened evaluation is the next step, in this milestone, before front-end work.

### The oracle

- **Probe**: every named density function at points; final density chunk digests through the game's chunk path; aquifer substance at points.
- **Height golden files** (new): `oracle-out/<version>/heights/overworld/<seed>.json` or binary like the biome files (phase 1 decides on size), for the golden seeds: `getBaseHeight` for both `_WG` heightmaps at every column of a 256 × 256 block area around the origin and 64 × 64 at the far and border regions; plus `getBaseColumn` digests for a sample of columns.
- **Structure golden files**: format 2 adds, per try, the start's bounding box and piece count when `start` is true. Already there: the stub (now exact for every type with terrain) and `valid_biome`.
- **Locate answers**: unchanged; the confirmed comparison requires the game's answer to be spyglass's first certain hit.

Specs and ADRs that govern this: [density-functions.md](../specs/density-functions.md), [structure-placement.md](../specs/structure-placement.md), [locate.md](../specs/locate.md), [search.md](../specs/search.md), [oracle.md](../specs/oracle.md), [render.md](../specs/render.md), [ADR 0008](../adr/0008-game-derived-test-values-stay-local.md), [ADR 0010](../adr/0010-typed-loading-of-worldgen-json.md), [ADR 0011](../adr/0011-parse-json-numbers-from-their-text.md), [ADR 0013](../adr/0013-structure-templates-and-nbt.md), [ADR 0014](../adr/0014-public-api-world-and-session.md), [ADR 0021](../adr/0021-search-engine.md).

## Tasks

### Phase 1: the bytecode, the oracle and specs

- [x] From the 26.3 bytecode, note in the specs (not in this plan): the chunk-level density path and its caches; what `getBaseHeight`, `getBaseColumn`, `getFirstFreeHeight` and `getFirstOccupiedHeight` evaluate (beardifier, blending, aquifers, material rule); `find_top_surface`; the aquifer; each structure type's terrain conditions; `addPieces` and bounding-box adjustment. In [terrain.md](../specs/terrain.md), [density-functions.md](../specs/density-functions.md) and [structure-placement.md](../specs/structure-placement.md), as drafts.
- [x] Decide surface rules (see [Decisions](#decisions)) from what heightmaps need, and update the milestone's scope if they move. Out: no height query evaluates the material rule; the milestone's scope says so.
- [x] Oracle: height golden files, structure format 2 with bounds and piece boxes; `cargo xtask oracle golden` regenerates them. `docs/specs/oracle.md` updated.
- [x] Oracle probe additions: every named density function at points, final density chunk digests, aquifer substance at points. Moved to phases 2 and 3, where the tests that read them are written. The aquifer's are whole columns (`terrain.json`) rather than points.
- [x] `docs/specs/terrain.md` (new): columns, aquifers, heightmaps, the generator queries, and what's out (features, carvers, beards).
- [x] ADR: `spyglass-terrain` and its boundary with density and structure; how terrain is bound lazily per seed; whether a new dependency is needed (none expected). [0022](../adr/0022-terrain-crate-and-volume-evaluation.md), accepted 2026-10-26: it adds a volume evaluation path, which the plan didn't foresee.

### Phase 2: density types

- [x] Fix `mul`: the point path returns `+0.0` for a zero left operand when neither operand is constant, without evaluating the right.
- [x] Ranges (`Interval`), and `min`/`max` compiled by them.
- [x] Every missing type, loaded by its codec, with point values; `div` by a constant as the game compiles it. `interpolated`'s point value needs the volume path, so it loads but refuses to bind until phase 3; the End's two types load only.
- [x] Hand-worked unit tests per type ([ADR 0008](../adr/0008-game-derived-test-values-stay-local.md)).
- [x] Oracle probe: every named density function at the probe's blocks (`cargo xtask oracle probe`).
- [x] Ignored probe test: every named function at every probed point, bit for bit; every one of the 55 loads. The 14 functions that reach `interpolated` or `distance_to_point` are skipped and listed until they can be bound.

### Phase 3: the volume path and aquifers

- [x] The volume path: `interpolated` (`sampleWithBlockStep`), `mul`/`div`/`min`/`max` and noise as the game's volume samplers compute them; for a column and for a chunk.
- [x] Tests: column against chunk evaluation; remembered against fresh values; probe column and chunk digests (ignored).
- [x] Aquifers, with hand-worked tests and probe columns. In `spyglass-terrain`, created for them (phase 4's first task, done early).

### Phase 4: `spyglass-terrain` and heightmaps

- [x] Create the crate (`[lints] workspace = true`; architecture table updated). Done in phase 3, with the noise settings terrain needs (`TerrainSettings`).
- [x] Columns, `base_height` for both heightmaps, `base_column`, first free and first occupied heights, and `base_heights` for a box, chunk by chunk. The per-seed column cache moves to phase 5, where structure checks are what share columns.
- [x] Ignored golden test: every height in the height golden files, and the block columns, listed in `xtask/src/oracle_test.rs` with a new `Heights` input. Exit criterion 1.
- [x] `Session::height_at` and `heights_in`, bound lazily. First speed measurement against the targets.

### Phase 5: validity

- [x] `Starts` with terrain: every Overworld type's check at its real position, and its other conditions. All but mineshafts, whose check needs their piece layout (`MineshaftPieces`), which isn't ported; they're decided by their column where it's all in or all out of their biomes.
- [x] The golden test, extended: with terrain, every stub must equal the game's exactly, and the selection must equal the game's choice wherever no tie or platform-dependent ring is involved. The confirmed `/locate` comparison moves to phase 7, where `Session` gets `confirm`.
- [x] `docs/specs/structure-placement.md`: what terrain decides, and what still can't be decided. `locate.md` moves to phase 7 with confirmed mode.

### Phase 6: jigsaw assembly

- [x] `addPieces`: queue, pools and fallbacks, depth and distance limits, jigsaw matching, rigid and terrain-matching projection, free space; pool aliases. `Jigsaw::assemble`, with the pools from `JigsawPools::load`, loaded apart from the structure sets (see Notes), and `Starts::with_pools(..).assemble`.
- [x] Unit tests on hand-written pools and templates (attachment, collision, depth, aliases).
- [x] Golden test: every jigsaw start's bounds and piece count equal the game's. Exit criterion 2. Stricter than planned: every piece's box, in order, too.

### Phase 7: confirmed mode in the library

- [ ] `confirm` in locate and `structures_in`; starts carry check positions and bounds.
- [ ] `Session::evaluate` and `Session::check` with `confirm`, terrain as the last stage; soundness and equality tests as in m06-seed-search phase 4, extended to confirmed mode.
- [ ] `docs/specs/search.md`: the `confirm` option and what's still uncertain in it.

### Phase 8: speed and benchmarks

- [ ] Bench lines (`cargo xtask bench`): one column, a region of heights, a confirmed swamp-hut count, a confirmed village locate, the witch-hut search confirmed (one thread and all).
- [ ] Meet the targets; flatten the density interpreter if the interpreter can't. Exit criterion 3.

### Phase 9: front ends and close-out

- [ ] CLI: `--confirm` on `locate` and `search`; `spyglass heights` (or a `locate` subcommand) for a column or region; bounds in output; `cli-json.md` updated; integration tests.
- [ ] Viewer: confirmed markers, jigsaw bounds at close zoom, a height layer (shaded heights), and biomes sampled at the surface as an option ([render.md](../specs/render.md)).
- [ ] Update architecture.md, README, getting-started.
- [ ] `cargo xtask check` and `cargo xtask oracle-test`; move this plan and the milestone to `completed/`.

## Verification

- Ignored oracle comparisons: density at points and chunk digests; heights at every column of the height golden files; stubs, selections and jigsaw bounds against the structure golden files; confirmed `/locate` answers. All run by `cargo xtask oracle-test` (new tests added to `xtask/src/oracle_test.rs`).
- Hand-worked unit tests per density type, aquifer case and jigsaw rule, committed without game data.
- Soundness and equality of confirmed `Session::check` against `Session::evaluate` over seed ranges (fixtures in the normal suite, 26.3 ignored).
- `cargo xtask bench` lines against the speed targets, recorded in Notes.
- A manual check: in a fresh 26.3 world for one golden seed, three villages and a swamp hut from `spyglass locate --confirm` are where the game puts them, and a village's bounds match its pieces in game (F3 or `/place` comparison).

## Durable outputs

- [x] `docs/specs/terrain.md` (new)
- [x] `docs/specs/density-functions.md`: every type, the cell path
- [x] `docs/specs/structure-placement.md`: terrain conditions, jigsaw assembly and bounds
- [ ] `docs/specs/locate.md`, `docs/specs/search.md`: confirmed mode
- [x] `docs/specs/oracle.md`: probe additions, height files, structure format 2
- [ ] `docs/specs/cli-json.md`, `docs/specs/render.md`
- [x] ADR: `spyglass-terrain` and lazy terrain binding ([0022](../adr/0022-terrain-crate-and-volume-evaluation.md))
- [ ] `docs/architecture.md`, `README.md`, `docs/getting-started.md`

## Decisions

Agreed on 2026-10-25, as recommended:

- **Surface rules:** port only what heightmaps need (phase 1 decides; we expect none of the surface part), and move the rest out of this milestone's scope, to a later milestone with features and the final spawn block, which need it too.
- **Confirmed by default:** on by default for locate (one query, and the answer becomes exact) and off by default for search (`--confirm`), where it costs most; both settable.
- **Bounds-based distances in the query language** (`within 100 of minecraft:ancient_city bounds`, say): not in this plan; bounds appear in results and on the map, and the language can add them once someone asks.
- **Non-jigsaw bounds:** out, as above. Strongholds and mansions are large ports for no exit criterion.
- **One plan**, phased like m06-seed-search. Phases 6 to 9 become a second plan if this one grows.

## Notes

- 2026-10-26: Phase 6 done. Exit criterion 2 is met: with terrain, all 301 jigsaw starts in the structure golden files (villages, outposts, abandoned camps, trail ruins, ancient cities, and 165 trial chambers with their pool aliases) assemble to the game's pieces, 47,534 boxes equal in order, and to the game's bounds. Mutations that make the queue last-in-first-out or drop the expansion hack fail 246 and 52 of them. From the bytecode and the data:
  - 26.3 templates write block-state properties as `properties` (with `id` for the name), so the jigsaw orientation is read from there.
  - A template missing from the bundle is empty, as `StructureTemplateManager.getOrCreate` makes it: 26.3's ancient city pool `walls/no_corners` names `intact_horizontal_wall_stairs_5`, which doesn't exist. Loading no longer fails on it.
  - Candidates are the pool's shuffled elements (unless at the maximum depth) followed by the fallback's, not one or the other, as the spec's draft had it. The fallback isn't aliased.
  - An explicit `dimension_padding` of 0 is checked, since decoding never yields `DimensionPadding.ZERO`; spyglass treated it as absent, now fixed. No 26.3 structure has one.
  - `JigsawPools` is loaded separately from `StructureSets` because it reads most of the bundle's 1,511 templates: 1.2 s (release), against 0.27 s for the sets. Phase 7 binds it lazily in `Session`, like terrain.
  - First timings (release, terrain bound, columns not cached): a plains village of 167 pieces 218 ms, nearly all in surface heights for terrain-matching pieces (one per parent jigsaw); a trial chamber of 267 pieces 8 ms; an ancient city of 107 pieces 1 ms. Bounds aren't in a speed target, but the village figure is for phase 8, with the per-seed column cache.
- 2026-10-26: Phase 5 done. `Starts::with_terrain` checks surface and ocean types, pyramids and temples (lowest corner below sea level rejects), monuments, mansions (rotation, lowest corner below 60 rejects), projected jigsaws (first free height, then padding) and ruined portals (setup, template, rotation and mirror from the context random, then `findSuitableY` over four block columns) at their real positions. Against the structure golden files: 1,640 stubs equal to the game's (mineshafts' 787 unknown); 5,986 of 6,056 placement chunks decided, every one as the game decided, 70 left open (mixed-column mineshafts and ties). Without terrain, mineshafts whose whole column is in their biomes now pass, so 870 starts are certain (175 before), all made by the game. From the bytecode: `findSuitableY` tests the placement's own heightmap, returns `minY + 15` when no height has three solid corners, and the Nether portal is tried in the Overworld's set (its y ranges are ported; its biome check fails). One stub sits behind a tied climate condition the game failed, so the test allows stubs behind ties.
- 2026-10-26: Phase 4 done. Exit criterion 1 is met: every height in the ten golden files (both heightmaps, 81,920 columns each) and every block of their sampled columns matches, 2,041,000 values, through both the chunk-wise path and, for every 13th column, a column's own. `OCEAN_FLOOR_WG` needs `#minecraft:blocks_motion_in_heightmap`, so bundles now include `data/minecraft/tags/block/` (297 files); older bundles must be extracted again. First measurement (release, one thread, `cargo xtask bench`):
  - Binding a seed's terrain: 321 µs, mostly creating its noises. One column in a bound session: 243 µs in the bench (one column, repeated), 430 µs on average over 300 varied columns, of which the density volume is 262 µs, the aquifer's surface-level prefill 141 µs and the scan the rest. One column in a new session: 552 µs in the bench. The game: about 500 µs.
  - 1,000 columns (40 × 25 from 0,0): 462 ms column by column, 24 ms chunk by chunk (one density volume and one aquifer per chunk, which share cell corners and surface levels). `heights_in` uses the chunk path.
  - So the 150 µs target with binding included can't be met: binding alone exceeds it. Split into 150 µs per bound column (missed by 1.6 to 2.9×, for phase 8) and 400 µs for binding (met). The region target is met.
- 2026-10-26: Phase 3 done. The aquifer (`spyglass-terrain`, with `TerrainSettings` loading and binding final density and the aquifer functions with one binder) matches the game block for block in 320 probe columns: 8 columns × 10 seeds, with each column's own density and with densities −1, −0.02 and 0. A mutation that drops the `sim13 > 0` condition fails 160; one that breaks ties the other way fails none (no ties in these columns), so a unit test pins the tie rule. From the bytecode, beyond the spec's phase 1 draft: pressure compares the two statuses' blocks at y, not their fluids; each of the last two pressure terms needs its own positive similarity; the aquifer prefills surface levels over its region as a quart-stepped volume at y 0, which equals points for 26.3 (checked by an ignored test). The terrain spec's open questions are all settled.
  - `cargo xtask oracle-test` listed the density loading test under an old name, so `--exact` skipped it silently; fixed, and the two terrain tests added. All 28 pass.
- 2026-10-26: Phase 3's volume path done. Every type has a volume path (`Sampler::sample_volume`, over the noise crate's new `Volume`), and `interpolated` binds, so 53 of the 55 bundled functions bind (the End's two don't). The probe gained column digests for every named function (8 columns × 10 seeds), full `final_density` columns and `final_density` chunk digests (4 chunks × 10 seeds); all 14,660 density cases match, and a mutation of `fillCell`'s y accumulation fails 604 of them. From the bytecode:
  - Over a volume every sampler samples its inputs over the same volume and combines them per position; `lerp`, `range_choice` and `interval_select` sample every branch. Only `interpolated`, `mul`/`div` (no zero test), `min`/`max` (compare and replace, no range shortcut), unshifted noise (`addToVolume`: scale times frequency first, `dotXz` then y) and `shift_b` (the 4 in each layer's weight, exact) compute differently.
  - Caches aren't modelled. Over a volume they only save work; a point inside a cache's last volume would return the volume's value, but every cache reachable from point-sampled functions (the aquifer's, `find_top_surface`'s density) holds only types whose volume and point values agree, checked over the 26.3 data. Recorded in the density spec.
  - The probe can't tell the noise volume path from the point path: a one-ulp `f64` coordinate difference rarely survives into the `f32` fraction. A unit test pins it.

- 2026-10-26: Phase 2 done. All 55 bundled density functions load, and the 41 that can be bound match the game at all 18 probe blocks for all 10 seeds (9,900 cases), with `cargo xtask oracle-test` still green throughout. From the bytecode:
  - The compiler inlines references, prepares caches, then applies `SLICE_UNIFORM_AXES`, which only wraps nodes in value-preserving slices, so spyglass skips it. Constant specializations test for `ConstantFunction`, which a `cache` of a constant isn't.
  - `old_blended_noise` compiles into existing samplers (`lerp(clamp(main + 0.5, 0, 1), minLimit, maxLimit)` over three `noise` samplers on smeared stacks), so it needed no new evaluation, only `SmearedPerlinNoise` and a stack generic over its noise.
  - `find_top_surface` and `slice` evaluate inputs at other positions, so the per-thread memo now saves and restores its place around them.
  - Ranges are computed at bind time, remembered per function; where the game's `Interval` throws, binding fails.
- 2026-10-26: ADR 0022 accepted; phase 1 committed.
- 2026-10-25: Phase 1 done but for the probe additions, which move to phases 2 and 3. From the bytecode, with three survey passes cross-checked against the oracle:
  - The generator's columns evaluate `final_density` and the aquifer only: no beardifier, no blending, no material rule (so no ore veins or surface blocks). The new height files agree: their block columns hold only stone, air, water and lava. So surface rules are out of the milestone.
  - Terrain is sampled as a **volume**, and the volume path differs from the point path for `interpolated` (different float order), for `mul`/`div` with a zero left operand, `min`/`max` with NaN or signed zeros, and noise octave coordinates with non-power-of-two scales. `min` and `max` are also compiled by their operands' ranges. So spyglass needs a second evaluation path and the game's `Interval` arithmetic: ADR 0022.
  - Our point-path `mul` misses the game's zero short-circuit (two non-constant operands, left ±0 → `+0.0`, right not evaluated). Harmless for the climate so far; fixed first in phase 2.
  - The structure spec had two errors: surface and ocean checks are at `getFirstOccupiedHeight`, one **below** the heightmap (confirmed on the height and structure files: ocean ruin and shipwreck stubs are at `OCEAN_FLOOR_WG − 1`), and the ruined portal's check is at `(minX, y, minZ)`, not the template's centre. Both fixed in the spec.
  - 26.3 has one cache type (`cache`), not 1.21's four; cell sizes live in each `interpolated`.
  - The game's `getBaseHeight` costs about 0.5 ms a column (164,000 per seed in 80 s, 10 seeds on 16 cores), so the 150 µs target is three times faster than the game. It stays for now; phase 4 measures.
- 2026-10-25: Started, with the decisions agreed as recommended.
- 2026-10-25: Drafted from milestone 07, the code at fa52e5c, the 26.3 bundle's noise settings, density functions and material rules, and bench figures from m06-seed-search.
