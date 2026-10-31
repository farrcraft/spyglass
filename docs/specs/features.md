# Features

Status: draft. Covers decoration (`ChunkGenerator.applyBiomeDecoration`) as far as where each placed feature tries to place: the decoration steps, the feature order every biome's lists sort into, each feature's seed, and the placement modifiers that turn a chunk's corner into tries. Whether a try places, and what it writes, comes later ([roadmap](../roadmap/README.md)). Implemented in `spyglass-feature` ([ADR 0052](../adr/0052-features-in-their-own-crate.md)), with the random in `spyglass-rng` ([rng.md](rng.md#worldgen-seeding-worldgenrandom)). Checked against the oracle's [feature golden files](oracle.md#feature-golden-files) in every supported version: the order in all three dimensions, and every try of the targets (dungeons, geodes, desert wells, fossils, lava lakes) whose position doesn't depend on blocks.

Ported from the 26.2, 26.3 and 26.4 bytecode.

## Data

A biome's `features` is a list of up to 11 holder sets of placed features, one per decoration step: `raw_generation`, `lakes`, `local_modifications`, `underground_structures`, `surface_structures`, `strongholds`, `underground_ores`, `underground_decoration`, `fluid_springs`, `vegetal_decoration`, `top_layer_modification`. A holder set is a `#tag` of `tags/worldgen/placed_feature`, an id, an inline placed feature, or a list of ids and inline ones. Placed features are in `worldgen/placed_feature`: a `feature` (an id or inline) and a `placement` list of modifiers. Configured features are in `worldgen/feature` from 26.3, with their configuration flattened into the object, and in `worldgen/configured_feature` in 26.2.

## The order

`FeatureSorter.buildFeaturesPerStep`, over the biome source's possible biomes in their order (`possibleBiomes()`: the climate parameter list's, without repeats, which is spyglass's `BiomeNames`; the End's and the checkerboard's fixed lists):

1. Each placed feature gets a number the first time it's seen: biome by biome, step by step, in list order. A registry entry is one feature wherever it's named; each inline one is its own.
2. A node is a feature in a step, `(step, number)`, ordered by step, then number. The same feature in two steps is two nodes.
3. Each biome's lists, every step flattened into one list, add an edge from each entry to the next, across steps too.
4. A depth-first search (`Graph.depthFirstSearch`) from each node in order, skipping those already done, visits a node's successors in order and emits the node after them. A node reached again while it's being searched is a cycle: the lists disagree, and the game refuses them ("Feature order cycle found").
5. The emitted list, reversed and split by step, is each step's order. A feature's index in its step is what its seed uses.

_Versions:_ 26.2's numbers features by equality (`Object2IntOpenHashMap`), 26.3's by identity (`Reference2IntOpenHashMap`). `PlacedFeature` is a record, so in 26.2 two placed features with the same configured feature and equal modifiers are one feature. Vanilla's lists never name two such features, so the order is the same; spyglass numbers them apart and refuses 26.2 lists that name two equal ones.

## Decoration

For chunk `(cx, cz)`, with `x0 = 16·cx` and `z0 = 16·cz`:

- **The random** is a `WorldgenRandom` over a `XoroshiroRandomSource` in every version, whatever the noise settings' `legacy_random_source`. `setDecorationSeed(seed, x0, z0)` gives the decoration seed.
- **Each step** places its structures' pieces first, then its features in ascending index. Before each feature, `setFeatureSeed(decoration seed, index, step)`. A feature's tries don't depend on the features before it.
- **Which features run:** those a biome lists, for any biome stored in the chunk and its 8 neighbours, kept to the possible biomes. A try that passes a `biome` modifier is in such a biome, so tries are the same if every target feature is run.
- **The origin** handed to the first modifier is `(x0, 16·(min y >> 4), z0)`: the chunk's corner at the bottom of the level's lowest section.

### Tries

The modifiers turn the origin into positions, depth first: each position goes through every remaining modifier, and reaches the feature, before the next position is made (26.3's `FeaturePlacer`, from a stack; 26.2's `PlacedFeature`, through a sequential stream's `flatMap`, which is depth first too). A feature draws from the same random, so a placed feature's later tries depend on what its earlier ones drew.

A try that reaches the feature is spyglass's `Try`. It's **exact** unless a modifier before it read blocks, or an earlier try of the same placed feature drew numbers that aren't known. Of the targets, only dungeons have more than one try per chunk (`count` 10 and 4): a dungeon that declines draws exactly two numbers (`nextInt(2)` for each size), and one that places draws more.

### Modifiers

| Type                               | What it does                                                                                                                                                                   |
| ---------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `count`                            | The position `count` times: an int provider, 0 to 256, sampled once.                                                                                                           |
| `rarity_filter`                    | The position if `nextFloat() < 1f / chance` (`float` division); NaN fails.                                                                                                     |
| `in_square`                        | x moved by `nextInt(16)`, then z by `nextInt(16)`.                                                                                                                             |
| `height_range`                     | y from the height provider, with the generator's vertical anchors (26.4's `VerticalAnchor.Context.from(generator, level)` is the same: the greater bottom, the lesser height). |
| `biome`                            | The position if the biome at its block lists the placed feature in any step: the level's `getBiome`, the zoomed biome (`ChunkGenerator::block_biome`), not the stored quart.   |
| `offset` (26.3 on)                 | Moved by `x`, `y` and `z`, three int providers sampled in that order.                                                                                                          |
| `heightmap`, `environment_scan`, … | Read the level's blocks or heightmaps, which decoration has changed: a try after one isn't exact.                                                                              |

Where the biome at a try's block is tied and the candidates disagree on listing the feature, the try is kept and marked uncertain.

_Versions:_ 26.2's desert well has `heightmap` and `biome` only, its sand check in the feature; 26.3 moved the check into data (`offset`, `block_predicate_filter`). 26.2's `random_offset` isn't supported yet: no target uses it.

## Testing

Unit tests: the sorter on hand-built lists (merging, steps, a feature in two steps, a cycle), list loading, and `WorldgenRandom` against plain Xoroshiro. Ignored tests on the real bundle (`cargo xtask oracle-test`): every dimension's lists load and sort, and `spyglass-feature/tests/tries.rs` compares each step's order and every target's tries with the feature golden files, position by position, until a try that isn't exact. A dungeon's declined calls are replayed by drawing its two sizes, so its tries are compared up to the first that placed.
