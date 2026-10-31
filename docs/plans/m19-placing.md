# m19. Placing features, certain or not

- Status: In progress
- Milestone: [19. Features](../roadmap/19-features.md)
- Started: 2026-10-30
- Completed: YYYY-MM-DD

## Goal

Whether each try from [m19-decoration](completed/m19-decoration.md) places, and the blocks it writes, for the five target features. Each answer is certain, or a candidate with its reason. It meets the milestone's first exit criterion: every feature in scope matches the game in the golden area, and no answer reported as certain is wrong.

## Approach

**Placing in order.** A chunk's tries run in decoration order on its blocks after carvers (`Session::chunk_blocks`). Each feature checks the blocks and, if it places, writes its own, which the next feature sees. A lake can turn a dungeon's wall to lava, and a geode can open its floor. So every target feature is placed in full, not only checked:

- **`lake`:** the lava lake's shape, its checks (no air or other fluid at the edge, at most one block of liquid above), and its blocks.
- **`amethyst_geode`:** the shell from its noise and distances, the count of invalid blocks against its threshold, and its layers and buds.
- **`monster_room`:** the room's size, the count of openings in its walls (1 to 5) with solid floor and ceiling, then its walls, spawner and chests. The chests' contents are [20](../roadmap/20-loot.md)'s.
- **`fossil`:** its template and rotation, the count of empty corners against `max_empty_corners_allowed`, and its processors (`fossil_rot`, `fossil_coal`, `fossil_diamonds`). Templates and processors come from the bundle, as the jigsaw code loads them.
- **`desert_well`:** the sand under it and the air around it, then its blocks.

The modifiers that read blocks (`heightmap`, `block_predicate_filter`, `environment_scan`, `surface_relative_threshold_filter`) read these blocks too, so tries that m19-decoration counted apart become exact.

**What spyglass can't place yet makes a try a candidate, never a guess.** Each reason is named in the spec, and each answer carries it:

- **Structure pieces placed earlier.** Mineshafts, buried treasure, trail ruins and trial chambers place their pieces at the start of `underground_structures`, before dungeons and fossils. Surface structures place theirs before the desert well. Spyglass lays out jigsaw structures but not mineshafts' pieces. A try within reach of a start whose pieces it can't lay out is a candidate. Mineshafts are common, so the plan measures how many dungeons this costs. It ports mineshaft pieces if that's most of the candidates.
- **Other features in the early steps:** `forest_rock`, icebergs and blue ice, `large_dripstone`, sulfur springs and pools, and the Nether's and the End's. Where one could reach a try, the try is a candidate, unless it's cheap to place exactly.
- **Neighbours decorated earlier.** A feature writes past its chunk, and whether a neighbour has decorated first depends on how the world was explored, not on the seed. A try whose checked blocks a neighbour's features could change is a candidate, unless placing the neighbour first gives the same answer.
- **Tied biomes,** as for blocks ([material.md](../specs/material.md#ties)).

**The oracle.** The feature golden files gain each placed feature's result. A second run decorates every neighbour before the chunk. Any try whose result differs between the two runs must be a candidate in spyglass, and the comparison checks that.

## Tasks

- [x] **Spike:** each target feature's code in 26.2, 26.3 and 26.4-snapshot-6, and where the versions differ. _Findings in the notes; the full reconstructions are kept locally in `target/spike/placing/`, never committed._
- [x] **Block data:** the per-state behaviour placing reads (`isSolid`, `liquid`, `canBeReplaced`, `isSolidRender`, the fluid state, and 26.2's `blocksMotion` and leaves), which no data report has. How the bundle gets it is an ADR. _The extractor's block report ([ADR 0054](../adr/0054-block-behaviour-from-the-running-game.md)), with `shape_updater` added for templates' neighbours._
- [x] **Placing:** the five features with their blocks, in order, on a chunk's blocks; the modifiers that read blocks, on them. _`Region`, `Placer` and `Decorator::decorate`; `tests/placing.rs` compares every call with the game's._
- [ ] **Certainty:** each reason above, measured on the golden seeds: how many tries each turns into candidates. Port mineshaft pieces, or place other early features, where they account for most of them.
- [x] **Oracle:** each try's result, and the second run with neighbours decorated first. _`after_neighbours` in the feature golden files; neighbours' full decoration needed an unlit light engine stand-in._
- [ ] **Comparison:** an ignored test: every certain answer is the game's, and every try whose result differs between the oracle's runs is a candidate.
- [ ] **Cost:** a chunk's features in `cargo xtask bench`.
- [ ] **Spec:** features.md: each feature, the certainty rules and their reasons.
- [ ] `cargo xtask check` and `cargo xtask oracle-test`. Complete the plan.

## Verification

- In the golden area of every golden seed and supported version, every certain answer is the game's, and every game's feature is an answer (certain or a candidate).
- Each candidate has a reason, and the share of candidates per reason is recorded here.

## Durable outputs

- [ ] features.md: the five features, certainty and its reasons.
- [ ] oracle.md: results in feature golden files, and the second run.

## Notes

- 2026-10-30: **Neighbours decorated first barely matter.** The second oracle run (all 8 neighbours decorated in full first) changes 4 of 26.3's 2,684 golden chunks: an underground lava lake's scan lands 4 blocks higher in one and the lake declines in another; in two, a dungeon that placed on its own declines, which moves that placed feature's later tries. The other 2,680 chunks' calls are the same.
- 2026-10-30: **Other features in the same steps.** Placing runs every placed feature of the steps up to the last target's; one spyglass doesn't place that reaches its feature puts the rest of the chunk in doubt (`Doubt::OtherFeature`). That cost 9,058 of 26.3's 37,842 calls, nearly all after large dripstone (7,028) and forest rocks (2,016), so both are placed exactly now (`block_blob`; `large_dripstone`, 26.4's `large_speleothem` with its `base_block`). After them: 26.3 37,318 compared and matching, 524 not compared (510 after a tie, 14 after a sulfur pool); 26.2 37,330 and 529; 26.4-snapshot-6 37,718 and 124. No differences.

- 2026-10-30: **Placing matches the game.** Every golden chunk of every seed placed on its carved 3 × 3, in every version, compared call by call (position, and whether it placed): no differences. Calls not compared, all after a tie in the chunk (a block or biome a tie decides): 26.2 503 of 37,859, 26.3 499 of 37,842, 26.4-snapshot-5 and 6 54 of 37,842. By feature (26.3): dungeons 365 and 74, geodes 30, underground lakes 30. Shape updates and waterlogging beside templates put none in doubt in the golden area. A chunk takes about 150 ms (its 9 carved chunks, shared with the neighbouring golden chunks, and the placing), measured in the test, which runs in parallel.
- 2026-10-30: **What placing found.** The geode's 26.3 noise is `NormalNoise.createParity`, now in `spyglass-noise` and bit-exact against the real class (checked once with the spike's vectors, which stay out of the repo, ADR 0008). The block report gained `shape_updater`: the class whose `updateShape` a block runs, since a template beside a fence or plant can change it, and falling blocks, fluids, slabs and brushable blocks only schedule a tick (their bytecode). Templates read through a new `Resources::structure_template`.

- 2026-10-30: **Spike: the level** (`WorldGenRegion`, `ProtoChunk`, `Heightmap`, the modifiers that read blocks):
  - **Writes** reach the chunk and its 8 neighbours (`blockStateWriteRadius(1)`). A write outside them is dropped, logged, and returns false. Writes outside the build height do nothing. The flags (2 or 3) change nothing on a proto chunk except bit 16, which skips the post-processing mark.
  - **Heightmaps:** all six exist during decoration. Writes update only the four final ones (`WORLD_SURFACE`, `OCEAN_FLOOR`, `MOTION_BLOCKING`, `MOTION_BLOCKING_NO_LEAVES`). The `*_WG` ones stay as terrain left them. `getHeight` is the first free y, from the heightmap of the chunk holding the column, which may be a neighbour. 26.2 primes them at the start of features; 26.3 at the end of terrain; 26.4-snapshot-6 then copies `WORLD_SURFACE` and `OCEAN_FLOOR` over the `_WG` ones.
  - **Heightmap predicates:** 26.2's `MOTION_BLOCKING` and `OCEAN_FLOOR` use `blocksMotion()` (legacy solid without cobweb and bamboo sapling), and `_NO_LEAVES` also leaves out `LeavesBlock`. From 26.3 they're the tags `#blocks_motion_in_heightmap` and `#blocks_motion_in_heightmap_no_leaves`: data.
  - **Modifiers** draw nothing. `heightmap` keeps `(x, h, z)` only if `h > min y`. `environment_scan`: nothing if the origin isn't allowed; up to `max_steps` steps of test, move, stop outside the build height or where not allowed; then one more test. `surface_relative_threshold_filter`: `h + min ≤ y ≤ h + max` in `long`. `volume_match` (26.3 on): inclusive, x outer, then z, then y, stopping at the first failure. `inside_world_bounds` checks y only. 26.4-snapshot-6 adds `below_heightmap`.
  - **26.2 skips a feature whose origin is outside the writable 3 × 3** (`Feature.place`'s `ensureCanWrite`), drawing nothing. From 26.3 that check is gone and the feature runs with its writes dropped. No target's origin leaves its chunk, so it doesn't matter for them.
  - Modifiers that read blocks run on the live region, interleaved with placing, so they can't be computed from the carved chunk ahead of time.
- 2026-10-30: **Spike: the features.** Each is the same algorithm in all three versions, except the desert well and the geode's noise:
  - **`lake`:** no draws and false if `y ≤ min y + 4`; then 4 to 7 blobs, six `nextDouble` each, into a 16 × 16 × 8 grid at `origin − (8, 4, 8)`. One check pass over every edge cell before any write: liquid at `y ≥ 4`, or at `y < 4` neither solid nor the lake's fluid, declines. Then cave air (`y ≥ 4`, with a tick and `markAboveForPostProcessing`) or fluid (`y < 4`), skipping `#features_cannot_replace`; then the stone barrier on solid edge cells not in `#lava_pool_stone_cannot_replace`, with `nextInt(2)` for each edge cell at `y ≥ 4` only. Freezing is water's only.
  - **`monster_room`:** two `nextInt(2)` sizes; a scan (no draws) that declines on a non-solid floor or ceiling cell, or unless 1 to 5 wall cells at the room's bottom have air there and above. Placing draws `nextInt(4)` per floor cell over solid ground (mossy or not), two chests of up to 3 tries each (two draws a try; a try needs air and exactly one solid side), a `nextLong` per chest written (the loot seed, from the region's block entity), and `nextInt(4)` for the spawner's mob. Air is cave air; the ceiling is never written. The chest's facing (`StructurePiece.reorient`) reads `isSolidRender`, not `isSolid`.
  - **`amethyst_geode`:** draws: points (`nextInt(2) + 3`), `nextDouble`, the crack's `nextFloat`, then per point three `nextInt(3) + 4` and a `nextInt(2) + 1`. A point's block that's air or in `#geode_invalid_blocks` counts, and the second declines, before any write. Its noise has its own random from the world seed: 26.2's `NormalNoise` in double, and from 26.3 a float `NormalNoise.createParity(-4, [1.0])` that `spyglass-noise` doesn't have yet. The two agree to about 1e-7, so a block at a layer's edge can differ: each version needs its own.
  - **`fossil`:** `nextInt(4)` rotation, `nextInt(8)` template (the overlay uses the same index), and y = `max(lowest OCEAN_FLOOR_WG over the footprint, capped at the origin's y, − 15 − nextInt(10), min y + 10)`. More than 4 of the box's 8 corners empty (air, or water or lava without waterlogging) declines. Each pass (bones with `fossil_rot`, then the overlay with `fossil_coal` or `fossil_diamonds`) draws `nextInt(1)` for the palette and a `nextFloat` per template block (`fossil_rot`'s integrity), processes every block before writing any, and then updates the shapes at its edge. The clip box (the chunk ± 16) never clips. 26.4-snapshot-6's `CornerConstraint` is the jigsaw's, not the fossil's.
  - **`desert_well`:** 26.2's feature walks down to sand from above the heightmap, declines where a column has air both one and two below, writes 112 blocks, and draws two `nextInt(5)` for the suspicious sand (loot seed `pos.asLong()`). From 26.3 the check is data (sand below, no air in the 5 × 2 × 5 under it, the biome one block lower), and the well is an `overlay` of three `template` features drawing 11 numbers. It's the same well plus 15 air cells, with the sand picked from a different list. The same shape with different checks and draws, so it's two algorithms, not a switch.
  - **Block data** none of the data reports has: `isSolid` (legacy solid, from collision shapes), `liquid`, `canBeReplaced`, `isSolidRender`, the fluid state, and for 26.2's heightmaps `blocksMotion` and leaves. They're the game's code, not data, so the bundle can only get them from the running game.
  - **Shape updates:** template placing (the well from 26.3, fossils) updates the neighbours' shapes, which can break a plant or join a fence beside it. Underground it rarely matters; where it could, a try is a candidate until it's ported.
