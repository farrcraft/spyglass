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

| Type                               | What it does                                                                                                                                                                                                                                                          |
| ---------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `count`                            | The position `count` times: an int provider, 0 to 256, sampled once.                                                                                                                                                                                                  |
| `rarity_filter`                    | The position if `nextFloat() < 1f / chance` (`float` division); NaN fails.                                                                                                                                                                                            |
| `in_square`                        | x moved by `nextInt(16)`, then z by `nextInt(16)`.                                                                                                                                                                                                                    |
| `height_range`                     | y from the height provider, with the generator's vertical anchors (26.4's `VerticalAnchor.Context.from(generator, level)` is the same: the greater bottom, the lesser height).                                                                                        |
| `biome`                            | The position if the biome at its block lists the placed feature in any step: the level's `getBiome`, the zoomed biome (`ChunkGenerator::block_biome`), not the stored quart.                                                                                          |
| `offset` (26.3 on)                 | Moved by `x`, `y` and `z`, three int providers sampled in that order.                                                                                                                                                                                                 |
| `noise_based_count`                | The position `ceil((n(x / noise_factor, z / noise_factor) + noise_offset) · noise_to_count_ratio)` times, `n` the biome info noise (`Biome.BIOME_INFO_NOISE`: simplex from legacy random 2345, `float` from 26.3, 26.2's `PerlinSimplexNoise` in `double`). No draws. |
| `noise_threshold_count`            | The position `below_noise` times where `n(x / 200, z / 200) < noise_level`, else (NaN too) `above_noise` times. No draws.                                                                                                                                             |
| `heightmap`, `environment_scan`, … | Read the level's blocks or heightmaps, which decoration has changed ([below](#modifiers-that-read-blocks)): without the blocks, a try after one isn't exact.                                                                                                          |

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
| `surface_water_depth_filter`        | The position if `WORLD_SURFACE`'s first free y less `OCEAN_FLOOR`'s is at most `max_water_depth`.                                                                                                                                                                        |

### Lake

`LakeFeature` with lava (`lake_lava`), the same in every version. Nothing, and no draws, if the origin's y is at most `min y + 4`. Otherwise the lake is a 16 × 16 × 8 box from `origin − (8, 4, 8)`:

1. `nextInt(4) + 4` blobs of six `nextDouble`s each: sizes (diameters) `xr = r·6 + 3`, `yr = r·4 + 2`, `zr = r·6 + 3`, then centres `r·(16 − xr − 2) + 1 + xr/2`, `r·(8 − yr − 4) + 2 + yr/2`, and z as x, in `double`, left to right. Cells with x and z from 1 to 14 and y from 1 to 6 are in the lake where `((x − cx)/(xr/2))² + ((y − cy)/(yr/2))² + ((z − cz)/(zr/2))² < 1`.
2. An **edge** is a cell outside the lake with a face neighbour in it. Before anything is written, every edge is checked, x, z, y in order: liquid at y ≥ 4 declines; at y < 4, a block that's neither solid nor the lake's fluid state declines; and `can_place_feature` must hold.
3. Lake cells that `can_replace_with_air_or_fluid` allows become cave air at y ≥ 4 and the fluid below.
4. If the barrier isn't air, each edge at y < 4, and each at y ≥ 4 for which `nextInt(2)` isn't 0 (drawn before the block is read), becomes the barrier if it's solid and `can_replace_with_barrier` holds.

Water lakes also freeze their top. No supported version has one, and spyglass refuses them.

### Geode

`GeodeFeature` (`amethyst_geode`), the same algorithm in every version. The codec's defaults fill what the data leaves out: layers 1.7, 2.2, 3.2 and 4.2; crack chance 1.0, size 2.0, offset 2; placement chance 0.35; `outer_wall_distance` uniform 4 to 5, `distribution_points` 3 to 4, `point_offset` 1 to 2; offsets ±16; noise multiplier 0.05.

1. `n = distribution_points`, then the spread `n / outer_wall_distance.max` (the game's `d9`); the thresholds are `1/√filling` and `1/√(layer + spread)` for the inner, middle and outer layers; the crack size is `1/√(base + nextDouble()/2 + (n > 3 ? spread : 0))`; and there's a crack if `nextFloat() < chance`, in `double`.
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
3. Two chests, each with up to three tries: `x = ox + nextInt(2xr + 1) − xr`, z likewise, at the origin's y. A try needs air with exactly one solid horizontal neighbour. It places a chest facing away from its one solid-render side (`StructurePiece.reorient`), and if a chest is there (`#features_cannot_replace` can keep it out) draws `nextLong()` for its loot seed, with table `chests/simple_dungeon` ([loot.md](loot.md#containers)).
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

Placing runs every placed feature of the steps it's asked for, not only the targets: the features of the same steps that spyglass places too (the forest rock and large dripstone, [below](#block-blob)) are placed exactly. Each answer has two certainties ([ADR 0056](../adr/0056-two-certainties-on-its-own-and-whatever-the-neighbours.md)):

- **On its own:** the answer for the chunk decorated before its neighbours, what the oracle's `tries` record. It's certain unless something in the chunk spyglass can't know could change it, which makes it a candidate with a reason:
  - **a tie:** a block that depends on a tied biome ([material.md](material.md#ties)), or a tied biome at the try;
  - **another feature:** one spyglass doesn't place, or isn't sure of, could have written what the try reads (below);
  - **a shape update or waterlogging** that a template's placing would cause ([above](#templates));
  - **outside the region:** a read past the 3 × 3, which vanilla features don't make.
- **Whatever the neighbours do:** also certain with any of the 8 neighbours decorated first, which their writes, bounded as [below](#neighbours), couldn't reach.

**What a try reads**, around its position: a lake its box (`−8..7`, `−4..3`, `−8..7`) and its column; a geode `±16`; a dungeon `−4..4`, `−2..4`, `−4..4`; a fossil its footprint's columns (`±7`); a well its columns (`±3`); a forest rock its columns (`±5`); a large dripstone its columns (`±32`). Dungeons and fossils read only whether a block is air, solid, a fluid, a chest or spawner, or in `#features_cannot_replace`, so writes that only change which solid block it is (ores, disks) can't change them; the others can be.

**Inside the chunk**, a placed feature spyglass doesn't place is unsure from its first try; one it places is unsure from its first candidate on. An unsure feature's later tries are candidates (their draws aren't known), and its writes, wherever its tries can be (as [below](#neighbours), for the chunk itself), put the later tries that read them in the same doubt.

### Neighbours

What a neighbour decorated first could write, for "whatever the neighbours do", from every placed feature of all 8 neighbours, in every step:

- **Where its tries can be** (`Decorator::try_boxes`): the modifiers run with each coordinate a range. The draws are the game's until a try reaches its feature, which draws what its blocks decide; from then on each modifier gives every value it can (`in_square` the whole chunk, a height provider its range, an offset its providers'). A modifier that reads blocks gives every position it could: a scan its steps; a `_WG` heightmap terrain's height (they never change); a live heightmap from 48 below terrain's to the top. A `biome` modifier rules out a single column only if no biome along it lists the feature. A placed feature runs only where a biome stored in the chunk or its 8 neighbours lists it (`ChunkGenerator::chunk_biomes`, with a quart's border for 26.4's upscaled biomes).
- **Where its feature can write** (`Reach`): a box around the try for each feature type, from its configuration, taken from each version's bytecode: ores `±ceil(3s/16)` across and `−ceil(s/16)−2` to `ceil(s/16)` up for size `s`; disks their radius and half height; trees `±10` across, `−15` to `32` up (higher for a taller trunk), and whole columns for pale oak's hanging moss; selectors the union of their children's, each moved by its modifiers' largest offset; and so on for every Overworld type. Features whose writes follow a heightmap down (ice spikes, icebergs, bamboo's podzol, 26.2's ocean plants, forest rocks, large dripstone, fossils, wells) write whole columns. A type not known writes anywhere in the 3 × 3. Ores and disks only change which solid block a block is.

Measured on 26.3's golden area, nothing holds whatever the neighbours do: a neighbour's many-try features (dungeons, lichen, pointed dripstone, cave vegetation) can be anywhere in its chunk after their first try, and reach across the border.

### Structures

Each decoration step places its structures' pieces in the chunk being decorated before its features (`StructureStart.placeInChunk`, clipped to the chunk), the structures in the registry's order. The targets' steps have `underground_structures` (mineshafts, buried treasure, trail ruins, trial chambers) and `surface_structures` (the surface structures, and the stronghold). The oracle's golden files have none, so these only make candidates:

- A try in a step is a candidate on its own where the pieces of a structure placing in that step or an earlier one meet what it reads inside its chunk (`Doubt::Structure`); for "whatever the neighbours do", anywhere in the region, since a neighbour decorated first places its pieces in its own chunk.
- The structures are those that could start within 8 chunks (the chunks that reference a start), by `Starts`: placement, then the biome and terrain checks, a start that only might be there counting.
- Their pieces: a jigsaw or mineshaft start's own boxes (`Starts::assemble`, [structure-placement.md](structure-placement.md#mineshafts)), a mineshaft's as whole columns, since its corridors drop supports to solid ground below and hang chains above; a single-piece structure's whole columns over its footprint from the start chunk's corner, either way turned (buried treasure `8..10`, desert pyramid `0..20`, jungle temple `0..14`, swamp hut `0..8`, igloo `−3..12`); any other everything within 8 chunks of its start.

### Block blob

`BlockBlobFeature` (`forest_rock`), the same in every version. From the origin, down while above `min y + 3` and the block below fails `can_place_on`; at `min y + 3` or below, it declines. Then three times: `a`, `b`, `c` each `nextInt(2)`, `f = (a + b + c)·0.333 + 0.5` in `float`, and `state` on every position within `(a, b, c)` of the centre whose squared distance is at most `(double)(f·f)`; then the centre moves by `(nextInt(2) − 1, −nextInt(2), nextInt(2) − 1)`.

### Large dripstone

`LargeDripstoneFeature` (`large_dripstone`; 26.4's `large_speleothem`, whose `base_block` 26.2 and 26.3 hard-code as dripstone block), the same algorithm in every version. Nothing unless the origin is air or water.

1. **The cave:** up and down from the origin through air and water (`Column.scan`, at most `floor_to_ceiling_search_range` blocks, default 30), each end needing the base block, a block in `replaceable_blocks` or lava. A cave fewer than 4 high declines.
2. **The radius:** `randomBetweenInclusive(min, clamp((int)(height · ratio), min, max))` of `column_radius`'s bounds. A stalactite below the ceiling and a stalagmite above the floor each draw a bluntness, then a height scale.
3. **Wind**, if both have at least `min_radius_for_wind` and `min_bluntness_for_wind`: a speed from `wind_speed`, an angle `nextFloat()·π`, and an offset that grows with the distance from the origin's y, at most `16 − radius`. 26.4's sine and cosine are rounded (switch `rounded_trig`).
4. **Each root** moves back into the ceiling or floor, up to `min(10, height)` blocks, until a circle of its radius is mostly in stone; lava on the way, or halving the radius to 1, gives up on that one.
5. **Each column** within the radius: the height at its distance (`getSpeleothemHeight`, in `double`), cut to `0.8` to `1` of it when `nextFloat() < 0.2`, then the base block into air, water or lava along it (offset by the wind), stopping at base stone once it has placed one, and for a stalagmite at the `WORLD_SURFACE_WG` heightmap.

### Ruling out

Without the unlikely candidates (the tries that don't place on spyglass's blocks), a chunk where no target could place on its own needn't be placed. Only spyglass's blocks decide whether a try places: structure pieces, and features spyglass doesn't place, only put it in doubt. So a chunk is ruled out from the terrain in outline (`Outline`), without its blocks, when every try of every target is shown to decline. Ruling out is never a guess: a try that can't be shown to decline keeps the chunk.

- **The outline:** the noise fill's final density at each position, sampled from the density's y alignment so the values are the chunk's ([terrain.md](terrain.md#the-column)); every position a carver could carve, every tied candidate's carvers included; and the chunks whose stored biomes (`chunk_biomes`, with tie candidates) are ones where the material rule can give air (in 26.3's and 26.4's rules, a frozen ocean's hole) or its extensions add blocks (eroded badlands' pillars, frozen oceans' icebergs). A block **could be air** outside the level, where the density isn't positive (air or a fluid), where a carver could carve, or in a chunk with the rule's air. It **surely isn't solid** outside the level, or where the density isn't positive in a chunk no extension reaches: the rule only replaces the noise fill's default block, and carvers only remove blocks. A rule that can give air in any biome, or 26.2's terrain, has no outline.
- **Earlier features:** what the features spyglass places could write before the try (`NeighbourBounds::outline_writes`: their try boxes, with any heightmap giving any height, grown by their write bounds) can be any block, and air unless the feature writes one state that isn't (forest rock, large dripstone). The targets' own writes are left out, since a chunk is ruled out only if none of their tries place.
- **A dungeon** draws its two sizes as the game does, so the next try is exact. It surely declines if a floor or ceiling block surely isn't solid and nothing earlier could write it, or if no wall block could be air under air.
- A try that isn't exact or certain, or whose feature isn't a dungeon, keeps the chunk.

Measured on 26.3's golden square (two seeds, 512 chunks), 246 chunks are ruled out, about 7 ms a chunk, and the answers are the same as placing every chunk. Most kept chunks have a dungeon try beside a cave. Each kept chunk still needs its 8 neighbours' carved blocks, so ruling out saves little: about 27 ms a chunk against 29.

## Testing

Unit tests: the sorter on hand-built lists (merging, steps, a feature in two steps, a cycle), list loading, and `WorldgenRandom` against plain Xoroshiro. Ignored tests on the real bundle (`cargo xtask oracle-test`):

- every dimension's lists load and sort;
- `spyglass-feature/tests/tries.rs` compares each step's order and every target's tries with the feature golden files, position by position, until a try that isn't exact (a dungeon's declined calls are replayed by drawing its two sizes);
- `spyglass-feature/tests/placing.rs` places every golden chunk on its carved 3 × 3 and compares each placed feature's calls, position and whether it placed, with the game's `tries`, up to its first candidate; where `after_neighbours` differs, spyglass must say the neighbours could change the call.
- `spyglass/tests/features.rs` finds the dungeons of two seeds' golden squares: every one the game placed is an answer, every certain answer is one it placed, and ruling chunks out gives the same certain and likely answers as placing every chunk.
