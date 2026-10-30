# m19. Decoration: where features try to place

- Status: In progress
- Milestone: [19. Features](../roadmap/19-features.md)
- Started: 2026-10-30
- Completed: YYYY-MM-DD

## Goal

Where each target feature tries to place in a chunk, exactly as the game decorates it. That covers the order of steps and features, each feature's random seed, and its placement modifiers. The targets are `monster_room` (dungeons), `amethyst_geode`, `desert_well`, `fossil` (coal and diamond) and `lake` (lava lakes). This plan stops at the tries: a try is a position the game hands to the feature, which may then decline to place. Whether it places is [m19-placing](m19-placing.md)'s. The oracle gains feature golden files that record every try and its result, which both plans compare against.

It meets none of the milestone's exit criteria alone. It's the ground for the first one.

## Approach

**What the game does** (26.3, `ChunkGenerator.applyBiomeDecoration`; the spike confirms, and finds 26.2's and 26.4's differences with `cargo xtask version-diff --code`):

- **Which features run in a chunk:** the biomes stored in the chunk and its 8 neighbours, kept to those the biome source can give. A placed feature runs if any of those biomes lists it.
- **The order:** step by step (`raw_generation`, `lakes`, `local_modifications`, `underground_structures`, `surface_structures`, …), with the step's structure pieces first and then its features. Within a step, features go in one global order per dimension: `FeatureSorter.buildFeaturesPerStep` sorts every possible biome's lists into one order, and fails if two biomes disagree.
- **The seeds:** a `WorldgenRandom`, with `setDecorationSeed(seed, chunk min x, chunk min z)`, then `setFeatureSeed(decoration seed, index, step)` before each feature. `index` is the feature's place in its step's global order. A feature's tries don't depend on the features before it.
- **Tries and the feature share the random.** Positions go depth first: each try passes the remaining modifiers and reaches the feature before the next try's modifiers draw. So a placed feature's later tries depend on how many numbers its earlier ones drew, and that depends on whether they placed. Of the targets, only dungeons have more than one try per chunk (`count` 10 and 4). A dungeon's first try is exact here. Its later tries are exact only while every earlier try declined, which takes the monster room's own check; [m19-placing](m19-placing.md) makes them all exact.
- **The modifiers** turn the chunk's corner into positions. The targets use `count`, `rarity_filter`, `in_square`, `height_range`, `biome`, `heightmap`, `offset`, `block_predicate_filter`, `environment_scan` and `surface_relative_threshold_filter`. `biome` checks the biome at the try's block, as the level's `getBiome` zooms it (`ChunkGenerator::block_biome`), not the stored quart. Height providers come from `spyglass_data::providers`, from the carvers' work.

**In the targets' biomes, the early steps are nearly theirs alone.** In 26.3's Overworld, `lakes` holds the lava lakes, `local_modifications` the geode, `underground_structures` fossils and dungeons, and `surface_structures` the desert well. A few biomes put other features in those steps:

- `forest_rock`;
- icebergs and blue ice;
- `large_dripstone`;
- sulfur springs and pools;
- the Nether's basalt and deltas, and the End's islands.

Mineshafts, buried treasure, trail ruins and trial chambers place their pieces at the start of `underground_structures`, and surface structures at the start of `surface_structures`. Those matter for whether a try places, so they're [m19-placing](m19-placing.md)'s. The global order and the seeds still need every feature of every biome, by id.

**Where it lives.** A new crate after `spyglass-blocks`, `spyglass-feature`, since placing needs a chunk's blocks. It's a crate boundary, so it gets an ADR. It holds:

- `FeatureSettings`: a dimension's placed and configured features, its steps and global order. Every placed feature is loaded by id. Only the targets and their modifiers are loaded as types. 26.3 keeps configured features in `worldgen/feature` with their config flattened, and 26.2 in `worldgen/configured_feature`.
- `Decorator::tries(chunk)`: for one seed, each target feature's tries in the chunk, in decoration order, each with its placed feature, step, index and position.

The API (`Session::feature_tries`) and the CLI come with [m19-queries](m19-queries.md), unless the oracle tests want them sooner.

**Modifiers that read blocks.** `heightmap` (`MOTION_BLOCKING`), `block_predicate_filter`, `environment_scan` and `surface_relative_threshold_filter` read the chunk as decoration finds it. This plan reads the chunk before features, which is the game's answer unless an earlier feature in the chunk, or a neighbour's, changed those blocks. Each try says whether it read blocks, and whether it follows a try of its feature that may have placed. The golden comparison expects exact positions for the rest, and reports these differences separately.

**The oracle** ([oracle.md](../specs/oracle.md), [ADR 0049](../adr/0049-the-oracle-generates-chunks-with-stand-ins.md)): `cargo xtask oracle features` decorates each golden chunk on its own. Its 3 × 3 neighbours are generated through carvers, not decorated, and there are no structures, as for block golden files. For each chunk, the file lists every try of every placed feature in steps up to `surface_structures`: the step, the index, the placed feature, the position the feature is called with, and whether it placed. It also lists each step's global order. How the harness sees tries is the spike's to decide. One way is a copy of the decoration loop that calls the game's own placed and configured features; another is a level wrapper. Either is an ADR, or an amendment to 0049.

**The golden area.** Tries are rare: a well is one try in a thousand desert chunks, and a fossil one in 64. The block files' 4 × 4 squares aren't enough. Each golden seed gets a square of 32 × 32 chunks around the origin, and the harness adds a desert window found by its own decoration, so wells place. The spike measures the cost.

## Tasks

- [x] **Spike:** decoration in 26.2, 26.3 and 26.4-snapshot-6: `applyBiomeDecoration`, `FeatureSorter`, `WorldgenRandom`'s decoration and feature seeds and the random source behind them, and each target modifier. Note what each version changes, and turn any change that doesn't show in the data into a switch ([ADR 0026](../adr/0026-how-game-versions-coexist.md)). Record findings here.
- [x] **ADR:** the `spyglass-feature` crate ([ADR 0052](../adr/0052-features-in-their-own-crate.md), proposed) and its place in the layers (`docs/architecture.md`).
- [ ] **Data:** placed features and the target configured features, in both layouts, with fixture tests for each modifier's loading and errors.
- [ ] **The order:** the global order per step from every possible biome's lists, with the game's failure when they disagree. Fixture tests, and an ignored test against the golden files' orders. _Sorter, list loading and fixture tests done; every version's real lists load and sort (`every_dimension_sorts`). The comparison with the game's order waits for the oracle._
- [x] **Seeds:** `WorldgenRandom` over Xoroshiro, and the decoration and feature seeds, in `spyglass-rng` ([rng.md](../specs/rng.md)), checked against the oracle's probe files in every version.
- [ ] **Tries:** which features run in a chunk, and each modifier. Tries in decoration order.
- [ ] **Oracle:** `cargo xtask oracle features`, in the core and each adapter, for the golden seeds and dimensions; `golden` runs it. Its file format in oracle.md. A determinism check, as the other files have.
- [ ] **Comparison:** an ignored test, in `cargo xtask oracle-test`'s list, comparing every try's position with the game's, for every supported version. Position-only tries must match exactly; tries that read blocks are counted apart.
- [ ] **Cost:** `cargo xtask bench` gains a chunk's tries. Record it here.
- [ ] **Spec:** `docs/specs/features.md`: decoration, the order, seeds, modifiers and tries, with per-version notes.
- [ ] `cargo xtask check` and `cargo xtask oracle-test`. Complete the plan.

## Verification

- Every try of the target features in the golden area, for every golden seed, dimension and supported version, is at the game's position. Tries whose modifiers read blocks, and dungeon tries after one that may have placed, are counted apart, with the reason.
- Each step's global order equals the game's in every version and dimension.
- Fixture tests for every modifier, and for the order's failure.

## Durable outputs

- [ ] `docs/specs/features.md`: decoration, the order, seeds, modifiers, tries.
- [ ] rng.md: the decoration and feature seeds.
- [ ] oracle.md: feature golden files and how the harness makes them.
- [ ] An ADR for the crate, and one (or an amendment to ADR 0049) for how the oracle decorates.
- [ ] architecture.md: the new crate.

## Notes

- 2026-10-30: **How the oracle sees tries.** Wrap each target's feature in the registry with one that records the origin it's called with and the result, and delegates to the real feature; then run the game's own `applyBiomeDecoration`, untouched. For the level, a real `WorldGenRegion`, built by its constructor over stand-ins ([ADR 0049](../adr/0049-the-oracle-generates-chunks-with-stand-ins.md)'s way): a server level answering `getSeed`, `getLevelData`, `getChunkSource().randomState()`, `dimensionType` and `uncachedBiomeResolver`; chunk holders handing out the neighbourhood's proto chunks; and the `FEATURES` chunk step. The region's own `setBlock`, write-radius checks, heightmaps and ticks then run as the game's. In 26.3 `Feature` is an interface and configured features are records, so the wrapper is per adapter.
- 2026-10-30: **Spike, 26.2 and 26.4** (diffs of the same classes):
  - 26.2's loop, seeds and random are 26.3's. It places through `PlacedFeature.placeWithContext`, a stream with a `flatMap` per modifier, which in a sequential stream is depth first too, so the order of draws is the same. Its modifiers are the same algorithms as `getPositions` streams.
  - 26.2's sorter numbers features in an `Object2IntOpenHashMap`, by equality, where 26.3's `Reference2IntOpenHashMap` goes by identity. `PlacedFeature` is a record, so in 26.2 two placed features with the same configured feature and equal modifiers share a number. 26.2's data has four such pairs (`acacia` and `acacia_checked`, and oak's, pine's and spruce's), none in a biome's list, so vanilla's order doesn't change. It's a switch, for data packs.
  - 26.2's `desert_well` has no `offset` or `block_predicate_filter`: its sand check is in the feature. 26.4-snapshot-6's placed features are 26.3's.
  - 26.4: `height_range` resolves anchors through `VerticalAnchor.Context.from(generator, level)`, the same bounds as 26.3's `WorldGenerationContext` (the greater bottom, the lesser height). Modifiers gain `modifyXzDomain`, how far each can move a try sideways, which m19-placing may use. Otherwise the same.
- 2026-10-30: **Spike, 26.3** (`target/version-diff/26.3`, and `javap -v` for lambdas):
  - `applyBiomeDecoration`: the random is always `WorldgenRandom` over a `XoroshiroRandomSource`, whatever `legacy_random_source` says. `setDecorationSeed(seed, x, z)` uses the chunk's minimum block x and z. Each step first places its structures, in the structure registry's order grouped by step, with `setFeatureSeed(decoration seed, j, step)` for the j-th; then its features, by ascending global index, with `setFeatureSeed(decoration seed, index, step)` and `FeaturePlacer.placeWithBiomeCheck(feature, random, section origin)`. The loop runs to `max(11, steps the sorter made)`.
  - Features run if a biome stored anywhere in the 3 × 3 chunks (every section's palette) lists them, kept to `possibleBiomes()`.
  - `WorldgenRandom` over Xoroshiro: `next(bits)` is the top `bits` of Xoroshiro's `nextLong()`; `nextInt(bound)`, `nextLong()`, `nextFloat()` and the rest are `LegacyRandomSource`'s on top of `next`, `setDecorationSeed`'s two `nextLong` calls included. `setDecorationSeed`: `setSeed(seed)`, `a = nextLong() | 1`, `b = nextLong() | 1`, `s = (x·a + z·b) ^ seed`, `setSeed(s)`. `setFeatureSeed`: `setSeed(seed + index + 10000·step)`, the product an `int`.
  - `FeatureSorter.buildFeaturesPerStep(List.copyOf(possibleBiomes()), …)`: each placed feature gets an id the first time it's seen, biome by biome in that list's order (the climate parameter list's, deduplicated, which spyglass's `BiomeNames` keeps), step by step. Nodes are `(step, id)`, ordered by step then id. Each biome's list, all steps flattened, adds an edge from each entry to the next. A depth-first search from each node in order (`Graph.depthFirstSearch`: children in the same order, a node emitted after its children) gives a list that, reversed and split by step, is each step's order. A cycle is an error.
  - `FeaturePlacer` (new in 26.3): positions go through the modifiers depth first, from a stack, and each reaches `Feature.place` before the next is modified. The feature draws from the same random.
  - The modifiers that read blocks: `heightmap` keeps a try only above the level's bottom, at `level.getHeight(type, x, z)`; `offset` samples three int providers (26.3's `OffsetPlacement`; 26.2 has `random_offset`); `environment_scan` steps up to `max_steps` while `allowed_search_condition` holds and stops at the first `target_condition`; `surface_relative_threshold_filter` and `block_predicate_filter` test the level. Block predicates used: `matching_blocks`, `matching_block_tag`, `not`, `all_of`, `inside_world_bounds`, `volume_match`.
  - Modifiers: `in_square` draws `nextInt(16)` for x, then z. `rarity_filter` passes when `nextFloat() < 1 / chance`. `count` samples its int provider. `height_range` samples its height provider. `biome` asks the level's `getBiome` at the block.

- 2026-10-30: **Before starting.** In 26.3's Overworld, the targets are nearly all of the first four steps in most biomes. Every try is seeded on its own, so tries can be exact without placing anything. Whether a try places reads blocks that earlier features, neighbours decorated earlier and structure pieces change, so placing and certainty are a plan of their own. The Nether's fossils are the `nether_fossil` structure, already placed and located, not a feature.
