# Value providers

Status: draft. Covers 26.3's `IntProvider`, `FloatProvider` and `HeightProvider` (`net.minecraft.util.valueproviders` and `net.minecraft.world.level.levelgen.heightproviders`): how their codecs read them, and how they sample. Implemented in `spyglass_data::providers`, for carvers ([carvers.md](carvers.md)) and structures ([structure-placement.md](structure-placement.md)). Carvers are checked against the game through them; the forms vanilla carvers don't use are checked by unit tests only.

Ported from the 26.3 bytecode. `randomBetweenInclusive(min, max)` is `nextInt(max − min + 1) + min`. `Mth.nextInt(min, max)` is `min` if `min ≥ max`, else `randomBetweenInclusive`. Integer arithmetic wraps.

## Int providers

A bare integer is `constant`. Otherwise `type` picks one of these. Each but `constant` and `weighted_list` must have `max ≥ min`.

| Type                    | Fields                                                         | Sample                                                                                                                        |
| ----------------------- | -------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------- |
| `constant`              | `value`                                                        | `value`.                                                                                                                      |
| `uniform`               | `min_inclusive`, `max_inclusive`                               | `randomBetweenInclusive(min, max)`.                                                                                           |
| `biased_to_bottom`      | `min_inclusive`, `max_inclusive`                               | `min + nextInt(nextInt(max − min + 1) + 1)`.                                                                                  |
| `very_biased_to_bottom` | `min_inclusive`, `max_inclusive`                               | `min + nextInt(nextInt(nextInt(max − min + 1) + 1) + 1)`.                                                                     |
| `clamped`               | `source`, `min_inclusive`, `max_inclusive`                     | `source`'s sample, clamped (`max(v, min)`, then `min(·, max)`).                                                               |
| `clamped_normal`        | `mean`, `deviation` (floats), `min_inclusive`, `max_inclusive` | `(int) clamp(mean + (float) nextGaussian() · deviation, min, max)`, clamped in `f32`.                                         |
| `trapezoid`             | `min`, `max`, `plateau` (at most `max − min`)                  | If `plateau` is 0 and `max = −min`: `nextInt(max + 1) − nextInt(max + 1)`. Else as the [height trapezoid](#height-providers). |
| `weighted_list`         | `distribution`                                                 | A [weighted](#weighted-lists) entry's sample.                                                                                 |

A provider's minimum and maximum are its bounds. `clamped`'s are `max(min, source's min)` and `min(max, source's max)`; `weighted_list`'s are its entries' extremes. Fields that must not be negative (a cave's `count`) check the minimum.

## Float providers

A bare number is `constant`. `FloatProviders.codec(min, max)` also requires the provider's range to be within `[min, max]`; NaN passes.

| Type             | Fields                                      | Sample                                                                                                                    |
| ---------------- | ------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| `constant`       | `value`                                     | `value`.                                                                                                                  |
| `uniform`        | `min_inclusive`, `max_exclusive` (greater)  | `nextFloat() · (max − min) + min`. Its maximum is `max_exclusive`.                                                        |
| `clamped_normal` | `mean`, `deviation`, `min`, `max`           | `clamp(mean + (float) nextGaussian() · deviation, min, max)`, `Mth.clamp`: `min` below it, else `Math.min`.               |
| `trapezoid`      | `min`, `max`, `plateau` (at most the range) | With `side = (max − min − plateau) / 2` and `rest = max − min − side`: `(min + nextFloat() · rest) + nextFloat() · side`. |

## Height providers

A bare [anchor](density-functions.md#loading) is `constant`. Anchors resolve against the generation context: the larger of the noise settings' and the level's minimum y, and the smaller height. None of them checks its range when loaded; an empty range gives the minimum (the game logs a warning).

| Type                    | Fields                                                            | Sample                                                                                                                                                                                                                             |
| ----------------------- | ----------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `constant`              | `value`                                                           | The anchor.                                                                                                                                                                                                                        |
| `uniform`               | `min_inclusive`, `max_inclusive`                                  | `min` if `min > max`, else `randomBetweenInclusive(min, max)`.                                                                                                                                                                     |
| `biased_to_bottom`      | `min_inclusive`, `max_inclusive`, `inner` (at least 1, default 1) | `min` if `max − min − inner + 1 ≤ 0`; else `nextInt(nextInt(max − min − inner + 1) + inner) + min`.                                                                                                                                |
| `very_biased_to_bottom` | the same                                                          | `min` if the span is empty; else `a = nextInt(min + inner, max)`, `b = nextInt(min, a − 1)`, then `nextInt(min, b − 1 + inner)`, each `Mth.nextInt`.                                                                               |
| `trapezoid`             | `min_inclusive`, `max_inclusive`, `plateau` (default 0)           | `min` if `min > max`; `randomBetweenInclusive(min, max)` if `plateau ≥ max − min`; else, with `side = (range − plateau) / 2` and `rest = range − side`, `min + randomBetweenInclusive(0, rest) + randomBetweenInclusive(0, side)`. |
| `weighted_list`         | `distribution`                                                    | A [weighted](#weighted-lists) entry's sample.                                                                                                                                                                                      |

## Weighted lists

`distribution` is a list of `{data, weight}`, with weights at least 0 and at least one above 0. A sample draws `r = nextInt(totalWeight)` and takes the first entry whose running sum of weights passes `r`. The game's two selectors (a flat array under a total weight of 64, a linear search above) both give that entry.

## Testing

`spyglass-data`'s unit tests sample every type and check its range, and check the codecs' rejections. Carvers' golden chunks ([carvers.md](carvers.md#testing)) check `uniform` heights, `very_biased_to_bottom` counts, and `uniform`, `trapezoid` and constant floats against the game.
