# Noise

Status: stable. Implemented in `spyglass-noise` for 26.3; checked against the game by the oracle's [probe files](oracle.md#probe-files).

26.3 reworked noise since 1.21. `ImprovedNoise` is gone, samples are `float`, and the noise JSON has a new shape, so ports of older versions don't apply. Ported from the 26.3 bytecode (`net.minecraft.world.level.levelgen.synth`). Coordinates are `f64`; samples are `f32`. The type of each step below is part of the spec.

## Perlin (`PerlinNoise`, `GradientNoise`)

**Construction** from a [random source](rng.md): `offsetX`, `offsetY`, `offsetZ` = `nextDouble() * 256.0`, in that order; then a permutation table `perms[i] = i` (256 bytes), shuffled with `j = nextInt(256 − i) + i; swap(perms[i], perms[j])` for `i` in `0..256`.

**`wrap(d)`** keeps `f64` precision far from the origin: if `−h ≤ d < h`, where `h = Math.nextDown(2^24)`, return `d`; otherwise `d − floor(d / 2^25 + 0.5) · 2^25`. NaN takes the second branch.

**`get(x, y, z)`**:

1. `x' = wrap(x) + offsetX` (likewise `y`, `z`), in `f64`.
2. `xi = (int) floor(x')`; `xf = (float)(x' − xi)` (likewise `y`, `z`).
3. Corner hashes, with `permute(i) = perms[i & 255] & 255` and wrapping `int` adds: `h0 = permute(xi)`, `h1 = permute(xi + 1)`, `h00 = permute(h0 + yi)`, `h01 = permute(h0 + yi + 1)`, `h10 = permute(h1 + yi)`, `h11 = permute(h1 + yi + 1)`.
4. Corner values, `gradDot(permute(hXY + zi + dz), xf − dx, yf − dy, zf − dz)` for each corner `(dx, dy, dz)`, where `gradDot(h, x, y, z) = gx·x + gy·y + gz·z` in `f32` with `(gx, gy, gz) = GRADIENT[h & 15]`. `GRADIENT` is the 12 cube edges, then `(1,1,0)`, `(0,−1,1)`, `(−1,1,0)`, `(0,−1,−1)`.
5. `smoothstep(t) = t·t·t·(t·(t·6 − 15) + 10)` of `xf`, `yf`, `zf`, then `lerp3` (`lerp(t, a, b) = a + t·(b − a)`, x first, then y, then z), all in `f32`.

**`get(x, z)`** is `get(wrap(x), 0.0, wrap(z))`.

## Smeared Perlin (`SmearedPerlinNoise`)

Perlin noise whose y gradient input is snapped down to a multiple of a scale `s` (`fudgeYScale`), so it smears along y. [Old blended noise](density-functions.md#old_blended_noise) is built from it. It's constructed exactly as Perlin, from the same draws. **`get(x, y, z)`**:

1. As Perlin: wrapped and offset coordinates, `xi`, `yi`, `zi` and `xf`, `zf` as `f32`, but `yd = y' − yi` stays `f64`.
2. `r = y` (the input before wrapping) if `0 ≤ y < yd`, else `yd`; `fudge = (double) floor(r / s + 1.0000000116860974E-7) · s`, where the constant is `1.0E-7f` widened and `floor` is `Mth.floor` (saturating).
3. Perlin's corner values and lerp, with `(float)(yd − fudge)` as the y for the gradient dots and `(float) yd` for the y smoothstep.

## Volumes (`addToVolume`)

Density functions' [volume path](density-functions.md#point-and-volume-evaluation) samples noise over a `DensityVolume` at once: `addToVolume(buffer, volume, xzScale, yScale, factor)` adds `factor · value` to each position's entry, in the volume's order. Not the point path in a loop:

- **Perlin:** for each position, z outermost, then x, then y: `z' = wrap(blockZ · xzScale) + offsetZ`, likewise x, and `y' = wrap(blockY · yScale) + offsetY`, then floors and `f32` fractions as `get` computes them. Each corner's gradient dot is `(gx·xf + gz·zf) + gy·yf` (`Gradient.dotXz`, then the y term), against `get`'s `(gx·xf + gy·yf) + gz·zf`; the corners and `lerp3` are `get`'s. The value added is `factor · lerp3(...)`.
- **Smeared Perlin:** the same, with the gradients' y fraction `(float)(yd − fudge)` and `fudge` from the unwrapped `blockY · yScale`, as `get` computes it from its `y`.
- **Stack:** each layer in order adds its noise with `xzScale · f`, `yScale · f` and `factor · amplitude`: scales and weights are multiplied first, where `get` multiplies the coordinate.

## Noise stack (`NoiseStack`)

A list of layers `(noise, frequency: f64, amplitude: f32)`, all Perlin or all smeared Perlin. `get(x, y, z)` accumulates `sum = sum + amplitude · noise.get(x·f, y·f, z·f)` in `f32`, in layer order, starting from 0. `get(x, z)` does the same with each layer's 2D sample. Rust never contracts `a + b·c` into a fused multiply-add, and neither does Java. Its volume path is [above](#volumes-addtovolume).

## Normal noise (`NormalNoise`)

**Parameters** (`worldgen/noise/*.json`): `base_octave` (required, −32 to 32), `base_amplitude` (default 1.0), `octave_count` (default 1, at most 32), `normalize` (`true` for enabled, `false` for disabled, or the string `"legacy"`; default **enabled**), `amplitude_modifiers` (default empty; up to 32 numbers in `[0, 1000000]`, and if not empty, exactly `octave_count` of them). `base_amplitude` must be in `[9.999999747378752e-6, 1000000]`. No 26.3 noise sets `normalize`.

**Octaves** (`buildOctaves`): `frequency = 2^base_octave`, `amplitude = base_amplitude`, times `2^(count−1) / (2^count − 1)` unless normalization is disabled. For each `i` in `0..octave_count`: the modifier is 1 if the list is empty, else `amplitude_modifiers[i]`. A non-zero modifier adds the octave `(index base_octave + i, frequency, amplitude · modifier)`. Then `frequency *= 2` and `amplitude *= 0.5`. The game computes the powers with `Math.pow(2.0, n)` and `Math.pow(0.5, n)`, which are exact for these integer `n`. spyglass builds them from the exponent bits.

**Normalization factor:** `sum` = the octaves' `|amplitude|` summed with `DoubleStream.sum()`, which uses **Kahan compensation** (`Collectors.sumWithCompensation`; the result is the running sum minus the compensation). `deviation = sqrt(Σ (0.2702247831245211 · |amplitude|)²)`, a plain loop. The factor is 0 if `deviation` is 0, else `sum · (1/3) / (deviation · sqrt(2))`. `legacy` normalization recomputes it differently, [below](#legacy-normalization).

**Range** (`range()`): `±(float)(sum · 0.3333333333333333 · 6.0)`, with the same `sum`. Density functions use it to compile `min` and `max` ([density-functions.md](density-functions.md#ranges)).

**Creation** from a random source (`create`): fork two positional factories, `first` then `second`. For each octave, in order, make `PerlinNoise(first.fromHashOf("octave_" + index))` and `PerlinNoise(second.fromHashOf(same))`, both with amplitude `(float)(factor · octave amplitude)`. The first is at the octave's frequency, the second at `frequency · 1.0181268882175227`. The layers alternate first, second. The result is a noise stack.

**In a world:** `RandomState` seeds a positional factory from the world seed: `XoroshiroRandomSource(seed).forkPositional()` when `legacy_random_source` is false, `LegacyRandomSource(seed).forkPositional()` when it's true ([rng.md](rng.md#legacy-legacyrandomsource)). Each noise is `NormalNoise.create(factory.fromHashOf(noise id))`, with the id in full, e.g. `minecraft:temperature`, except the two Nether biome noises [below](#the-nether-biome-noises). The same `create` serves both factories.

### Legacy normalization

From the 26.3 bytecode (`NormalNoise` constructor). No 26.3 noise uses it, so only hand-worked tests check it. With `normalize: "legacy"` and a non-zero standard factor:

- `parity = base_amplitude · 0.5 · 0.3333333333333333 / (0.1 · (1.0 + 1.0 / (double)(hi − lo + 1)))`, left to right, where `lo` and `hi` are the first and last octave indices `i` in `0..octave_count` with a non-zero modifier (1 when the list is empty), and `hi − lo + 1` is an `int` sum.
- `sum = sum · (parity / factor)`, the division first; then the factor is `parity`.

The range is computed from the adjusted `sum`.

### The Nether biome noises

From the 26.3 bytecode (`RandomState`'s compile context, `NormalNoise.createForLegacyNetherBiome`, `LegacyFbmInitializer`). `minecraft:nether/temperature` and `minecraft:nether/vegetation`, when a density function's `noise` refers to them by id, aren't made by `create`. The key decides, not `legacy_random_source`:

- The source is `LegacyRandomSource(seed + 0)` for temperature and `LegacyRandomSource(seed + 1)` for vegetation (wrapping), with no positional factory.
- `amps` = the modifiers, or `octave_count` ones when the list is empty; `n` = their count; `zero = −base_octave`.
- Two stacks, `first` then `second`, are built from the **same** source in turn. Each:
  1. Construct a `PerlinNoise` from the source: always, as the zero octave. Keep it as layer `zero` only if `0 ≤ zero < n` and `amps[zero] ≠ 0`.
  2. For `i` from `zero − 1` down to 0: if `i < n` and `amps[i] ≠ 0`, construct layer `i`'s `PerlinNoise`; otherwise skip exactly 262 draws (`consumeCount(262)`), not a constructed and discarded noise, which can take more.
  3. `zero < n − 1` (a positive octave) is an error.
  4. Layers in index order, from `freq = 2^−zero` and `val = 2^(n−1) / (2^n − 1)`: each kept layer at frequency `freq` with amplitude `(float)(val · amps[i])`; then `freq *= 2`, `val /= 2`.
- `amp = (float)(factor · base_amplitude)`, with the standard normalization factor of the noise's parameters.
- The noise is one stack: `first`'s layers at frequency `f · 1.0` and amplitude `a · amp` (in `f32`), then `second`'s at `f · 1.0181268882175227` and `a · amp`. They aren't interleaved, unlike `create`'s.

Its range is the parameters' `range()` as usual. For 26.3's two noises (`base_octave` −7, two octaves, `base_amplitude` 0.9494731054427981), each stack builds and drops the zero octave, skips 5 × 262 draws, then builds layers 1 and 0.

## Simplex (`SimplexNoise`)

From the 26.3 bytecode. The End's outer islands use it ([density-functions.md](density-functions.md#end_outer_islands)). In 26.3 it's a `GradientNoise`, like Perlin, and returns `f32`.

**Construction** from a random source, with an offset scale `S` (256, or 0 when made with zero offsets, as the End's is): `offsetX`, `offsetY` and `offsetZ` are each `nextDouble() · S`, drawn in that order even when `S` is 0 (they're then `+0.0`); then the permutation, exactly as Perlin's. `permute(i) = perms[i & 255] & 255`.

**Gradients** are Perlin's table; simplex uses only the first 12 (`g % 12`). `dot(g, x, y, z) = (gx·x + gy·y) + gz·z` in `f64`, the z term always added.

**Corner** `corner(g, x, y, z, base)`: `t = ((base − x·x) − y·y) − z·z`; 0 if `t < 0` (NaN goes on), else `t = t·t` and the result is `(t·t) · dot(g, x, y, z)`.

**2D** `get(x, y)`, all `f64` unless said, with `F2 = 0.5 · (sqrt(3) − 1)` and `G2 = (3 − sqrt(3)) / 6` (`Math.sqrt`, computed once):

1. `xx = x + offsetX`, `yy = y + offsetY`; `s = (xx + yy) · F2`; `i = floor(xx + s)`, `j = floor(yy + s)` (`Mth.floor`); `t = (double)(i + j) · G2`, the sum in wrapping `i32`.
2. `x0 = xx − ((double)i − t)`, `y0 = yy − ((double)j − t)`. `(i1, j1)` is `(1, 0)` if `x0 > y0`, else `(0, 1)`.
3. `x1 = (x0 − i1) + G2`, `y1 = (y0 − j1) + G2`; `x2 = (x0 − 1) + 2·G2`, `y2 = (y0 − 1) + 2·G2`.
4. With `ii = i & 255`, `jj = j & 255`: gradients `permute(ii + permute(jj)) % 12`, `permute(ii + i1 + permute(jj + j1)) % 12`, `permute(ii + 1 + permute(jj + 1)) % 12`.
5. The result is `(float)(70 · ((n0 + n1) + n2))`, each `n` a corner with z 0 and base 0.5.

3D isn't used by vanilla worldgen and isn't ported.

## Parity descriptions

The game describes a noise's setup with `parityConfigString`: per Perlin octave, `xo=%.3f, yo=%.3f, zo=%.3f, p0=%d, p255=%d` (the offsets as `float`, `perms[0]` and `perms[255]` as signed bytes). For a normal noise, the first stack's octaves come in order, with `null` for skipped ones, then the second stack's. The probe records them. Comparing them to spyglass's layers separates a construction bug from a sampling bug.

## Testing

Committed unit tests cover `wrap`, the permutation, octave layout, the compensated sum, smeared Perlin's fudge and random use, and the `Mth` helpers ([ADR 0008](../adr/0008-game-derived-test-values-stay-local.md)). The ignored test `spyglass-noise/tests/probe.rs` reads each climate noise's parameters from the bundle, rebuilds it for every golden seed, and compares every `noise.json` sample bit for bit, plus the parity descriptions. Smeared Perlin is checked through old blended noise, in the density probe.
