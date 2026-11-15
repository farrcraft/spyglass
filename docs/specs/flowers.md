# Flowers

Status: draft. Covers the flower that bone meal grows on grass at a block: bone meal's walk and its outcomes (`GrassBlock.performBonemeal`), the features it picks from (`BiomeGenerationSettings.getBoneMealFeatures`), the block state providers vanilla's flower features use, and the odds of each answer. Implemented in `spyglass_feature::flowers`, with the noises from `spyglass-noise` ([noise.md](noise.md)) and the block's biome from `BiomeManager`'s zoom ([biome-lookup.md](biome-lookup.md)).

Ported from the 26.2, 26.3 and 26.4-snapshot-8 bytecode. They agree unless a paragraph says otherwise.

## Bone meal on grass

`GrassBlock.performBonemeal(level, random, pos)`, with `pos` the grass block, makes 128 tries from `start = pos.above()`. Try `i` (0 to 127):

1. Starts at `start` and takes `i / 16` steps (integer division). Each step offsets the position by `dx = nextIntBetweenInclusive(-1, 1)`, then `dy = nextIntBetweenInclusive(-1, 1) · nextInt(3) / 2` (int arithmetic, left to right), then `dz = nextIntBetweenInclusive(-1, 1)`, in that order of draws. After each step, if `stopBonemealSpread` holds at the new position (the block below it isn't grass, or the block is a full cube), the try ends with nothing.
2. Otherwise `placeBonemealEffect` runs at the position reached, `p`:
   - If the block at `p` is short grass, then with `nextFloat() < 0.1` it's bone-mealed in turn (into tall grass), if it can be.
   - If the block at `p` isn't air, or `p` is outside the build height, nothing more happens.
   - With `nextFloat() < 0.125`, a flower: the [bone meal features](#bone-meal-features) of the biome at `p`. If there are none, nothing; otherwise one is picked with `Util.getRandom` (`list.get(nextInt(list.size()))`) and placed at `p`.
   - Otherwise the `minecraft:grass_bonemeal` placed feature is placed at `p` (short grass).

So the flower bone meal grows at a block is decided at the flower's own position, one above the grass, and by its biome there. Where the walk lands depends on the player's random, so spyglass answers for a block, not for a click.

## Bone meal features

The biome is `ServerLevel.getBiome(p)`: `BiomeManager.getBiome`, the zoomed biome of a nearby quart ([biome-lookup.md](biome-lookup.md)), with the quart's y clamped to the level. Where that lookup is tied, each candidate biome has its own answer.

A biome's bone meal features (`getBoneMealFeatures`, memoized) are, in order:

- for each decoration step's holder set of placed features ([features.md](features.md#data)), in order, and each placed feature in it, in order:
  - every configured feature reachable from it (`PlacedFeature.getFeatures`: its feature, then the features nested in that feature's configuration, depth first, as `ConfiguredFeature.getFeatures` streams them);
  - kept if it is a registered feature in the tag `#minecraft:can_spawn_from_bone_meal` (`tags/worldgen/feature` from 26.3, `tags/worldgen/configured_feature` in 26.2). An inline feature has no id, so it's never in the tag.

The list isn't deduplicated: a feature reachable twice is picked twice as often. Vanilla's tag lists `flower_default`, `flower_flower_forest`, `flower_swamp`, `flower_plain`, `flower_meadow`, `flower_cherry`, `wildflower` and `flower_pale_garden`.

## Placing the flower

Each of these is a `simple_block` feature (`SimpleBlockFeature.place`):

1. The state is its `to_place` provider's `getOptionalState(level, random, p)`. A provider that gives no state places nothing.
2. If the state can't survive at `p` (`canSurvive`, for flowers: the block below is in the flower's soil, which grass is), nothing is placed.
3. A double plant (`DoublePlantBlock`, such as tall grass) needs the block above `p` to be air, or replaceable with the same fluid; otherwise nothing. A mossy carpet places its own way. Anything else is set at `p`.

spyglass's answer is the state from step 1: the farm's grass is the soil. A double plant's answer says it needs the block above to be free.

## Block state providers

`to_place` is a block state provider. In 26.3 and later it's an id in the `worldgen/block_state_provider` registry, a block state on its own (a single state), or an inline provider with a `type`. In 26.2 it's always inline, and the types end in `_provider` (`noise_threshold_provider`), with states as `{"Name", "Properties"}`. The noise parameters are each version's [noise parameters](noise.md) form: `{"firstOctave", "amplitudes"}` in 26.2, `{"base_octave", "base_amplitude", ...}` from 26.3.

| Type (26.3)       | 26.2                       | Fields                                                                                                                                                  |
| ----------------- | -------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------- |
| a state           | `simple_state_provider`    | `state` (26.2)                                                                                                                                          |
| `weighted`        | `weighted_state_provider`  | `entries`, a list of `{data, weight}`: a [weighted list](providers.md#weighted-lists)                                                                   |
| `noise`           | `noise_provider`           | `seed` (long), `noise`, `scale` (float, above 0), `states` (at least one)                                                                               |
| `noise_threshold` | `noise_threshold_provider` | `seed`, `noise`, `scale`, `threshold` (float, −1 to 1), `high_chance` (float, 0 to 1), `default_state`, `low_states`, `high_states` (each at least one) |
| `dual_noise`      | `dual_noise_provider`      | `seed`, `noise`, `scale`, `states`, `variety` (`[min, max]`, 1 to 64), `slow_noise`, `slow_scale` (float, above 0)                                      |

Other providers (`randomized_int`, `rule_based`, `rotated_block`, ...) aren't used by vanilla's flower features; a flower feature with one gives no answer, with an error saying which.

### The noise

A noise provider builds its noise once, from its own `seed`: `NormalNoise.create(WorldgenRandom(LegacyRandomSource(seed)), noise)`. `dual_noise` builds `slow_noise` the same way, from the same seed. The world seed isn't involved, so **a provider picks the same state at a block in every world**.

- **26.2:** `getNoiseValue(pos, scale)` is the noise's `getValue(x · scale, y · scale, z · scale)`, a `double`, with `x`, `y`, `z` the block's coordinates as doubles and `scale` the float field widened to double. `getSlowNoiseValue(pos)` is `getValue((double)((float) x · slow_scale), ...)`: each coordinate is multiplied by `slow_scale` in `float`, then widened.
- **From 26.3:** the same products, but the noise's `get(x, y, z)` returns a `float` ([noise.md](noise.md)), and so do both functions. The slow noise's coordinates are still multiplied in `float`.

### Picking a state

- **A single state:** that state.
- **`weighted`:** the weighted list's sample (`getRandomOrThrow`): `r = nextInt(totalWeight)`, and the first entry whose running sum of weights passes `r`.
- **`noise`:** `getRandomState(states, v)` with `v = getNoiseValue(pos, scale)`:
  - 26.2: `t = clamp((1.0 + v) / 2.0, 0.0, 0.9999)` in `double`; the state at index `(int)(t · size)`.
  - From 26.3: the same in `float`: `t = clamp((1f + v) / 2f, 0f, 0.9999f)`, index `(int)(t · (float) size)`. 26.3's `Mth.clamp` and 26.4's `Math.clamp` agree for every noise value.
- **`noise_threshold`:** with `v = getNoiseValue(pos, scale)`, compared as a double with `threshold` widened:
  - if `v < threshold`: `Util.getRandom(low_states, random)`;
  - else if `nextFloat() < high_chance`: `Util.getRandom(high_states, random)`;
  - else `default_state`.
- **`dual_noise`:**
  1. `s = getSlowNoiseValue(pos)`, widened to double.
  2. `n = (int) Mth.clampedMap(s, -1.0, 1.0, variety.min, variety.max + 1)`: `clampedLerp(inverseLerp(s, -1, 1), min, max + 1)`, in double.
  3. For `j` in `0..n`: the state `getRandomState(states, getSlowNoiseValue(pos.offset(j · 54545, 0, j · 34234)))`, picked as `noise` picks from the slow noise's value (int multiplication, wrapping). These may repeat.
  4. The answer is `getRandomState(those n states, pos, scale)`: picked from them by the main noise at `pos`, as `noise` does.

Only `weighted` and `noise_threshold` draw from the random, and only in the order above.

## Odds

The answer at a block is the flowers it can grow, each with its chance given that the effect makes a flower (the `0.125` draw passed). With the biome's bone meal features `f₁ … fₙ`, each is picked with chance `1/n`, and a flower's chance is the sum over features of `1/n` times the feature's chance of giving it:

- a single state, `noise` and `dual_noise`: chance 1 for the one state they pick at the block;
- `weighted`: each entry's `weight / totalWeight`, summed over entries with the same state;
- `noise_threshold`: below the threshold, `1 / |low_states|` for each low state (summed over repeats); otherwise `high_chance / |high_states|` for each high state, and `1 − high_chance` for the default state. `high_chance` is a float: the chance is the share of `nextFloat()`'s 2²⁴ values below it, `ceil(high_chance · 2²⁴) / 2²⁴`.

Chances are kept as exact fractions. A flower is **certain** at a block when its chance there is 1. Where the biome has no bone meal features, the answer is no flower.

States are answers as block states: `pink_petals` and `wildflowers` with each `facing` and `flower_amount`, and the meadow's `tall_grass` and `short_grass`, are states like any flower's. Front ends name a flower by its block id and may group a block's states.

## Testing

`spyglass-feature`'s unit tests check each provider's odds against the share of states its `getState` draws over many random seeds, and that every answer's chances sum to 1. The oracle's [flower files](oracle.md#flower-files) check, for every supported version, each bone meal feature's provider draw for draw at fixed positions across the build height with fixed random seeds, and the biome and bone meal features at fixed blocks of the golden seeds.
