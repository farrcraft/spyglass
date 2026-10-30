# m19. Decoration: where features try to place

- Status: Draft
- Milestone: [19. Features](../roadmap/19-features.md)
- Started: YYYY-MM-DD
- Completed: YYYY-MM-DD

## Goal

Where each target feature tries to place in a chunk, exactly as the game decorates it. That covers the order of steps and features, each feature's random seed, and its placement modifiers. The targets are `monster_room` (dungeons), `amethyst_geode`, `desert_well`, `fossil` (coal and diamond) and `lake` (lava lakes). This plan stops at the tries: a try is a position the game hands to the feature, which may then decline to place. Whether it places is [m19-placing](m19-placing.md)'s. The oracle gains feature golden files that record every try and its result, which both plans compare against.

It meets none of the milestone's exit criteria alone. It's the ground for the first one.

## Approach

**What the game does** (26.3, `ChunkGenerator.applyBiomeDecoration`; the spike confirms, and finds 26.2's and 26.4's differences with `cargo xtask version-diff --code`):

- **Which features run in a chunk:** the biomes stored in the chunk and its 8 neighbours, kept to those the biome source can give. A placed feature runs if any of those biomes lists it.
- **The order:** step by step (`raw_generation`, `lakes`, `local_modifications`, `underground_structures`, `surface_structures`, …), with the step's structure pieces first and then its features. Within a step, features go in one global order per dimension: `FeatureSorter.buildFeaturesPerStep` sorts every possible biome's lists into one order, and fails if two biomes disagree.
- **The seeds:** a `WorldgenRandom`, with `setDecorationSeed(seed, chunk min x, chunk min z)`, then `setFeatureSeed(decoration seed, index, step)` before each feature. `index` is the feature's place in its step's global order. A feature's tries depend only on its own seed, never on the features before it. What it finds at those positions does depend on them.
- **The modifiers** turn the chunk's corner into positions. The targets use `count`, `rarity_filter`, `in_square`, `height_range`, `biome`, `heightmap`, `offset`, `block_predicate_filter`, `environment_scan` and `surface_relative_threshold_filter`. `biome` checks the biome at the try's position (the chunk's stored quart). Height providers come from `spyglass_data::providers`, from the carvers' work.

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

**Modifiers that read blocks.** `heightmap` (`MOTION_BLOCKING`), `block_predicate_filter`, `environment_scan` and `surface_relative_threshold_filter` read the chunk as decoration finds it. This plan reads the chunk before features, which is the game's answer unless an earlier feature in the chunk, or a neighbour's, changed those blocks. Each try says whether it read blocks. The golden comparison then expects exact positions for tries that read none, and for the others, reports the differences separately.

**The oracle** ([oracle.md](../specs/oracle.md), [ADR 0049](../adr/0049-the-oracle-generates-chunks-with-stand-ins.md)): `cargo xtask oracle features` decorates each golden chunk on its own. Its 3 × 3 neighbours are generated through carvers, not decorated, and there are no structures, as for block golden files. For each chunk, the file lists every try of every placed feature in steps up to `surface_structures`: the step, the index, the placed feature, the position the feature is called with, and whether it placed. It also lists each step's global order. How the harness sees tries is the spike's to decide. One way is a copy of the decoration loop that calls the game's own placed and configured features; another is a level wrapper. Either is an ADR, or an amendment to 0049.

**The golden area.** Tries are rare: a well is one try in a thousand desert chunks, and a fossil one in 64. The block files' 4 × 4 squares aren't enough. Each golden seed gets a square of 32 × 32 chunks around the origin, and the harness adds a desert window found by its own decoration, so wells place. The spike measures the cost.

## Tasks

- [ ] **Spike:** decoration in 26.2, 26.3 and 26.4-snapshot-6: `applyBiomeDecoration`, `FeatureSorter`, `WorldgenRandom`'s decoration and feature seeds and the random source behind them, and each target modifier. Note what each version changes, and turn any change that doesn't show in the data into a switch ([ADR 0026](../adr/0026-how-game-versions-coexist.md)). Record findings here.
- [ ] **ADR:** the `spyglass-feature` crate and its place in the layers (`docs/architecture.md`).
- [ ] **Data:** placed features and the target configured features, in both layouts, with fixture tests for each modifier's loading and errors.
- [ ] **The order:** the global order per step from every possible biome's lists, with the game's failure when they disagree. Fixture tests, and an ignored test against the golden files' orders.
- [ ] **Seeds and tries:** decoration and feature seeds in `spyglass-rng` ([rng.md](../specs/rng.md)), which features run in a chunk, and each modifier. Tries in decoration order.
- [ ] **Oracle:** `cargo xtask oracle features`, in the core and each adapter, for the golden seeds and dimensions; `golden` runs it. Its file format in oracle.md. A determinism check, as the other files have.
- [ ] **Comparison:** an ignored test, in `cargo xtask oracle-test`'s list, comparing every try's position with the game's, for every supported version. Position-only tries must match exactly; tries that read blocks are counted apart.
- [ ] **Cost:** `cargo xtask bench` gains a chunk's tries. Record it here.
- [ ] **Spec:** `docs/specs/features.md`: decoration, the order, seeds, modifiers and tries, with per-version notes.
- [ ] `cargo xtask check` and `cargo xtask oracle-test`. Complete the plan.

## Verification

- Every try of the target features in the golden area, for every golden seed, dimension and supported version, is at the game's position. Tries whose modifiers read blocks are counted apart, with the reason.
- Each step's global order equals the game's in every version and dimension.
- Fixture tests for every modifier, and for the order's failure.

## Durable outputs

- [ ] `docs/specs/features.md`: decoration, the order, seeds, modifiers, tries.
- [ ] rng.md: the decoration and feature seeds.
- [ ] oracle.md: feature golden files and how the harness makes them.
- [ ] An ADR for the crate, and one (or an amendment to ADR 0049) for how the oracle decorates.
- [ ] architecture.md: the new crate.

## Notes

- 2026-10-30: **Before starting.** In 26.3's Overworld, the targets are nearly all of the first four steps in most biomes. Every try is seeded on its own, so tries can be exact without placing anything. Whether a try places reads blocks that earlier features, neighbours decorated earlier and structure pieces change, so placing and certainty are a plan of their own. The Nether's fossils are the `nether_fossil` structure, already placed and located, not a feature.
