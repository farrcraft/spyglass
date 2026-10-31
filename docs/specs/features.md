# Features

Status: draft. Covers decoration (`ChunkGenerator.applyBiomeDecoration`): the decoration steps, the feature order every biome's lists sort into, each feature's seed, the placement modifiers that turn a chunk's corner into tries, and placing the targets (dungeons, geodes, desert wells, fossils, lava lakes) on a chunk's blocks, each answer certain or a candidate. Implemented in `spyglass-feature` ([ADR 0052](../adr/0052-features-in-their-own-crate.md)), with the random in `spyglass-rng` ([rng.md](rng.md#worldgen-seeding-worldgenrandom)) and block behaviour from the extractor's [block report](data-sources.md#block-report) ([ADR 0054](../adr/0054-block-behaviour-from-the-running-game.md)). Checked against the oracle's [feature golden files](oracle.md#feature-golden-files) in every supported version: the order in all three dimensions, every try's position, and whether each target placed.

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
| `heightmap`, `environment_scan`, … | Read the level's blocks or heightmaps, which decoration has changed ([below](#modifiers-that-read-blocks)): without the blocks, a try after one isn't exact.                   |

Where the biome at a try's block is tied and the candidates disagree on listing the feature, the try is kept and marked uncertain.

_Versions:_ 26.2's desert well has `heightmap` and `biome` only, its sand check in the feature; 26.3 moved the check into data (`offset`, `block_predicate_filter`). 26.2's `random_offset` isn't supported yet: no target uses it.

## Placing

Placing runs a chunk's tries on its blocks, in decoration order: each target feature checks the blocks, and if it places, writes its own, which every later modifier and feature reads.

### The region

`WorldGenRegion` during decoration, spyglass's `Region`: the chunk and its 8 neighbours, each after carvers (`ChunkGenerator::carved_chunk`), none decorated ([ADR 0053](../adr/0053-the-oracle-decorates-chunks-on-their-own.md)'s model).

- **Reads** outside the level give air (void air). Vanilla features never read outside the 3 × 3.
- **Writes** outside the 3 × 3 are dropped (`ensureCanWrite`), and outside the level do nothing. The flags (2 or 3) change nothing on a proto chunk.
- **Heightmaps.** `getHeight(type, x, z)` is the first free y, one above the top block the type counts, from the heightmap of the chunk holding the column; `min y` for none. Writes keep the four final heightmaps (`WORLD_SURFACE`, `OCEAN_FLOOR`, `MOTION_BLOCKING`, `MOTION_BLOCKING_NO_LEAVES`) up to date (`Heightmap.update`), and leave `WORLD_SURFACE_WG` and `OCEAN_FLOOR_WG` as terrain left them, so those are computed once from the carved chunks.
- **What the heightmaps count:** `WORLD_SURFACE*`, blocks that aren't air; `OCEAN_FLOOR*`, blocks in `#minecraft:blocks_motion_in_heightmap`; `MOTION_BLOCKING`, those or any block holding a fluid; `MOTION_BLOCKING_NO_LEAVES`, blocks in `#minecraft:blocks_motion_in_heightmap_no_leaves` or holding a fluid.
- **Blocks** are the generator's palette, extended with the states features place. Each state's behaviour comes from the block report: `isAir`, `liquid`, `isSolid` (the legacy solid flag), `isSolidRender`, its fluid and its block tags.

_Versions:_ (switch `heightmap_blocks_motion`) 26.2's heightmaps test `blocksMotion()` for `OCEAN_FLOOR*` and `MOTION_BLOCKING`, and `MOTION_BLOCKING_NO_LEAVES` is `(blocksMotion() or a fluid) and not a LeavesBlock`; there are no tags. 26.2 primes the final heightmaps at the start of features and 26.3 at the end of terrain; 26.4-snapshot-6 then copies `WORLD_SURFACE` and `OCEAN_FLOOR` over the `_WG` ones. On a carved chunk each is the same as computing it from the blocks.

### Block predicates

`BlockPredicate.test(level, pos)`. Those with an `offset` (−16 to 16 on each axis, default 0) test the block there.

| Type                  | Test                                                                                                |
| --------------------- | --------------------------------------------------------------------------------------------------- |
| `true`                | True.                                                                                               |
| `matching_blocks`     | The block is one of `blocks` (an id, a list or a `#tag`).                                           |
| `matching_block_tag`  | The block is in `tag`.                                                                              |
| `not`                 | Not `predicate`.                                                                                    |
| `all_of`, `any_of`    | `predicates` in order, stopping at the first false (all of) or true (any of); empty, true or false. |
| `inside_world_bounds` | The offset position's y is from `min y` to `max y`.                                                 |
| `volume_match`        | (26.3 on) Every position from `min` to `max` (inclusive; x outer, then z, then y) passes `match`.   |
| `below_heightmap`     | (26.4-snapshot-6) y is below the heightmap's first free y.                                          |

### Modifiers that read blocks

None draws from the random.

| Type                                | What it does                                                                                                                                                                                                                                                             |
| ----------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `heightmap`                         | The position moved to the heightmap's first free y, if that's above `min y`.                                                                                                                                                                                             |
| `environment_scan`                  | Nothing unless `allowed_search_condition` (default `true`) holds at the position. Then up to `max_steps` times: the position, if `target_condition` holds; move one up or down; nothing outside the build height; stop if not allowed. Then one more test of the target. |
| `surface_relative_threshold_filter` | The position if `h + min_inclusive ≤ y ≤ h + max_inclusive`, in `long`, with `h` the heightmap's first free y; the bounds default to `i32`'s.                                                                                                                            |
| `block_predicate_filter`            | The position if `predicate` holds.                                                                                                                                                                                                                                       |

### Lake

`LakeFeature` with lava (`lake_lava`), the same in every version. Nothing, and no draws, if the origin's y is at most `min y + 4`. Otherwise the lake is a 16 × 16 × 8 box from `origin − (8, 4, 8)`:

1. `nextInt(4) + 4` blobs of six `nextDouble`s each: radii `xr = r·6 + 3`, `yr = r·4 + 2`, `zr = r·6 + 3`, then centres `r·(16 − xr − 2) + 1 + xr/2`, `r·(8 − yr − 4) + 2 + yr/2`, and z as x, in `double`, left to right. Cells with x and z from 1 to 14 and y from 1 to 6 are in the lake where `((x − cx)/(xr/2))² + ((y − cy)/(yr/2))² + ((z − cz)/(zr/2))² < 1`.
2. An **edge** is a cell outside the lake with a face neighbour in it. Before anything is written, every edge is checked, x, z, y in order: liquid at y ≥ 4 declines; at y < 4, a block that's neither solid nor the lake's fluid state declines; and `can_place_feature` must hold.
3. Lake cells that `can_replace_with_air_or_fluid` allows become cave air at y ≥ 4 and the fluid below.
4. If the barrier isn't air, each edge at y < 4, and each at y ≥ 4 for which `nextInt(2)` isn't 0 (drawn before the block is read), becomes the barrier if it's solid and `can_replace_with_barrier` holds.

Water lakes also freeze their top. No supported version has one, and spyglass refuses them.

### Geode

`GeodeFeature` (`amethyst_geode`), the same algorithm in every version. The codec's defaults fill what the data leaves out: layers 1.7, 2.2, 3.2 and 4.2; crack chance 1.0, size 2.0, offset 2; placement chance 0.35; `outer_wall_distance` uniform 4 to 5, `distribution_points` 3 to 4, `point_offset` 1 to 2; offsets ±16; noise multiplier 0.05.

1. `n = distribution_points`, then `d9 = n / outer_wall_distance.max`; the thresholds are `1/√filling` and `1/√(layer + d9)` for the inner, middle and outer layers; the crack size is `1/√(base + nextDouble()/2 + (n > 3 ? d9 : 0))`; and there's a crack if `nextFloat() < chance`, in `double`.
2. `n` points, each `origin + (outer_wall, outer_wall, outer_wall)`. A point whose block is air or in `invalid_blocks` counts, and passing `invalid_blocks_threshold` declines, before anything is written. Each kept point then draws its `point_offset`.
3. With a crack, `nextInt(4)` picks a corner at `2n + 1` (x, z, both or neither), and its points at y 7, 5 and 1.
4. Every position from `origin − 16` to `origin + 16` (x fastest, then y, then z): `s` sums `1/√(distSqr(point) + offset) + noise` over the points, the noise sampled at the block and times 0.05. Below the outer threshold, nothing; at or above the filling's, the filling (air); else on the crack (its own sum at or above the crack size), air; else the inner layer, budding (`nextFloat() < use_alternate_layer0_chance`) or not, a budding block being a bud position if `nextFloat() < 0.35`; else the middle or outer layer. Every write keeps blocks in `cannot_replace`.
5. Each bud position draws one of the inner placements (`nextInt(4)`) and places it, facing out, on the first neighbour (down, up, north, south, west, east) that's air or a full water block, waterlogged if that neighbour held a source.

The noise has its own random, `LegacyRandomSource(world seed)`. From 26.3 it's `NormalNoise.createParity(-4, [1.0])`, whose base amplitude gives it the parity normalization's deviation, sampled in `float`.

_Versions:_ (switch `geode_noise_v1_21`) 26.2's noise is the 1.21 family's `NormalNoise.create(random, -4, [1.0])`, sampled in `double`. The two agree to about 1e-7, so a block at a layer's edge can differ.

### Monster room

`MonsterRoomFeature`, the same in every version.

1. Half sizes `xr` and `zr`, each `nextInt(2) + 2`. The box is x from `−xr − 1` to `xr + 1`, y from −1 to 4, and z like x. A scan of the whole box (x, y, z) declines on a floor or ceiling cell that isn't solid, and counts **openings**: wall cells at y 0 that are air with air above. Fewer than 1 or more than 5 declines. A declining room has drawn two numbers.
2. Carving, x ascending, y from 3 down to −1, z ascending: a boundary cell (floor or wall) becomes cave air if the block below isn't solid, whatever it was; else cobblestone if it's solid and not a chest, mossy where it's the floor and `nextInt(4)` isn't 0. Inside, every block but chests and spawners becomes cave air. The ceiling isn't written. Every write but the boundary's air keeps `#features_cannot_replace`.
3. Two chests, each with up to three tries: `x = ox + nextInt(2xr + 1) − xr`, z likewise, at the origin's y. A try needs air with exactly one solid horizontal neighbour. It places a chest facing away from its one solid-render side (`StructurePiece.reorient`), and draws `nextLong()` for its loot seed.
4. The spawner goes at the origin, with `nextInt(4)` for its mob if it's there (a chest drawn at the origin keeps it out).

### Templates

Fossils and, from 26.3, the desert well place structure templates (`StructureTemplate.placeInWorld`) with the feature's random: `nextInt(palettes)` picks the palette, drawn even for one. Each of the palette's blocks, sorted by y, then x, then z, those with block-entity NBT last, is moved by the rotation (mirror none, pivot zero) and passes the processors before any is written:

- `block_rot`: kept if `nextFloat() <= integrity`, in `float`.
- `protected_blocks`: dropped if the level's block there is in the tag.
- `rule`: the first rule whose input block matches replaces the state (location and position always true). Its block entity modifiers draw from a random of their own.

Rotation turns `axis` (x and z swap) and horizontal `facing`. Templates with other rotating properties are refused. The game sorts three lists apart (full blocks, others, those with NBT); spyglass sorts all but the NBT blocks together, which differs only when a drawing processor meets a template that mixes full and other blocks, which none of the targets' do.

After placing, the game updates the shapes of the blocks beside the template (`updateShapeAtEdge`), and waterlogs blocks that can hold water from water beside them. Neither is ported. A placed block beside one whose `updateShape` can change it puts the try in doubt. Those are the block report's `shape_updater` blocks, except falling blocks, fluids, slabs and brushable blocks, whose updates only schedule a tick. So does a placed block that can hold water where there was a fluid, or beside a source the template didn't place.

### Fossil

`FossilFeature`, the same in every version: `nextInt(4)` for the rotation, then `nextInt(templates)` for the template (the overlay takes the same index). The corner is the origin less half the rotated size in x and z (truncating), and `y = max(m − 15 − nextInt(10), min y + 10)`, where `m` is the lowest `OCEAN_FLOOR_WG` first free y over the footprint, at most the origin's y. More than `max_empty_corners_allowed` of the box's 8 corners being air, water or lava (those blocks, not waterlogged ones) declines. Then the fossil, with `fossil_processors`, and the overlay, with `overlay_processors`, at the same zero position (`getZeroPositionWithTransform`).

### Desert well

26.2's `desert_well` goes from one above the heightmap's position down through air while above `min y + 2`. It needs sand there, and declines where any column of the 5 × 5 has air both one and two below. Then it writes 5 × 3 × 5 of sandstone, water on the plus shape, sand under the water, the rim with slabs at the middle of each side, four pillars and a roof. Two `nextInt(5)` then pick suspicious sand from the centre, east, south, west and north, one and two below.

From 26.3 the well is data. `offset` and `block_predicate_filter` check the sand and the 5 × 2 × 5 below it, and the feature is an `overlay` of three `template` features run in order on the same origin and random. A template feature draws `nextInt(total weight)` for its template and `nextInt(4)` for its rotation, and centres the template on its origin. The suspicious sand's children pick their offset with `randomly_selected` (`nextInt(5)`).

### Certainty

Placing runs every placed feature of the steps it's asked for, not only the targets: the features of the same steps that spyglass places too (the forest rock and large dripstone, [below](#block-blob)) are placed exactly. A placing answer is certain unless something spyglass can't know could change it, which makes it a candidate with a reason:

- **A tie:** a block that depends on a tied biome ([material.md](material.md#ties)), or a tied biome at the try.
- **Another feature:** one spyglass doesn't place reached its feature earlier in the chunk (or its position wasn't known), and could have written anything in the region.
- **A shape update or waterlogging** that a template's placing would cause ([above](#templates)).
- **Outside the region:** a read past the 3 × 3, which vanilla features don't make.

Once one answer in a chunk is a candidate, every later one is too: a candidate's blocks may not be the game's.

### Block blob

`BlockBlobFeature` (`forest_rock`), the same in every version. From the origin, down while above `min y + 3` and the block below fails `can_place_on`; at `min y + 3` or below, it declines. Then three times: `a`, `b`, `c` each `nextInt(2)`, `f = (a + b + c)·0.333 + 0.5` in `float`, and `state` on every position within `(a, b, c)` of the centre whose squared distance is at most `(double)(f·f)`; then the centre moves by `(nextInt(2) − 1, −nextInt(2), nextInt(2) − 1)`.

### Large dripstone

`LargeDripstoneFeature` (`large_dripstone`; 26.4's `large_speleothem`, whose `base_block` 26.2 and 26.3 hard-code as dripstone block), the same algorithm in every version. Nothing unless the origin is air or water.

1. **The cave:** up and down from the origin through air and water (`Column.scan`, at most `floor_to_ceiling_search_range` blocks, default 30), each end needing the base block, a block in `replaceable_blocks` or lava. A cave fewer than 4 high declines.
2. **The radius:** `randomBetweenInclusive(min, clamp((int)(height · ratio), min, max))` of `column_radius`'s bounds. A stalactite below the ceiling and a stalagmite above the floor each draw a bluntness, then a height scale.
3. **Wind**, if both have at least `min_radius_for_wind` and `min_bluntness_for_wind`: a speed from `wind_speed`, an angle `nextFloat()·π`, and an offset that grows with the distance from the origin's y, at most `16 − radius`. 26.4's sine and cosine are rounded (switch `rounded_trig`).
4. **Each root** moves back into the ceiling or floor, up to `min(10, height)` blocks, until a circle of its radius is mostly in stone; lava on the way, or halving the radius to 1, gives up on that one.
5. **Each column** within the radius: the height at its distance (`getSpeleothemHeight`, in `double`), cut to `0.8` to `1` of it when `nextFloat() < 0.2`, then the base block into air, water or lava along it (offset by the wind), stopping at base stone once it has placed one, and for a stalagmite at the `WORLD_SURFACE_WG` heightmap.

## Testing

Unit tests: the sorter on hand-built lists (merging, steps, a feature in two steps, a cycle), list loading, and `WorldgenRandom` against plain Xoroshiro. Ignored tests on the real bundle (`cargo xtask oracle-test`):

- every dimension's lists load and sort;
- `spyglass-feature/tests/tries.rs` compares each step's order and every target's tries with the feature golden files, position by position, until a try that isn't exact (a dungeon's declined calls are replayed by drawing its two sizes);
- `spyglass-feature/tests/placing.rs` places every golden chunk on its carved 3 × 3 and compares each call, its position and whether it placed, with the game's, stopping at a chunk's first candidate.
