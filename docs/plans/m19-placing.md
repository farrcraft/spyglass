# m19. Placing features, certain or not

- Status: Draft
- Milestone: [19. Features](../roadmap/19-features.md)
- Started: YYYY-MM-DD
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

- [ ] **Spike:** each target feature's code in 26.2, 26.3 and 26.4-snapshot-6, and where the versions differ.
- [ ] **Placing:** the five features with their blocks, in order, on a chunk's blocks; the modifiers that read blocks, on them.
- [ ] **Certainty:** each reason above, measured on the golden seeds: how many tries each turns into candidates. Port mineshaft pieces, or place other early features, where they account for most of them.
- [ ] **Oracle:** each try's result, and the second run with neighbours decorated first.
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
