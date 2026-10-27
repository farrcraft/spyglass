# Density functions

Status: stable for every type's point and volume paths, implemented in `spyglass-density` for 26.3 and 26.4-snapshot-5 and checked against the game by the oracle's [probe files](oracle.md#probe-files). 26.2's differences are drafted in _Versions_ notes and the [26.2 section](#262-evaluation) from the bytecode, and not yet checked against the oracle.

26.3 reworked density functions since 1.21. They are compiled into samplers (`DensityFunctionCompiler`, package `net.minecraft.world.level.levelgen.densityfunction`) that return `float`, after rewrite rules (`DfRewriteRule.INLINE_REFERENCE`, `SLICE_UNIFORM_AXES`). Older ports, which evaluate in `double`, don't apply. Ported from the 26.3 bytecode.

_Versions:_ 26.2 is a different algorithm family: everything is `f64`, there is no volume arithmetic, and caching and interpolation exist only inside the chunk generator's `NoiseChunk`. It has its own type ids, fields and formulas, so none of 26.3's samplers can be reused for it bit for bit. It has [its own section](#262-evaluation). 26.4-snapshot-5 keeps 26.3's compiler, replaces `interpolated` with [`upscale`](#upscale), and changes splines and `clamp` in edge cases, noted where they apply.

## Loading

Density functions live in a bundle's `data/<namespace>/worldgen/density_function/`. They are parsed as the game's codecs read them ([ADR 0010](../adr/0010-typed-loading-of-worldgen-json.md)), with numbers read from their text ([ADR 0011](../adr/0011-parse-json-numbers-from-their-text.md)). A density-function value (`DensityFunction.CODEC`) is one of:

- **a number:** a constant, read as `float` (`Float.parseFloat` of the text), which must be in `[-1000000, 1000000]` (`NOISE_VALUE_CODEC`);
- **a string:** a reference to a named function. It's loaded once and shared, as after `INLINE_REFERENCE`. A cycle is an error;
- **an object with a `type`:** dispatched on the type id. An id without a namespace is in `minecraft`. Fields the codec doesn't know are ignored.

Loading fails, naming the resource and the JSON path, on an unsupported type, a missing field, a wrong shape or an out-of-range value.

_Versions:_ 26.2 (`levelgen.DensityFunctions`) reads a constant as `double` (`Codec.doubleRange(-1000000, 1000000)`: the text parsed as `f64`, never narrowed), and has its own type ids and field names ([below](#262-types)). 26.4-snapshot-5 has no `interpolated` id; its data uses [`upscale`](#upscale) instead.

## Types

Point evaluation at block `(x, y, z)` (ints), in `f32` unless stated. `X`, `Y`, `Z` are the coordinates as `f64`.

| Type                          | Fields (defaults)                                                                                                                                                                 | Value                                                                                                                                                                                                                                                                                                |
| ----------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `constant`                    | `value` (noise value)                                                                                                                                                             | the value                                                                                                                                                                                                                                                                                            |
| `noise`                       | `noise` (a noise id; inline noises aren't allowed, by the game either), `xz_scale`, `y_scale` (`f64`), `shift_x`, `shift_y`, `shift_z` (functions, default 0)                     | `noise.get(X·xz + sx, Y·y + sy, Z·xz + sz)`, with each shift's value widened to `f64`. A shift that is the constant `+0.0` is left out entirely (the game picks a sampler without it), and a non-zero `shift_y` is added. The noise is the seed's [normal noise](noise.md#normal-noise) with that id |
| `shift_a`                     | `noise`                                                                                                                                                                           | `noise.get(X·0.25, Y·0.0, Z·0.25) · 4`                                                                                                                                                                                                                                                               |
| `shift_b`                     | `noise`                                                                                                                                                                           | `noise.get(Z·0.25, X·0.25, 0.0) · 4`                                                                                                                                                                                                                                                                 |
| `shift`                       | `noise`                                                                                                                                                                           | `noise.get(X·0.25, Y·0.25, Z·0.25) · 4`                                                                                                                                                                                                                                                              |
| `add`, `mul`                  | `left`, `right`                                                                                                                                                                   | `left + right`, `left · right`. The game specializes constant operands, which computes the same float. A `mul` of two non-constant operands returns `+0.0` when `left` is `0` or `-0`, without evaluating `right` (point path only; see [below](#point-and-volume-evaluation))                       |
| `abs`                         | `input`                                                                                                                                                                           | `abs(input)`                                                                                                                                                                                                                                                                                         |
| `cache`                       | `input`                                                                                                                                                                           | `input` (caching is the game's optimization; the point value is unchanged)                                                                                                                                                                                                                           |
| `lerp`                        | `alpha`, `first`, `second`                                                                                                                                                        | `first` if `alpha == 0`, `second` if `alpha == 1`, else `first + alpha·(second − first)`. The endpoints are returned as-is, not computed                                                                                                                                                             |
| `gradient`                    | `axis` (`x`, `y`, `z`), `tiling` (default `clamp_to_edge`; the only one supported), `from_coordinate`, `to_coordinate` (ints, not equal), `from_value`, `to_value` (noise values) | `c` = the axis coordinate clamped to the coordinates' range; `from_value + (float)(c − from_coordinate) · factor`, where `factor = (to_value − from_value) / (float)(to_coordinate − from_coordinate)` is computed once                                                                              |
| `spline`                      | `spline` (below)                                                                                                                                                                  | the spline's value                                                                                                                                                                                                                                                                                   |
| `blend_alpha`, `blend_offset` | none                                                                                                                                                                              | 1 and 0: context-bound constants whose defaults apply when nothing is blending, which is always the case for new terrain                                                                                                                                                                             |

**Splines** (`CubicSpline`): a number is a constant (`float`). Otherwise `{"coordinate": function, "points": [{"location", "value", "derivative"}, ...]}`, where `location` and `derivative` are `float` and `value` is a spline. To sample, take `c` = the coordinate's value and `i` = the index of the first location greater than `c`, minus one (`c < location`, so a NaN `c` lands past the end):

- `i < 0` or `i` is the last point: the point's value `v`, extended linearly. That's `v` if its derivative `d` is 0, else `v + d·(c − location)`;
- otherwise, with `x0, x1` the locations, `f0, f1` the values and `d0, d1` the derivatives of points `i` and `i + 1`: `t = (c − x0)/(x1 − x0)`, `a = d0·(x1 − x0) − (f1 − f0)`, `b = −d1·(x1 − x0) + (f1 − f0)`, and the value is `lerp(t, f0, f1) + (t·(1 − t))·lerp(t, a, b)`, in exactly that order.

_Versions:_ (switch `spline_nan_is_nan`) 26.4-snapshot-5 (`CubicSpline.Multipoint.sample`) tests the coordinate first:

1. `c` is NaN: the value is NaN. (26.3 lands past the end, so a NaN `c` gives the last point's `v` when its derivative is 0.)
2. `c ≤ location_0`: the first point, extended linearly. `c ≥ location_last`: the last point, extended.
3. Otherwise `i` is found by a linear scan (`i = 1`; while `i < n − 1` and `c ≥ location_i`, `i++`; the interval is `i − 1, i`), the same interval as 26.3's search, with the same formula.

So 26.4 differs from 26.3 only for a NaN `c`, and at `c == location_0`, where 26.3 computes the interval formula at `t = 0`: the two agree unless the first value is `−0` or a neighbouring value or derivative isn't finite. (The scan and the binary search find the same interval while the locations increase, as in all vanilla data.) The switch is read when the spline is loaded, so each spline samples one way. 26.2 samples splines with code identical to 26.3's, each coordinate being `(float)` of its `f64` value and the result widened to `f64` ([below](#262-types)).

### Terrain types

The types terrain needs ([terrain.md](terrain.md)), and the rest of 26.3's 44 type ids. Point values, as above. Where the volume path differs, [below](#point-and-volume-evaluation) says how.

**Compiling.** `DensityFunctionCompiler` inlines references, turns each `cache` into a prepared cache, then applies `SLICE_UNIFORM_AXES`. That rule wraps nodes (other than constants and gradients) in `slice`s at coordinate 0 for the axes they don't depend on. It doesn't change values, so spyglass doesn't apply it. The samplers that specialize constant operands (`add` to `div`, `min`, `max`, `range_choice`, `pow`, the rounding types, `lerp`) test whether the operand **is** a `ConstantFunction` after inlining. A `cache` of a constant isn't one, so it compiles as a general operand.

| Type                                           | Fields (defaults)                                                                                                                        | Value                                                                                                                                                                                                                                                                                      |
| ---------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `sub`                                          | `left`, `right`                                                                                                                          | `left − right`. A constant right `c` compiles to `left + (−c)`, the same float                                                                                                                                                                                                             |
| `div`                                          | `left`, `right`                                                                                                                          | `+0.0` if `left` is ±0 (right not evaluated), else `left / right`. A constant left `c`: `c / right`, no zero test. A constant right `c`: `left · (1.0f / c)`, which isn't always the float `left / c`                                                                                      |
| `min`, `max`                                   | `left`, `right`                                                                                                                          | Chosen by the operands' [ranges](#ranges): an operand that's always on the winning side is the whole value. Else, with a constant operand, `Math.min`/`Math.max`. Else `min`: `left` if `left ≤ right.min`, else `Math.min(left, right)`; `max` mirrored                                   |
| `square`, `cube`                               | `input`                                                                                                                                  | `x·x`; `(x·x)·x`                                                                                                                                                                                                                                                                           |
| `half_negative`, `quarter_negative`            | `input`                                                                                                                                  | `x` if `x > 0`, else `x · 0.5f` (`0.25f`)                                                                                                                                                                                                                                                  |
| `negate`                                       | `input`                                                                                                                                  | `−x`                                                                                                                                                                                                                                                                                       |
| `squeeze`                                      | `input`                                                                                                                                  | `c = clamp(x, −1, 1)`; `c / 2f − cube(c) / 24f`                                                                                                                                                                                                                                            |
| `sqrt`, `log`, `sign`, `reciprocal`            | `input`                                                                                                                                  | `(float) Math.sqrt((double) x)`; `(float) Math.log((double) x)`; `Math.signum(x)`; `1.0f / x`. Unused in vanilla                                                                                                                                                                           |
| `floor`, `ceil`, `round`, `truncate`           | `input`, `multiple` (function, default 1)                                                                                                | `m = multiple`; `x` if `m == 0`, else `R(x / m) · m`, where `R` is `Math.floor`/`Math.ceil` in `f64`, `Math.round` to an int (halves up), or towards zero. Unused in vanilla                                                                                                               |
| `clamp`                                        | `input`, `min`, `max` (noise values, `max ≥ min`)                                                                                        | `Mth.clamp`: `min` if `x < min`, else `Math.min(x, max)`                                                                                                                                                                                                                                   |
| `pow`                                          | `base`, `exponent`                                                                                                                       | `(float) Math.pow(base, exponent)` in `f64`; a constant exponent of magnitude ½, 1, 2 or 3 compiles to sqrt, identity, square or cube, then `1.0f / ·` if negative. Unused in vanilla                                                                                                      |
| `range_choice`                                 | `input`, `min_inclusive`, `max_exclusive` (noise values), `when_in_range`, `when_out_of_range`                                           | `when_in_range` if `min ≤ input < max`, else `when_out_of_range` (NaN is out). Only the chosen branch is evaluated                                                                                                                                                                         |
| `interval_select`                              | `input`, `thresholds` (noise values, in `Float.compare` order, so `−0` before `0`), `functions` (one more than thresholds, at least two) | `functions[i]` for the first `i` with `input < thresholds[i]`, else the last. NaN gives the last                                                                                                                                                                                           |
| `slice`                                        | `axis`, `coordinate` (int), `input`                                                                                                      | `input` with that coordinate replaced. Doesn't depend on that axis                                                                                                                                                                                                                         |
| `interpolated`                                 | `input`, `cell_size_xz`, `cell_size_y` (positive ints)                                                                                   | [Below](#interpolated)                                                                                                                                                                                                                                                                     |
| `find_top_surface`                             | `density`, `upper_bound` (functions), `lower_bound` (int, −4064 to 4062: twice `DimensionType`'s y range), `cell_height` (positive int)  | [Below](#find_top_surface)                                                                                                                                                                                                                                                                 |
| `old_blended_noise`                            | `xz_scale`, `y_scale`, `xz_factor`, `y_factor` (`f64`, 0.001 to 1000), `smear_scale_multiplier` (`f64`, 1 to 8)                          | [Below](#old_blended_noise)                                                                                                                                                                                                                                                                |
| `beardifier`                                   | none                                                                                                                                     | 0, with no structures (terrain before chunks exist never has any)                                                                                                                                                                                                                          |
| `blend_density`                                | `input`                                                                                                                                  | `input`, with nothing blending                                                                                                                                                                                                                                                             |
| `gradient` tilings `repeat`, `mirrored_repeat` | as `gradient`                                                                                                                            | `from_value + (float) floorMod(c − from, r) · factor` with the int `r = to − from`, negative when `to < from` (`floorMod` and `floorDiv` follow its sign); mirrored: `q = floorDiv(c − from, r)`, `rem = (c − from) − q·r`, and `rem` or `r − rem` by the parity of `q`. Unused in vanilla |
| `distance_to_point`                            | `point` (three ints), `metric` (`euclidean`, `euclidean_squared`, `manhattan`, `chebyshev`)                                              | [Below](#distance_to_point)                                                                                                                                                                                                                                                                |
| `end_outer_islands`                            | none                                                                                                                                     | [Below](#end_outer_islands)                                                                                                                                                                                                                                                                |

_Versions:_ (switch `clamp_positive_zero`) 26.4-snapshot-5 removes `Mth.clamp`; `clamp` (and `squeeze`'s clamp) use `Math.clamp(x, min, max)` = `Math.min(max, Math.max(x, min))`. That differs from `Mth.clamp` only for `x = −0` with `min = +0`, which gives `+0` (`Mth.clamp` gives `−0`); NaN stays NaN. `squeeze`'s bounds are ±1, so it doesn't change. `ClampFunction.compileSampler` also chooses by the input's range `r` (in this order):

- `min ≤ r.min` and `max ≥ r.max`: the input's own sampler, unclamped;
- `max ≥ r.max`: `ConstMaxSampler(input, min)`, the constant `max` above (point `Math.max(x, min)`; volume: `min` replaces `x` only if `min > x`);
- `min ≤ r.min`: `ConstMinSampler(input, max)`, mirrored;
- else the clamp.

Values are the same except for signed zeros and NaN: the unclamped input keeps a `−0` that a clamp at `min = +0` would turn into `+0`, and the volume path of the constant `max` and `min` keeps a NaN, and a `−0` under `min = +0` (`+0` isn't above `−0`), where its point path (`Math.max`) gives `+0`. Ranges don't change. The switch covers both the clamp and the compile choice, and is read when the function is loaded. `old_blended_noise`'s clamp of `main + 0.5` to `[0, 1]` is `Math.clamp` in 26.4 whatever the switch, but `main + 0.5` is never `−0`, so it has the same values.

**`Math.pow` and `Math.log`** aren't required by Java to be correctly rounded, and spyglass uses Rust's `powf` and `ln`. So `pow`, `log` and `pow`'s range bounds may differ from the game's in the last bit of the `f64`, before it's narrowed to `f32`. No vanilla function uses them. `Math.sqrt` is correctly rounded in both.

#### `interpolated`

Cells are `cell_size_xz × cell_size_y × cell_size_xz` blocks, aligned to absolute coordinates with `floorDiv`. The input is sampled at cell corners, always through the volume path.

- **Point:** with `f` = `floorMod` of each coordinate by its cell size, if all three are 0 the value is the input's at the point. Otherwise the eight corners of the cell are sampled as a 2 × 2 × 2 volume, and the value is `Mth.lerp3(fx / (float) cxz, fy / (float) cy, fz / (float) cxz, …)`: x, then y, then z, with each `t` a division.
- **Volume** (what terrain uses; `sampleWithBlockStep`): the corners of every cell the volume touches are sampled as one volume. For each cell, z outer, then x, then y: per `dz`, `tz = (float) dz · (1.0f / cxz)` and the four z lerps; per `dx`, `tx` likewise and two x lerps, `b0` and `b1`; then `step = (b1 − b0) · (1.0f / cy)`, the first value is `b0 + step · (float) dy0`, and each next y adds `step` to the last value. So volume values differ from point values in the low bits, and depend on where in its cell the volume's lowest y is (not on its x and z origin). Columns and chunks both start at the noise settings' minimum y, so they agree.
- **Corners** (`sampleWithBlockStep`): with `first` and `last` the cells (`floorDiv`) of the volume's minimum and maximum block on each axis, the corner volume starts at `first · size`, one cell apart, with `last − first + 1` corners, plus one unless the maximum block is itself on a corner. Cell `i`'s far corner is `min(i + 1, corners − 1)`. In `fillCell`, a cell's blocks outside the volume are skipped: offsets run from `max(0, −origin)` to `min(size, volume size − origin) − 1`, with `origin` the cell's first block minus the volume's.
- A volume that is on cell corners (each axis one position deep or a cell apart, and each minimum a multiple of the cell size) is the input's own volume.
- A volume with any step above 1 (`sampleWithNonBlockStep`) is interpolated over every block it spans, then each position's value is picked.

_Versions:_ 26.4-snapshot-5 has no `interpolated`: its data uses [`upscale`](#upscale), which isn't bit-identical to it even with linear modes. 26.2's `interpolated` has no cell size fields and only interpolates inside a `NoiseChunk` ([below](#262-noisechunk)).

#### `upscale`

_Versions:_ 26.4-snapshot-5 only (package `densityfunction.op.upscale`, `UpscaleFunction`, `Upscaler`, `PolymorphicUpscaler`). Not in 26.2 or 26.3.

Fields: `input`, and `x`, `y`, `z`, each `{"mode": M, "cell_size": N}` (`UpscaleFunction.Axis.CODEC`; all fields required, `cell_size` an int from 1 to 64, `mode` one of the names below). Cells are aligned to absolute coordinates with `floorDiv`, as for `interpolated`. Each axis is upscaled on its own, with a mode:

| Mode                 | Samples before | Samples after | Exact on a corner |
| -------------------- | -------------- | ------------- | ----------------- |
| `nearest_neighbor`   | 0              | 0             | yes               |
| `interpolate_linear` | 0              | 1             | yes               |
| `interpolate_cosine` | 0              | 1             | yes               |
| `interpolate_cubic`  | 1              | 2             | yes               |
| `b_spline_cubic`     | 1              | 2             | no                |

A mode samples `before + after + 1` corners per axis. 26.4's vanilla data maps each 26.3 `interpolated(input, xz = a, y = b)` to all-`interpolate_linear` with `x = z = a` and `y = b` (cells 4/8/4, 8/4/8, and 16/1/16 in `chunk_surface_level`).

**Coefficients** (`Upscaler.Coefficients.computeTable(cell, mode)`): entry `i` for `i` in `0..cell` uses `t = (float) i / (float) cell`, a division, all `f32`. `cube(t) = (t·t)·t`, `square(t) = t·t` (`Mth`).

- nearest: the first sample, `s0`.
- linear: `c1 = 1 − t`, `c2 = t`; the value is `c1·s0 + c2·s1`.
- cosine: linear with `t` replaced by `Ease.inOutSine(t) = −(Mth.cos((double)(π_f·t)) − 1) / 2`, with `π_f` the `float` π. `Mth.cos(x)` is the game's sine table at `(long)(|x| · 10430.378350470453 + 16384.0 + 0.5) & 65535`, where entry `i` is `(float) Math.sin(i / 10430.378350470453)`, but exactly 0, 1, 0 and −1 at 0, 16384, 32768 and 49152. spyglass computes that entry with Rust's `sin`, which, as for `Math.pow` above, may differ from Java's in the last bit of the `f64` before it's narrowed. No vanilla function uses the cosine mode.
- cubic (Catmull-Rom): `c1 = (−0.5·cube(t) + square(t)) − 0.5·t`, `c2 = (1.5·cube(t) − 2.5·square(t)) + 1`, `c3 = (−1.5·cube(t) + 2·square(t)) + 0.5·t`, `c4 = 0.5·cube(t) − 0.5·square(t)`; the value is `((c1·s0 + c2·s1) + c3·s2) + c4·s3`.
- b-spline: `c1 = cube(1 − t) / 6`, `c2 = ((3·cube(t) − 6·square(t)) + 4) / 6`, `c3 = (((−3·cube(t) + 3·square(t)) + 3·t) + 1) / 6`, `c4 = cube(t) / 6`, combined as cubic.

`compileSampler` makes an `upscale` whose three modes are all nearest a `NearestNeighborSampler` (**All nearest**, below); any other mix is the general sampler, where a nearest axis samples one corner. The general sampler's upscaler (`PolymorphicUpscaler`, which `makeMonomorphicDelegate` specializes per parameter set: not traced in the bytecode, and the oracle agrees with the polymorphic arithmetic) holds each axis's coefficient table, `1.0f / (float) cell` for x and y, and the sample counts, with the samples laid out as a volume's: y stride 1, x stride `countY`, z stride `countX·countY`. `combine` below is `Coefficients.multiply`.

**Point** (`UpscaleFunction.Sampler.sampleValue`), with the upscaler for the function's own modes and cell sizes: with `f` = `floorMod` of each coordinate by its cell size, if all three are 0 and every mode is exact on a corner, the value is the input's at the point. Otherwise the input is sampled through its volume path over a `countX × countY × countZ` volume (each count `before + after + 1`), one cell apart, from `(x − fx − beforeX·cx, y − fy − beforeY·cy, z − fz − beforeZ·cz)`. Then, with the buffer y fastest, then x, then z:

```text
for i in 0..countX·countY:  tz[i] = coefZ[fz].combine(buf, i, stride countX·countY)   // z first
for j in 0..countY:         tx[j] = coefX[fx].combine(tz, j, stride countY)           // then x
value = coefY[fy].combine(tx, 0, stride 1)                                            // then y
```

**Volume** (`sampleVolume`): each axis is planned from the volume's minimum `min`, size and step (`planUpsample`):

```text
if size == 1: step = cell
max = min + (size − 1)·step
if step % cell == 0 and min % cell == 0 and mode exact on a corner:
    exactly aligned: the axis takes part as nearest, cell 1, at the volume's own positions
if cell % step != 0: no plan
first = floorDiv(min, cell); last = floorDiv(max, cell)
s0 = first − before
s1 = last + max(after − (1 if exact on a corner and max % cell == 0 else 0), 0)
offset = first·cell − min                       // ≤ 0
if step == 1 or size == 1: minOut = offset; cellSteps = cell
else: if offset % cell != 0: no plan            // tests against cell, not step
      minOut = offset / step; cellSteps = cell / step
corners: s1 − s0 + 1 from s0·cell, one cell apart; output cells last − first + 1
```

`isMultipleOf` (`%` above) is Java's `irem`, so `−8 % 4` is 0. Integer arithmetic wraps.

- If any axis has no plan (`sampleAndUpscaleFallback`), the box of every block from the volume's minimum to its last position (`(size − 1)·step + 1` per axis, steps of 1) is upscaled, each of its axes planned with step 1 (which always has a plan), then every `step`th value is picked. The `offset % cell` test sends every stepped volume whose minimum isn't on a corner here.
- If all three axes are exactly aligned, the value is the input's own volume.
- Otherwise the input is sampled over the corner volume (each axis's `s1 − s0 + 1` corners from `s0·cell`, step `cell`; an exactly aligned axis's own positions), and upscaled (`Upscaler.upscale`) by the upscaler for the **plans'** modes and cell sizes, not the function's: an exactly aligned axis is nearest with cell 1, and a stepped axis's cell is `cellSteps`, so its table has `cellSteps` entries at `t = i / (float) cellSteps` and its reciprocal is `1.0f / cellSteps`.
- `doUpscale`: for each output cell `cz`, then `cx`, then `cy`, the cell starts at output position `minOut + c·cell` on each axis and reads corners `c` to `c + count − 1`, each index clamped to the corner volume (`min(c + k, size − 1)`). For each y cell after the first, the y samples slide down one and the next corner is read. `fillCell` writes the cell's offsets inside the volume: from `max(0, −o)` to `min(cell, size − o) − 1` along each axis, with `o` the cell's start, or just offset 0 along an axis whose cell is 1. Per `dz`, `tz[i] = coefZ[dz].combine(samples, i, strideZ)`; then:
  - x linear and y nearest (as the upscaler's modes, so whenever x is linear and y exactly aligned: always for a y cell of 1, as in `chunk_surface_level`): `a = tz[0]`, `b = tz[1]`, `step = (b − a) · (1.0f / cx)`, and per `dx` the value is `a + step · (float) dx`, the same down y;
  - else, per `dx`, `tx[j] = coefX[dx].combine(tz, j, strideX)`; then per `dy`: nearest is `tx[0]`; linear is `a + step · (float) dy` with `a = tx[0]`, `step = (tx[1] − a) · (1.0f / cy)`, computed directly for each `dy` (not accumulated); other modes (cosine included) use their coefficients.
- A stepped volume therefore gives the values of the blocks it picks when it falls back, and when it's planned with power-of-two steps (`t` is the same division, and the reciprocals differ by an exact power of two).

**Linear `upscale` against 26.3's `interpolated`.** With the same cell sizes the results aren't bit-identical:

- Weights: a linear axis computes `(1 − t)·a + t·b` with `t = i / (float) cell`, where 26.3 computes `a + t·(b − a)` with `t = i · (1.0f / cell)` in the volume path.
- Point order: z, then x, then y. 26.3's point path is x, then y, then z (`lerp3`).
- Volume y: `a + ((b − a) · (1.0f / cy)) · (float) dy` for each block, where 26.3 adds `step` to the last value from the volume's first y. So 26.4's volume value depends only on a block's offsets in its cell, not on where the volume starts: columns, chunks and any aligned box agree.
- Point and volume still differ in the low bits: y uses weights on the point path and the step form on the volume path.
- At `t = 0` the weighted form gives `1·a + 0·b`: NaN if `b` is infinite or NaN, and `+0` for `a = −0`, where an exactly aligned axis returns `a`. Edge cases only.

**All nearest** (`NearestNeighborSampler`): point is the input at `(floorDiv(x, cx)·cx, floorDiv(y, cy)·cy, floorDiv(z, cz)·cz)`. A volume that fits the grid (`DensityVolume.fitsOnGrid`: on each axis one position deep or a step that's a multiple of the cell, and a minimum whose `floorMod` by the cell is 0) is the input's own volume. Otherwise the input is sampled over the cells' corners, from `floorDiv(min, cell)` to `floorDiv(last, cell)` on each axis, one cell apart, and each position takes its cell's corner. `last` is 26.4's `maxBlock`, the last position, `min + (size − 1)·step` (26.3's `maxBlock` was `min + size·step − 1`). Unused in vanilla.

Its range is in [Ranges](#ranges).

#### `find_top_surface`

Compiled as a slice at y 0, so it never depends on y and `upper_bound` is sampled at `(x, 0, z)`. With `upper` that value:

1. `start = Mth.floor(upper / (float) cell_height) · cell_height` (int, wrapping).
2. If `start ≤ lower_bound`, the result is `lower_bound`. Otherwise, from `start` down to `lower_bound` in steps of `cell_height`, the first y where `density` (always sampled at a point) is greater than 0. If none, `lower_bound`.
3. The result is the int as `f32`. It is never below `lower_bound`.

Its volume value equals its point value whenever `upper_bound`'s does, which holds for 26.3's.

_Versions:_ 26.2 divides by `(double) cell_height` in `f64`, has no slice (`upper_bound` is evaluated at the caller's position; its callers pass y 0), and evaluates `density` in `f64` ([below](#262-types)).

#### `old_blended_noise`

`BlendedNoise`, built from the seed's positional factory with `fromHashOf("minecraft:terrain")` (`CompileContext.createRandom`), or from `LegacyRandomSource(seed)` with legacy randomness ([below](#binding-to-a-seed)):

- `xzMult = 684.412 · xz_scale`, `yMult = 684.412 · y_scale`, `smear = yMult · smear_scale_multiplier`, `mainSmear = smear / y_factor`, in `f64`.
- Three stacks of [smeared Perlin noise](noise.md#smeared-perlin-smearedperlinnoise) from that one random, in order, each layer's noise taking the next draws: `minLimit` (first octave −15, `smear`, amplitude factor `0.9999847412109375`), `maxLimit` (the same), `main` (first octave −7, `mainSmear`, 12.75). A stack with first octave `o` has `n = 1 − o` layers (`createFbm`); layer `i` from the first has frequency `2^−i`, amplitude `factor / (2^n − 1) · 2^i` as `f32`, and smear scale `s · 2^−i`.
- It compiles to existing samplers: `lerp(clamp(main + 0.5f, 0, 1), minLimit, maxLimit)` with the `lerp` type's rule above, each stack sampled as `noise` with no shifts. `minLimit` and `maxLimit` have scales `xzMult` and `yMult`; `main` has `xzMult / xz_factor` and `yMult / y_factor`, divided once, so it samples at `(X · (xzMult / xz_factor), …)`.

_Versions:_ 26.2's `old_blended_noise` is `BlendedNoise.compute`, a different algorithm in `f64` that skips the limit stacks when the main value saturates ([below](#262-old_blended_noise)).

#### `distance_to_point`

From the 26.3 bytecode (`DistanceToPointFunction`). At block `(x, y, z)`, with `d = (float)(point − pos)` per axis (an `int` subtraction, wrapping, then widened), in `f32`:

- `euclidean`: `(float) sqrt((double)((dx·dx + dy·dy) + dz·dz))`;
- `euclidean_squared`: `(dx·dx + dy·dy) + dz·dz`;
- `manhattan`: `(|dx| + |dy|) + |dz|`;
- `chebyshev`: `max(max(|dx|, |dy|), |dz|)`.

Its volume value is the point value at each block.

#### `end_outer_islands`

From the 26.3 bytecode (`EndIslandFunction`). Binding seeds `LegacyRandomSource(seed)` (always, whatever `legacy_random_source` says), skips 17,292 draws (`consumeCount`), then builds a [simplex noise](noise.md#simplex-simplexnoise) with zero offsets.

At block `(x, y, z)` the value is `(height(x / 8, z / 8) − 8) / 128` in `f32`, the divisions by 8 truncating toward zero; y isn't read. `height(x, z)`:

1. `xs = x / 2`, `zs = z / 2`, `xr = x % 2`, `zr = z % 2` (truncating `int` division and remainder); `h = −100f`.
2. For `dx` from −12 to 12, then `dz` from −12 to 12: `px = (long)(xs + dx)`, `pz = (long)(zs + dz)`, the sums in wrapping `i32`. Skip unless `px² + pz² > 4096` and `noise.get((double)px, (double)pz) < −0.9f`.
3. Otherwise `size = (|(float)px| · 3439f + |(float)pz| · 147f) % 13f + 9f` (`%` is Java's `frem`), `fx = (float)(xr − dx · 2)`, `fz = (float)(zr − dz · 2)`, `v = 100f − (float) sqrt((double)(fx·fx + fz·fz)) · size`, clamped to `[−100, 80]`; `h = max(h, v)`.

There's no central-island term: 26.3's `end/islands` adds the main island as `distance_to_point` in data. The volume value is the point value, filled up each column.

_Versions:_ 26.2 has `end_islands` instead, with the central island in code, on the `/8` coordinates, and the `(h − 8) / 128` step in `f64` ([below](#262-end_islands)). 26.4-snapshot-5 clamps with `Math.clamp`, which gives the same values here (bounds −100 and 80).

### Ranges

`min` and `max` compile by their operands' ranges (`net.minecraft.util.Interval`, `f32` bounds), so spyglass computes ranges as the game does, bound for bound. An interval is `[min, max]` or NaI (not an interval, NaN bounds). `Interval.of` throws on NaN or reversed bounds; spyglass reports that as a binding error. `Math.min` and `Math.max` order `−0` below `0` and return NaN if either operand is.

| Type                                                   | Range                                                                                                                                                                                                                                                                                                                                                                                         |
| ------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `constant`                                             | exactly the value                                                                                                                                                                                                                                                                                                                                                                             |
| `noise`                                                | `±(float)(sum · 0.3333333333333333 · 6.0)`, with `sum` the octaves' absolute amplitudes before normalization, summed as for the [normalization factor](noise.md#normal-noise)                                                                                                                                                                                                                 |
| `shift_a`, `shift_b`, `shift`                          | the noise's range times `[4, 4]`                                                                                                                                                                                                                                                                                                                                                              |
| `add`, `sub`                                           | `[a.min + b.min, a.max + b.max]`, `[a.min − b.max, a.max − b.min]`; NaI if a bound is NaN                                                                                                                                                                                                                                                                                                     |
| `mul`                                                  | the min and max of the four bound products, each `mulBound(p, q)`: `0` if either is zero (so `0 · ∞ = 0`), else `p · q`                                                                                                                                                                                                                                                                       |
| `div`                                                  | `mul(a, reciprocal(b))`. `reciprocal`: NaI for `[0, 0]`; `[1/max, 1/min]` if 0 is outside; `[−∞, 1/min]` if `max` is 0; `[1/max, ∞]` if `min` is 0; else `[−∞, ∞]`                                                                                                                                                                                                                            |
| `min`, `max`                                           | the bounds' min (or max) pairwise                                                                                                                                                                                                                                                                                                                                                             |
| `abs`, `square`                                        | `[0, top]` if the range holds 0, else `[min(abs(min), abs(max)), top]`, with `top` the larger magnitude (squared)                                                                                                                                                                                                                                                                             |
| `cube`, `half_negative`, `quarter_negative`, `squeeze` | each bound mapped through the function (`mapMonotonic`, which throws on NaN), then ordered                                                                                                                                                                                                                                                                                                    |
| `negate`, `reciprocal`, `sqrt`                         | `sub([0, 0], r)`; `reciprocal(r)`; `pow(r, [0.5, 0.5])`                                                                                                                                                                                                                                                                                                                                       |
| `log`                                                  | NaI if `max < 0`; else `max(r, [0, 0])`, each bound mapped by `(float) Math.log`                                                                                                                                                                                                                                                                                                              |
| `sign`                                                 | `signum` of an exact range; with 0 inside, `[0, 1]`, `[−1, 0]` or `[−1, 1]` by which bound is 0; else `1` or `−1`                                                                                                                                                                                                                                                                             |
| `floor`, `round`, `ceil`, `truncate`                   | `mul(mapMonotonic(div(input, multiple), R), multiple)`                                                                                                                                                                                                                                                                                                                                        |
| `clamp`                                                | `[max, max]` if `r.min ≥ max`, `[min, min]` if `r.max ≤ min`, else the range cut to `[min, max]`                                                                                                                                                                                                                                                                                              |
| `pow`                                                  | [below](#pow-ranges)                                                                                                                                                                                                                                                                                                                                                                          |
| `cache`, `blend_density`, `slice`, `interpolated`      | the input's                                                                                                                                                                                                                                                                                                                                                                                   |
| `lerp`                                                 | the span of `lerp(alpha, f, s)` for the four pairs of `first`'s and `second`'s bounds. With finite `f` and `s`: the span of `f + mulBound(t, s − f)` at `alpha`'s bounds. Otherwise `[f, f]` if `f == s`, else at each bound `t`: `mulBound(1 − t, f) + mulBound(t, s)`, or when both terms are infinite, `t ≤ 0` gives `−∞` if `s > f` else `∞`, `t ≥ 1` the opposite, and anything else NaI |
| `range_choice`, `interval_select`                      | the span of the branches' ranges, ignoring NaI ones (NaI if all are)                                                                                                                                                                                                                                                                                                                          |
| `find_top_surface`                                     | `[lower, max(lower, upper_bound's max)]`                                                                                                                                                                                                                                                                                                                                                      |
| `old_blended_noise`                                    | `computeFbmRange(−15, yMult · smear_scale_multiplier, 0.9999847412109375)`: starting from `[0, 0]`, add each `minLimit` layer's `±(float)(abs(s · f) + 2.0)` times `[(float) a, (float) a]`, with `f` and `a` the layer's frequency and amplitude                                                                                                                                             |
| `gradient`                                             | the span of `from_value` and `to_value`                                                                                                                                                                                                                                                                                                                                                       |
| `spline`                                               | [below](#spline-ranges)                                                                                                                                                                                                                                                                                                                                                                       |
| `blend_alpha`; `blend_offset`, `beardifier`            | `[0, 1]`; `[−∞, ∞]`                                                                                                                                                                                                                                                                                                                                                                           |
| `distance_to_point`; `end_outer_islands`               | `[0, ∞]`; `[−0.84375, 0.5625]`                                                                                                                                                                                                                                                                                                                                                                |

_Versions:_ 26.4-snapshot-5's `upscale` range is the input's range `r` modified per axis, z, then x, then y (`UpscaleFunction.Axis.modifyRange`): for each entry of the axis's coefficient table (`computeTable(cell_size, mode)`, the function's own), `Σ_k ofExact(c_k) · r` (Interval `mul` and `add`, from `ofExact(0)`, in coefficient order; nearest's one weight is 1), then `encapsulating` over all entries. Catmull-Rom's negative weights widen it (a cell of 2 takes `[0, 1]` to `[−0.125, 1.125]`), and rounding can too, where 26.3's `interpolated` keeps `r`. That only matters to range-driven compile choices (`min`, `max`, `clamp`), so for NaN and signed zeros. 26.2's ranges are `f64` and its own ([below](#262-ranges)).

#### Spline ranges

`CubicSpline.Multipoint.range()`, with `c` the coordinate's range (NaI if it is), `lo = ∞`, `hi = −∞`, and `extend(x, v, i) = v + d_i · (x − loc_i)` (or `v` if `d_i` is 0):

1. If `c.min < loc_0`: widen `lo` and `hi` by `extend(c.min, v, 0)` for both bounds `v` of the first value's range. If `c.max > loc_last`: likewise with `c.max` and the last value.
2. Widen by every value's range.
3. For each interval `i` where `d_i` or `d_{i+1}` isn't 0, with `dx = loc_{i+1} − loc_i`, `p = d_i · dx`, `q = d_{i+1} · dx`, and the two values' ranges `[a0, b0]` and `[a1, b1]`: `lo = min(lo, min(a0, a1) + 0.25 · min(p − b1 + a0, −q + a1 − b0))` and `hi = max(hi, max(b0, b1) + 0.25 · max(p − a1 + b0, −q + b1 − a0))`, each sum left to right.
4. The range is `of(lo, hi)`.

#### `pow` ranges

`Interval.pow(base, exponent)`: NaI if either is. An exact base `b` gives `pow(b, exponent)`; otherwise the span of `pow(base.min, e)` and `pow(base.max, e)`, widened by `pow(0, e)` if the base range holds 0 and `max > 0`, and by `pow(−0, e)` if it holds 0 and `min < 0`. For a fixed base `b` (`(float) Math.pow` throughout):

- An exact exponent: `[b^e, b^e]`, or NaI if NaN.
- `b = ±0`: `powZeroBase` times `[±1, ±1]` (the sign of `b`), where `powZeroBase` is `[1, ∞]` if the exponent's max is 0, `[0, 1]` if its min is 0, `[0, ∞]` if it holds 0 otherwise, `[∞, ∞]` if it's negative, else `[0, 0]`.
- `b = 1`: `[1, 1]`.
- `b > 0`, finite exponent bounds: the span of `b^min` and `b^max`. With an infinite bound: `[0, ∞]` if both are; with `min = −∞`, `[b^max, ∞]` if `b < 1` else `[0, b^max]`; with `max = ∞`, `[0, b^min]` if `b < 1` else `[b^min, ∞]`.
- `b < 0`: only integer exponents are real. With `lo = ceil(min)` and `hi = floor(max)` (as `f32`): NaI if `hi < lo`; else the span of `b^lo` and `b^hi`, widened by `−b^lo` if `lo` is infinite or else by `b^(lo+1)` if `lo + 1 < hi`, and by `−b^hi` if `hi` is infinite or else by `b^(hi−1)` if `hi − 1 > lo`.

## Binding to a seed

A loaded graph doesn't depend on the seed. Binding creates the seed's noises as `RandomState` does: a positional factory `XoroshiroRandomSource(seed).forkPositional()`, and for each noise id `NormalNoise.create(factory.fromHashOf(id))`, once per id (`getOrCreateNoise`).

With `legacy_random_source: true` (the Nether and the End), from the 26.3 bytecode: Legacy changes only where randomness comes from; every sampler is the same:

- The factory is `LegacyRandomSource(seed).forkPositional()` ([rng.md](rng.md#legacy-legacyrandomsource)), for noises and for random factories (`fromHashOf(id).forkPositional()`, which the aquifer would use; neither dimension has one).
- `old_blended_noise` is built from `LegacyRandomSource(seed)` itself, not `fromHashOf("minecraft:terrain")`.
- `end_outer_islands` is seeded from `LegacyRandomSource(seed)` whatever the flag says ([below](#end_outer_islands)).
- `minecraft:nether/temperature` and `minecraft:nether/vegetation` are built by their own rule whatever the flag says ([noise.md](noise.md#the-nether-biome-noises)), and aren't remembered per id.

26.3 has no other legacy case: 1.21's override of the shift noise is gone.

_Versions:_ 26.2 binds the same way (`RandomState` and its `NoiseWiringHelper`): the same factories and ids, `old_blended_noise` from `LegacyRandomSource(seed)` or `fromHashOf("minecraft:terrain")`, `end_islands` from `LegacyRandomSource(seed)` whatever the flag, the Nether biome noises by their own rule, and no shift-noise override. Its noises are evaluated in `f64` ([noise.md](noise.md)). The aquifer's factory is `fromHashOf("minecraft:aquifer").forkPositional()`, and ore veins use a factory from the id `"minecraft:ore"` (its derivation not traced in the bytecode).

## Point and volume evaluation

The game has two paths: `sampleValue` at a point, and `sampleVolume` over a buffer (`DensityVolume`: sizes, minimum block and step per axis; values with y fastest, then x, then z). Climate lookups and the aquifer use points. Chunk generation and the generator's columns ([terrain.md](terrain.md)) sample `final_density` as a volume, so terrain values follow the volume path. spyglass has both ([ADR 0022](../adr/0022-terrain-crate-and-volume-evaluation.md)): `Sampler::sample` and `Sampler::sample_volume`.

Over a volume, each sampler samples its inputs over the same volume, then combines them position by position with its point formula, except:

- **`interpolated`**: different arithmetic, [above](#interpolated). A point inside a cell still samples its corners through the volume path.
- **`mul`** of two non-constant operands, and **`div`**: no zero test; the volume path multiplies or divides, so `0 · −1` is `−0` and `0 · ∞` NaN where the point path gives `+0`.
- **`min`, `max`**: no range shortcut and no `Math.min`/`Math.max`: the right value replaces the left only if it's strictly below (above) it. A constant operand likewise. They differ from the point path only for NaN and signed zeros.
- **`noise`** with no shifts (`NoiseFunction.Sampler`, also `shift_a`, `shift`, and old blended noise's stacks): the stack's own volume path (`addToVolume`) from a buffer of zeros. Each layer is passed `scale · f` and `1.0f · amplitude`, and each Perlin octave samples `wrap(block · scale) + offset`, against `wrap((X · scale) · f) + offset` on the point path. They differ when neither the scale nor the octave's frequency is a power of two (a normal noise's second stack, `f = 2^k · 1.0181268882175227`, with y scales like 0.67), though rarely after the fraction is narrowed to `f32`. Perlin's volume path sums each corner's gradient dot as `(gx·xf + gz·zf) + gy·yf` (the game's `dotXz`, then y), which differs only in the sign of an exact zero. A noise with shifts samples its shifts over the volume, then the noise's point path at `block · scale + shift`.
- **`shift_b`**: the noise over a volume with x and z swapped and y 0 (`(sizeZ, sizeX, 1)` from `(minZ, minX, 0)`), with the `4` folded into each layer's weight (`4.0f · amplitude`), copied up each column. Scaling by 4 is exact, so it equals the point value.
- **`lerp`**, **`range_choice`** and **`interval_select`** sample every branch over the whole volume, then choose per position as the point path does.
- **`find_top_surface`** samples `upper_bound` over the volume (the game requires it one block deep, which its slice guarantees), then searches each column with `density` at points.
- **Splines** sample every coordinate they hold over the volume, then evaluate per position.

_Versions:_ 26.4-snapshot-5 has the same paths, with [`upscale`](#upscale) in place of `interpolated` (its point path also samples corners through the volume path; its volume value at a block doesn't depend on where the volume starts), and `clamp`'s [range-chosen samplers](#terrain-types) under switch `clamp_positive_zero`. Its range choices (`range_choice`, `interval_select`) sample the first-needed branch over the whole volume and stop if every position chose it, else every other needed branch over the whole volume: the same values as 26.3 (`interval_select` is not confirmed in the bytecode, only its structure). 26.2 has no volume arithmetic at all: every `fillArray` ends in each position's point value, and only the `NoiseChunk`'s interpolation and caches differ from a point ([below](#262-noisechunk)).

The climate's scales are 0.25 and 1, so its volume and point values are equal: the probe compares them for the six climate functions over whole chunks (4 chunks × 10 seeds in 26.3), and fails if that changes.

**Slices.** `SLICE_UNIFORM_AXES` wraps nodes that don't depend on an axis in a slice at coordinate 0, so over a volume they're sampled one position deep and copied along it. spyglass does the same for y alone, with the nodes [that don't depend on y](#remembered-values), as a speed-up: a node that doesn't depend on y has the same volume value at every y.

**Caches.** `cache` is 26.3's only caching type (there's no `flat_cache` or `cache_2d`); structurally equal inputs share one. In chunk generation and the generator's columns caches are on: each keeps the last volume it filled and the last point it computed. A volume sample of a cache returns what its input gives over that same volume, so it only saves work. A point sample inside the cache's last volume returns that volume's value instead of the point value, which differs only where the input's volume and point values differ. spyglass doesn't model caches. In 26.3's overworld, the points sampled through caches are `find_top_surface`'s density and the aquifer's functions. None of the caches they reach holds an `interpolated`, a `min` or `max`, a `mul` or `div` of two non-constant operands, or an unshifted noise at a scale that isn't a power of two: only splines, shifted noises, gradients and arithmetic with a constant operand, whose volume and point values agree. So the result is the same, and a data pack that broke this could make spyglass differ. The probe's column and chunk cases are sampled through caching contexts and agree bit for bit.

_Versions:_ 26.2 has four cache markers (`flat_cache`, `cache_2d`, `cache_once`, `cache_all_in_cell`) besides `interpolated`, all identity outside a `NoiseChunk`. Inside one, `flat_cache` changes values: it returns the value at the quart corner ([below](#262-caches)).

## Remembered values

Evaluation remembers values per thread, so work isn't repeated:

- **Shared nodes** (a named function referenced from several places, as after `INLINE_REFERENCE`) are computed once per position, across every function bound by the same `Binder`. The six climate functions share their shifts and the continents, erosion and ridges noises this way.
- **Nodes that don't depend on y** are computed once per column: consecutive samples at the same `(x, z)` reuse them, as the game's `SLICE_UNIFORM_AXES` rewrite and `cache_2d` do. Callers that fill a region go column by column.

Each `Binder` numbers its nodes' slots, and the memo is stamped with the binder, the column and the y it was filled at, so values never leak between seeds, columns or heights. Samplers that evaluate their input somewhere else (`slice`, `find_top_surface`) move the memo there and back, so their input's values carry that position's stamps. Remembering changes when a floating-point operation runs, never which ones run or their order, so the results are the same bits. Tests compare remembered values with fresh ones down and up columns, and between two seeds' samplers alternating at one position.

`Sampler::depends_on_y` says whether a bound function's value can depend on y.

What depends on y:

- `shift` and a `gradient` along y do. Constants, `shift_a`, `shift_b` and other gradients don't.
- A `noise` depends on y if its `y_scale` isn't 0 or its `shift_y` depends on y. With a y scale of 0 the y input is `+0.0` or `-0.0`, depending on the sign of y (plus a y-independent shift). Each Perlin octave adds its y offset, `nextDouble() · 256`, which is never `-0.0`, so the two zeros give the same noise.
- A `slice` along y and `find_top_surface` don't, whatever their inputs.
- An `upscale` (26.4) with a y cell above 1 does, whatever its input: equal corners along y still give `(1 − t)·a + t·a`, which isn't always `a`, and a position on a corner takes the input's point value where others take its volume path. With a y cell of 1 it depends on y if its input does: every y is on a corner, and a volume one position deep at y 0 plans y the same way as a taller one.
- Everything else depends on y if any input does, including a spline's coordinate or any of its point values.

## The climate sampler

`NoiseRouter.createClimateSampler` binds six router entries: `temperature`, `vegetation`, `continents`, `erosion`, `depth`, `ridges` (each any density function value: a reference in the vanilla data, but a number or an inline object is accepted too, as by the game's codec), which feed the climate parameters temperature, humidity, continentalness, erosion, depth and weirdness. `Climate.Sampler.sample(qx, qy, qz)` evaluates each at block `(4·qx, 4·qy, 4·qz)` (`QuartPos.toBlock`, no centring), and `Climate.target` quantizes each with `quantizeCoord(f) = (long)(f · 10000.0f)`: an `f32` multiply, then truncation.

_Versions:_ 26.2's `Climate.Sampler.sample` evaluates each of the six in `f64` at `SinglePointContext(4·qx, 4·qy, 4·qz)`, through a visitor that replaces references by their value and every cache marker by its input (so `flat_cache` and the rest are identity here), then narrows with `(float)` and quantizes as above. 26.4-snapshot-5's sampler is the same as 26.3's.

## 26.2 evaluation

From the 26.2 bytecode (package `net.minecraft.world.level.levelgen`: `DensityFunction`, `DensityFunctions`, `NoiseChunk`, `NoiseRouter`). Not yet checked against the oracle.

A function is evaluated with `double compute(FunctionContext)`, where the context gives the block's `int` x, y and z. Everything is `f64`. `fillArray(array, provider)` sets each `array[i]` to `compute(provider.forIndex(i))`: there is no volume arithmetic. Only the `NoiseChunk`, the chunk generator's evaluation context, interpolates and caches ([below](#262-noisechunk)); outside it, every cache marker and `interpolated` returns its input's value at the point.

### 26.2 types

`X`, `Y`, `Z` are the block coordinates as `f64`. Constants and noise values are `f64`.

| Type                                                                        | Fields                                                                                          | Value                                                                                                                                                                                                                                                                                    |
| --------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `constant`                                                                  | `argument` (number)                                                                             | the value, as `f64`                                                                                                                                                                                                                                                                      |
| `add`                                                                       | `argument1`, `argument2`                                                                        | `a1 + a2`. A constant operand (`argument1` tested first) compiles to `x + c` with the other operand `x`, the same `f64`                                                                                                                                                                  |
| `mul`                                                                       | `argument1`, `argument2`                                                                        | A constant operand: `x · c`, no zero test. Else `a = argument1`; `+0.0` if `a == 0.0` (so `−0.0` too), without evaluating `argument2`; else `a · argument2`. Only `argument1` is tested                                                                                                  |
| `min`                                                                       | `argument1`, `argument2`                                                                        | `a = argument1`; `a` if `a < argument2.minValue()`, without evaluating `argument2`; else `Math.min(a, argument2)`. No constant specialization                                                                                                                                            |
| `max`                                                                       | `argument1`, `argument2`                                                                        | `a` if `a > argument2.maxValue()`; else `Math.max(a, argument2)`                                                                                                                                                                                                                         |
| `abs`, `square`, `cube`                                                     | `argument`                                                                                      | `Math.abs(x)`; `x·x`; `(x·x)·x`                                                                                                                                                                                                                                                          |
| `half_negative`, `quarter_negative`                                         | `argument`                                                                                      | `x` if `x > 0.0`, else `x · 0.5` (`0.25`)                                                                                                                                                                                                                                                |
| `invert`                                                                    | `argument`                                                                                      | `1.0 / x` (26.3's `reciprocal`, in `f64`)                                                                                                                                                                                                                                                |
| `squeeze`                                                                   | `argument`                                                                                      | `c = Mth.clamp(x, −1.0, 1.0)`; `c / 2.0 − ((c·c)·c) / 24.0`                                                                                                                                                                                                                              |
| `clamp`                                                                     | `input`, `min`, `max`                                                                           | `Mth.clamp`: `min` if `x < min`, else `Math.min(x, max)`                                                                                                                                                                                                                                 |
| `noise`                                                                     | `noise`, `xz_scale`, `y_scale` (both required)                                                  | `noise.get(X·xz, Y·y, Z·xz)`, the `f64` [normal noise](noise.md#normal-noise)                                                                                                                                                                                                            |
| `shifted_noise`                                                             | as `noise`, and `shift_x`, `shift_y`, `shift_z` (functions)                                     | `noise.get(X·xz + sx, Y·y + sy, Z·xz + sz)`, the shifts evaluated in the order x, y, z. A constant `0.0` shift is still added, which only matters for a `−0.0` product                                                                                                                   |
| `shift_a`, `shift_b`, `shift`                                               | `argument` (a noise id)                                                                         | `noise.get(X·0.25, 0.0·0.25, Z·0.25) · 4.0`; `noise.get(Z·0.25, X·0.25, 0.0) · 4.0`; `noise.get(X·0.25, Y·0.25, Z·0.25) · 4.0`                                                                                                                                                           |
| `y_clamped_gradient`                                                        | `from_y`, `to_y` (ints), `from_value`, `to_value`                                               | `Mth.clampedMap(Y, from_y, to_y, from_value, to_value)`: `t = (Y − from_y) / (to_y − from_y)`; `from_value` if `t < 0`, `to_value` if `t > 1`, else `from_value + t·(to_value − from_value)`. Not 26.3's `gradient`: it divides the coordinate, where 26.3 multiplies by an `f32` factor |
| `range_choice`                                                              | `input`, `min_inclusive`, `max_exclusive`, `when_in_range`, `when_out_of_range`                 | `when_in_range` if `min ≤ input < max`, else `when_out_of_range` (NaN is out). Only the chosen branch is evaluated                                                                                                                                                                       |
| `interval_select`                                                           | `input`, `thresholds` (ascending, checked by the codec), `functions` (one more than thresholds) | `functions[i]` for the first `i` with `input < thresholds[i]`, else the last                                                                                                                                                                                                             |
| `spline`                                                                    | `spline`                                                                                        | `(double)` of the spline's `f32` value, each coordinate being `(float)` of its function's `f64` value. The spline arithmetic is [26.3's](#types)                                                                                                                                         |
| `find_top_surface`                                                          | `density`, `upper_bound`, `lower_bound` (int), `cell_height`                                    | `start = Mth.floor(upper / (double) cell_height) · cell_height` (int), with `upper` evaluated at the caller's position. Then as [26.3's](#find_top_surface), with `density` evaluated in `f64` at `SinglePointContext(x, y, z)`. The result is the int as `f64`                          |
| `end_islands`                                                               | none                                                                                            | [Below](#262-end_islands)                                                                                                                                                                                                                                                                |
| `old_blended_noise`                                                         | `xz_scale`, `y_scale`, `xz_factor`, `y_factor`, `smear_scale_multiplier`                        | [Below](#262-old_blended_noise)                                                                                                                                                                                                                                                          |
| `blend_alpha`, `blend_offset`, `blend_density`                              | none; none; `argument`                                                                          | 1.0, 0.0, the input: nothing blends for new terrain                                                                                                                                                                                                                                      |
| `beardifier`                                                                | none                                                                                            | 0.0. Not in the vanilla data: the `NoiseChunk` adds it to `final_density` in code                                                                                                                                                                                                        |
| `interpolated`, `flat_cache`, `cache_2d`, `cache_once`, `cache_all_in_cell` | `argument`                                                                                      | the input, outside a `NoiseChunk`. No cell size fields: cells come from the noise settings. Inside a `NoiseChunk`, [below](#262-noisechunk)                                                                                                                                              |

There is no `sub`, `div`, `negate`, `lerp`, `gradient`, `slice`, `pow`, `sqrt`, `log`, `sign`, rounding type, `distance_to_point` or `end_outer_islands`, and no `cache`. A constant is the `constant` type or a bare number. The `abs`, `square`, `cube`, `half_negative`, `quarter_negative`, `invert` and `squeeze` types are one class (`Mapped`).

### 26.2 ranges

`minValue()` and `maxValue()` are `f64`, computed when a function is built. They only steer `min` and `max`: `a < b.minValue()` means `Math.min(a, b) = a` unless `b` is NaN, so a range only decides whether a NaN `argument2` propagates (and whether `argument2` is evaluated).

- `constant`: the value. `noise`, `shifted_noise`: `±` the normal noise's maximum. `y_clamped_gradient`: the span of the two values. `clamp`: `[min, max]`. `end_islands`: `[−0.84375, 0.5625]`. `blend_alpha`: 1. `blend_offset`, `beardifier`: 0. `blend_density`: `[−∞, ∞]`. Other markers and references: the input's.
- `add`: `[a.min + b.min, a.max + b.max]`. `min`, `max`: the bounds' `Math.min` (or `Math.max`) pairwise.
- `mul` (`a`, `b` the operands' ranges): the minimum is `a.min · b.min` if both minimums are positive, else `a.max · b.max` if both maximums are negative, else `Math.min(a.min · b.max, a.max · b.min)`. The maximum is `a.max · b.max` if both minimums are positive, else `a.min · b.min` if both maximums are negative, else `Math.max(a.min · b.min, a.max · b.max)`.
- With `r` the input's range and `f` the type's function: `abs`, `square`: `[Math.max(0, r.min), Math.max(f(r.min), f(r.max))]`. `invert`: `[−∞, ∞]` if `r.min < 0 < r.max`, else `[f(r.max), f(r.min)]`. `cube`, `half_negative`, `quarter_negative`, `squeeze`: `[f(r.min), f(r.max)]`.
- `range_choice`: the span of both branches. `interval_select`: the span of every function's range. `find_top_surface`: `[lower, Math.max(lower, upper_bound.max)]`. `spline`: the spline's `f32` bounds widened (the spline's own rule is not confirmed in the bytecode). `shift_a`, `shift_b`, `shift` and `old_blended_noise`: not traced.

### 26.2 `end_islands`

`DensityFunctions.EndIslandDensityFunction`. Seeded like 26.3's `end_outer_islands`: `LegacyRandomSource(seed)`, `consumeCount(17292)`, then a simplex noise. The value is `((double) height(x / 8, z / 8) − 8.0) / 128.0`, with `height` in `f32`:

1. `xs = x / 2`, `zs = z / 2`, `xr = x % 2`, `zr = z % 2` (truncating).
2. `h = 100f − (float) sqrt((float)(x·x + z·z)) · 8f`, the squares and sum in wrapping `i32`, then clamped to `[−100, 80]` (`Mth.clamp`). This is the central island, on the `/8` coordinates.
3. The outer islands exactly as [26.3's](#end_outer_islands), each `v` raising `h` to `max(h, v)`.

26.3's `end/islands` takes the central term from the unquantized block distance and applies `(h − 8) / 128` in `f32` before taking the max, so near the main island every value differs. 26.2 needs its own sampler.

### 26.2 `old_blended_noise`

`synth.BlendedNoise.compute`. Built as [26.3's](#old_blended_noise) (the same random, stacks and draws; the constant 684.412 is not confirmed in the bytecode). In `f64`, with `wrap` and the five-argument `ImprovedNoise.noise(x, y, z, smear, yFloor)` from [noise.md](noise.md#smeared-perlin-smearedperlinnoise):

```text
X = x·xzMult; Y = y·yMult; Z = z·xzMult
mX = X / xz_factor; mY = Y / y_factor; mZ = Z / xz_factor
smear = yMult · smear_scale_multiplier; mainSmear = smear / y_factor
main = 0; f = 1
for i in 0..8:   if main octave i exists: main += octave.noise(wrap(mX·f), wrap(mY·f), wrap(mZ·f), mainSmear·f, mY·f) / f
                 f /= 2
t = (main / 10 + 1) / 2
skipMin = t >= 1; skipMax = t <= 0
min = 0; max = 0; f = 1
for i in 0..16:  wx = wrap(X·f); wy = wrap(Y·f); wz = wrap(Z·f); s = smear·f
                 if not skipMin and minLimit octave i exists: min += octave.noise(wx, wy, wz, s, Y·f) / f
                 if not skipMax and maxLimit octave i exists: max += octave.noise(wx, wy, wz, s, Y·f) / f
                 f /= 2
value = Mth.clampedLerp(t, min / 512, max / 512) / 128     // t < 0: min/512; t > 1: max/512; else lerp
```

Which layer `getOctaveNoise(i)` returns, and `wrap`, are [noise.md](noise.md)'s. This isn't 26.3's compiled `lerp` of three noises.

### 26.2 `NoiseChunk`

The chunk generator evaluates terrain through a `NoiseChunk`, for a chunk (`cellCountXZ = 16 / cellWidth`) or a base column (`cellCountXZ = 1`) starting at block `(x0, z0)`:

- `cellWidth = 4 · size_horizontal`, `cellHeight = 4 · size_vertical` (from the noise settings, [below](#262-router)). `cellCountY = floorDiv(height, cellHeight)`, `cellNoiseMinY = floorDiv(min_y, cellHeight)`. `firstCellX = floorDiv(x0, cellWidth)`; `firstNoiseX = x0 >> 2` (z alike); `noiseSizeXZ = (cellCountXZ · cellWidth) >> 2`.
- The router is mapped (`mapAll(wrap)`) so each marker becomes this `NoiseChunk`'s implementation: `interpolated` → `NoiseInterpolator`, `flat_cache` → `FlatCache`, `cache_2d` → `Cache2D`, `cache_once` → `CacheOnce`, `cache_all_in_cell` → `CacheAllInCell`, `blend_density` → its input, `beardifier` → the chunk's beardifier (0 for a base column).
- The density for blocks is `cache_all_in_cell(add(final_density, beardifier))`, mapped the same way. The `add` isn't a constant specialization, so `final_density + 0.0` turns `−0.0` into `+0.0`.

**Interpolation.** `NoiseInterpolator`'s corners are its input's point values at `(cellX·cellWidth, (cellY + cellNoiseMinY)·cellHeight, cellZ·cellWidth)`. While a cell's final density is filled (`CacheAllInCell`, once per cell, for every block of it), each block's value is

```text
lerp3(inX / (double) cellWidth, inY / (double) cellHeight, inZ / (double) cellWidth, n000, n100, n010, n110, n001, n101, n011, n111)
lerp3(dx, dy, dz, …) = lerp(dz, lerp2(dx, dy, n000, n100, n010, n110), lerp2(dx, dy, n001, n101, n011, n111))
lerp2(dx, dy, a, b, c, d) = lerp(dy, lerp(dx, a, b), lerp(dx, c, d))
lerp(t, a, b) = a + t·(b − a)
```

with `nXYZ` the corner at offsets X, Y, Z (0 near, 1 far): x, then y, then z, each `t` a division, in `f64`. The value depends only on the block's offsets in its cell, so columns and chunks agree. Everything above `interpolated` in `final_density` (`squeeze`, `min`, the noodle caves) is then evaluated per block on the interpolated values.

Outside the fill, the interpolator answers incrementally (`updateForY`, `updateForX`, `updateForZ`, with `t = d / (double) size`): y first (four lerps), then x (two), then z (one). Only the ore veins read it that way ([terrain.md](terrain.md#262-columns)). A context other than the `NoiseChunk` itself (a `SinglePointContext` from the aquifer or `find_top_surface`) gets the input's raw, uninterpolated value.

### 26.2 caches

- **`flat_cache`** fills, when the `NoiseChunk` is built, the quart corners `qx` from `firstNoiseX` to `firstNoiseX + noiseSizeXZ` (inclusive, z alike) with the input at `SinglePointContext(4·qx, 0, 4·qz)`. Asked at any context, it takes `qx = x >> 2`, `qz = z >> 2`, and inside that window returns the cached corner value; outside, the input at the point. So inside the window it **quantizes x and z to the quart corner, at y 0**. Every flat-cached function in vanilla 26.2 is y-independent, so only the x and z quantization matters, and only at positions that aren't multiples of 4. The window is blocks `[x0, x0 + 19]` for a chunk, and `[x0, x0 + 4·(cellWidth / 4 + 1) − 1]` for a base column (8 blocks in the Overworld).
- **`cache_2d`** remembers the last `(x, z)`: exact for y-independent inputs, which all of vanilla's are.
- **`cache_once`** returns the last value computed at the same fill or point counter. In vanilla every `cache_once` is under an `interpolated`, evaluated only at corners, so it doesn't change values (not confirmed in the bytecode for every path). A foreign context computes directly.
- **`cache_all_in_cell`**: the final density per cell, above.

Of what terrain reads, only one thing reaches a `flat_cache` at a position that isn't a quart corner: the aquifer's exclusion test at aquifer centres ([terrain.md](terrain.md#262-aquifer)). Corners, climate sampling (quart corners, and caches stripped), surface levels (`x & ~3`, `z & ~3`) and the aquifer's plain noises are exact.

### 26.2 router

`NoiseRouter` has 15 fields: `barrier`, `fluid_level_floodedness`, `fluid_level_spread`, `lava`, `temperature`, `vegetation`, `continents`, `erosion`, `depth`, `ridges`, `preliminary_surface_level`, `final_density`, `vein_toggle`, `vein_ridged`, `vein_gap`. Against 26.3: the first four and `preliminary_surface_level` are 26.3's `aquifers` settings (`surface_level`); 26.3's `exclusion` is code in 26.2 ([terrain.md](terrain.md#262-aquifer)); the `vein_*` fields feed the ore veins, which 26.3 does in the material rule; `chunk_surface_level` doesn't exist.

The noise settings carry `size_horizontal` and `size_vertical` (each 1 to 4): Overworld, Nether and caves 1 and 2 (cells 4 × 8 × 4), End and floating islands 2 and 1 (8 × 4 × 8). `NoiseGeneratorSettings` has `aquifers_enabled` and `ore_veins_enabled` booleans, and `default_block`.

## Testing

Committed tests use hand-written JSON ([ADR 0008](../adr/0008-game-derived-test-values-stay-local.md)): shorthands, sharing, cycles, error paths, codec defaults and validation, each type's values worked out by hand, the compile choices where they're visible (the zero short-circuit, `div` by a constant, `min` and `max` by range through signed zeros and NaN, `pow` by exponent), and column folding against per-cell evaluation. For the volume path: `interpolated`'s point and volume arithmetic worked out from the corners, columns against chunks and stepped volumes against block volumes, the volume-only signed zeros of `min`, `max` and `mul`, y-independent nodes, `find_top_surface`, and remembered against fresh values with points and volumes mixed. Unit tests cover `Interval`'s arithmetic and the spline and blended-noise ranges, and the noise crate covers Perlin's volume path. The ignored tests in `spyglass-density/tests/probe.rs` check every `density.json` case, including the column and chunk digests, in all three dimensions, and that every density function in the bundle loads. In the Nether and the End they bind with legacy randomness, and include the router's inline entries. Functions that can't be bound are skipped and listed; in 26.3 there are none. For 26.3: 19,660 cases, 2,240 of them the Nether's and 2,760 the End's, all equal. Skipping 17,291 draws instead of 17,292 for the End's islands fails 548. For 26.4-snapshot-5, whose routers use `upscale` throughout: the same 19,660 cases, all equal, and all 55 density functions load; weighing `upscale`'s linear y on the volume path as the point path does, instead of the step form, fails 838.

For 26.4: hand-worked `upscale` values on the point path (z, then x, then y weights) and the volume path (each y step from its cell's corner, and x's step form when y is nearest), linear `upscale` against `interpolated` on the same input (corners equal, points, chunks and y-shifted columns not), stepped volumes (planned and fallen back) against block volumes, Catmull-Rom and B-spline along one axis, all-nearest points and volumes, the axis codec's limits, axis plans and coefficient tables, and the Catmull-Rom range. Each switch is pinned by a case that differs between its settings: `clamp_positive_zero`'s `−0` under `min = +0` through each of the compile choices, on both paths, and `spline_nan_is_nan`'s NaN coordinate and `−0` first value.
