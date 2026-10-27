# Noise

Status: stable. Implemented in `spyglass-noise` for 26.3; checked against the game by the oracle's [probe files](oracle.md#probe-files). The 26.4 snapshots are implemented and checked against the oracle; 26.2's noise ([26.2](#262)) is implemented in `spyglass_noise::v1_21` and checked against its probe files.

26.3 reworked noise since 1.21. `ImprovedNoise` is gone, samples are `float`, and the noise JSON has a new shape, so ports of older versions don't apply. Ported from the 26.3 bytecode (`net.minecraft.world.level.levelgen.synth`). Coordinates are `f64`; samples are `f32`. The type of each step below is part of the spec.

_Versions:_ every `synth` class in 26.4-snapshot-5 is byte-identical to 26.3's, so this spec holds for it unchanged. 26.2 still has the 1.18–1.21 noise, all in `f64`; [26.2](#262) describes it in full. Its draws and seeds are 26.3's; its arithmetic and its noise JSON aren't. (26.4-snapshot-5 also changes `Mth.sin` and `Mth.cos`, which only carvers and features use; spyglass doesn't port them.)

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

_Versions:_ 26.2's Perlin is `ImprovedNoise`: the same construction, hashing and corner order, but fractions, gradient dots, `smoothstep` and `lerp3` all in `f64`, and the caller wraps the coordinates ([26.2](#262-improved-noise-improvednoise)).

## Smeared Perlin (`SmearedPerlinNoise`)

Perlin noise whose y gradient input is snapped down to a multiple of a scale `s` (`fudgeYScale`), so it smears along y. [Old blended noise](density-functions.md#old_blended_noise) is built from it. It's constructed exactly as Perlin, from the same draws. **`get(x, y, z)`**:

1. As Perlin: wrapped and offset coordinates, `xi`, `yi`, `zi` and `xf`, `zf` as `f32`, but `yd = y' − yi` stays `f64`.
2. `r = y` (the input before wrapping) if `0 ≤ y < yd`, else `yd`; `fudge = (double) floor(r / s + 1.0000000116860974E-7) · s`, where the constant is `1.0E-7f` widened and `floor` is `Mth.floor` (saturating).
3. Perlin's corner values and lerp, with `(float)(yd − fudge)` as the y for the gradient dots and `(float) yd` for the y smoothstep.

_Versions:_ in 26.2, smeared Perlin is `ImprovedNoise.noise` with a non-zero y scale, in `f64`; the unwrapped y arrives as a separate `yMax` argument ([26.2](#262-improved-noise-improvednoise)).

## Volumes (`addToVolume`)

Density functions' [volume path](density-functions.md#point-and-volume-evaluation) samples noise over a `DensityVolume` at once: `addToVolume(buffer, volume, xzScale, yScale, factor)` adds `factor · value` to each position's entry, in the volume's order. Not the point path in a loop:

- **Perlin:** for each position, z outermost, then x, then y: `z' = wrap(blockZ · xzScale) + offsetZ`, likewise x, and `y' = wrap(blockY · yScale) + offsetY`, then floors and `f32` fractions as `get` computes them. Each corner's gradient dot is `(gx·xf + gz·zf) + gy·yf` (`Gradient.dotXz`, then the y term), against `get`'s `(gx·xf + gy·yf) + gz·zf`; the corners and `lerp3` are `get`'s. The value added is `factor · lerp3(...)`.
- **Smeared Perlin:** the same, with the gradients' y fraction `(float)(yd − fudge)` and `fudge` from the unwrapped `blockY · yScale`, as `get` computes it from its `y`.
- **Stack:** each layer in order adds its noise with `xzScale · f`, `yScale · f` and `factor · amplitude`: scales and weights are multiplied first, where `get` multiplies the coordinate.

_Versions:_ 26.2's noise classes have no volume path. Every sample is a point sample.

## Noise stack (`NoiseStack`)

A list of layers `(noise, frequency: f64, amplitude: f32)`, all Perlin or all smeared Perlin. `get(x, y, z)` accumulates `sum = sum + amplitude · noise.get(x·f, y·f, z·f)` in `f32`, in layer order, starting from 0. `get(x, z)` does the same with each layer's 2D sample. Rust never contracts `a + b·c` into a fused multiply-add, and neither does Java. Its volume path is [above](#volumes-addtovolume).

_Versions:_ 26.2 has no `NoiseStack`. Its octave set is `PerlinNoise`, an `f64` sum with per-octave weights computed as it goes ([26.2](#262-octave-set-perlinnoise)).

## Normal noise (`NormalNoise`)

**Parameters** (`worldgen/noise/*.json`): `base_octave` (required, −32 to 32), `base_amplitude` (default 1.0), `octave_count` (default 1, at most 32), `normalize` (`true` for enabled, `false` for disabled, or the string `"legacy"`; default **enabled**), `amplitude_modifiers` (default empty; up to 32 numbers in `[0, 1000000]`, and if not empty, exactly `octave_count` of them). `base_amplitude` must be in `[9.999999747378752e-6, 1000000]`. No 26.3 noise sets `normalize`.

_Versions:_ 26.2's noise JSON is `{"firstOctave": int, "amplitudes": [double…]}`, and its normal noise has a different value factor and no `f32` amplitudes ([26.2](#262-normal-noise-normalnoise)). Octaves, normalization, range and creation below are 26.3's only; the seeding in [In a world](#normal-noise-normalnoise) is the same in 26.2.

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

_Versions:_ 26.2 makes these two noises with `NormalNoise.createLegacyNetherBiome`, from the same sources, keyed the same way, and with the same draws (steps 1–3, with `amps` = the `amplitudes` list). Its values follow [26.2's normal noise](#262-normal-noise-normalnoise): the amplitudes as they are, no `base_amplitude`, and the two sets summed separately.

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

_Versions:_ 26.2's 2D simplex returns the `f64` `70 · ((n0 + n1) + n2)`, not narrowed, and doesn't add the offsets ([26.2](#262-simplex-simplexnoise)).

## Parity descriptions

The game describes a noise's setup with `parityConfigString`: per Perlin octave, `xo=%.3f, yo=%.3f, zo=%.3f, p0=%d, p255=%d` (the offsets as `float`, `perms[0]` and `perms[255]` as signed bytes). For a normal noise, the first stack's octaves come in order, with `null` for skipped ones, then the second stack's. The probe records them. Comparing them to spyglass's layers separates a construction bug from a sampling bug.

_Versions:_ 26.2 formats each octave the same way (`NoiseUtils` is byte-identical in all three versions), but wraps them differently: per set, the first octave, the amplitudes (`%.2f`) and every level index, lowest frequency first, with `null` for a zero amplitude; the normal noise lists `first`, then `second`.

## 26.2

From the 26.2 bytecode (`net.minecraft.world.level.levelgen.synth`). 26.2 has the 1.18–1.21 noise: `ImprovedNoise`, the octave set `PerlinNoise`, `NormalNoise`, `SimplexNoise` and `BlendedNoise`. Every step is `f64`, samples included, and nothing is narrowed to `f32`. Construction draws, seeding and the gradient table are 26.3's. `Mth.smoothstep` and `Mth.lerp3` are the `double` overloads: `smoothstep(t) = ((t·t)·t) · ((t·((t·6.0) − 15.0)) + 10.0)`, and `lerp3` as in [Perlin](#perlin-perlinnoise-gradientnoise).

### 26.2 improved noise (`ImprovedNoise`)

Perlin and smeared Perlin in one class. **Construction** is [Perlin's](#perlin-perlinnoise-gradientnoise), with the same draws and table: `xo`, `yo`, `zo`, then `j = nextInt(256 − i)` and a swap of `p[i]` and `p[i + j]`. `p(i) = p[i & 255] & 255`.

**`noise(x, y, z, yScale, yMax)`** doesn't wrap; callers do:

1. `xx = x + xo` (likewise `y`, `z`); `xi = Mth.floor(xx)`; `xf = xx − (double) xi`, kept in `f64`.
2. `fudge` is 0 if `yScale == 0`. Otherwise (NaN counts as non-zero) `r = yMax` if `yMax ≥ 0 && yMax < yf`, else `yf`; `fudge = (double) Mth.floor(r / yScale + 1.0000000116860974E-7) · yScale`.
3. Perlin's corner hashes. Each corner's value is `gradDot(h, xf − dx, (yf − fudge) − dy, zf − dz)`, with `gradDot = ((double) gx·x + (double) gy·y) + (double) gz·z` in `f64`.
4. `smoothstep` of `xf`, `yf` (without the fudge) and `zf`, then `lerp3`, in `f64`.

`noise(x, y, z)` is `noise(x, y, z, 0.0, 0.0)`: plain Perlin. `noiseWithDerivative` has no worldgen caller and isn't described.

### 26.2 octave set (`PerlinNoise`)

An octave set has a `firstOctave`, a list of `n` `f64` `amplitudes` and `n` levels: level `i` is octave `firstOctave + i`, lowest frequency first, and is empty when `amplitudes[i]` is zero (`-0.0` included; NaN counts as non-zero). Zero entries still count in `n`.

**Construction** (`new` initialization, used by `NormalNoise.create`): `factory = source.forkPositional()`; for each `i` in `0..n` with a non-zero amplitude, level `i` is `ImprovedNoise(factory.fromHashOf("octave_" + (firstOctave + i)))`, the octave in signed decimal. There is no check against positive octaves.

**Legacy construction** (for the Nether biome noises and `BlendedNoise`): steps 1–3 of [the Nether biome noises](#the-nether-biome-noises), with `amps` = `amplitudes`, `zero = −firstOctave` and `ImprovedNoise` for `PerlinNoise`. It also fails if the number of levels built differs from the number of non-zero amplitudes.

**Factors**: `inF0 = Math.pow(2.0, (double)(−zero))`, i.e. `2^firstOctave`; `valF0 = Math.pow(2.0, (double)(n − 1)) / (Math.pow(2.0, (double) n) − 1.0)`.

**`wrap(d)`** is `d − (double) Mth.lfloor(d / 3.3554432E7 + 0.5) · 3.3554432E7`, with no fast path. It returns `d` itself wherever 26.3's fast path does. Far out the two differ: `lfloor` saturates where 26.3 floors in `f64`, so once `|d / 2^25 + 0.5|` reaches 2^63 (`|d|` about 2^88) 26.2 subtracts `±2^63 · 2^25` instead, and it keeps `±∞` where 26.3 gives NaN. No worldgen coordinate comes near.

**`getValue(x, y, z, yScale, yMax)`**:

```text
sum = 0.0; inF = inF0; valF = valF0                       // f64
for i in 0..n:
    if level i exists:
        v = level.noise(wrap(x·inF), wrap(y·inF), wrap(z·inF), yScale·inF, yMax·inF)
        sum = sum + (amplitudes[i] · v) · valF
    inF = inF · 2.0
    valF = valF / 2.0
return sum
```

`getValue(x, y, z)` passes `yScale = yMax = 0`. `getOctaveNoise(i)` is level `n − 1 − i`, so 0 is the highest frequency. `maxValue` is the same sum with `2.0` for each noise value (`edgeValue(2.0)`); the constructor computes it, and it serves density-function ranges, not sampling. `maxBrokenValue(y)` is `edgeValue(y + 2.0)`, where `edgeValue(m)` sums `(amplitudes[i] · m) · valF` over the non-empty levels with `valF` halving per level, empty ones included.

### 26.2 normal noise (`NormalNoise`)

**Parameters** (`worldgen/noise/*.json`, codec `NormalNoise$NoiseParameters`): `firstOctave` (required `int`) and `amplitudes` (required list of `double`), with no range checks. A file with `firstOctave` and no `base_octave` is this form; anything else is read as 26.3's. Parameters in this form only bind to 26.2's noise: 26.3's normal noise refuses them.

**Creation**: `create` builds `first`, then `second`, each a new-initialization octave set from the same `firstOctave` and `amplitudes`; each calls `forkPositional()` on the source in turn, so the octave seeds are 26.3's. `createLegacyNetherBiome` builds both with legacy construction from the same source.

**Value factor**, in the constructor:

```text
lo = Integer.MAX_VALUE; hi = Integer.MIN_VALUE
for each index i with amplitudes[i] != 0.0: lo = min(lo, i); hi = max(hi, i)
k = hi − lo                                               // wrapping int
valueFactor = 0.16666666666666666 / (0.1 · (1.0 + 1.0 / (double)(k + 1)))
```

The factor depends only on the span of non-zero indices, not on the amplitudes' sizes. With every amplitude zero, `k = MIN_VALUE − MAX_VALUE = 1` (wrapping), so the factor is `0.16666666666666666 / 0.15000000000000002`; the noise is 0 anyway, with no levels. The amplitudes weight octaves only in the octave sets' sums. It equals 26.3's [legacy normalization](#legacy-normalization) with `base_amplitude` 1 (`0.5 · 0.3333333333333333` is `0.16666666666666666`), but 26.3 folds it into `f32` amplitudes, so even `legacy` isn't bit-identical.

26.3's data carries these weights over: for every noise in both versions' data, `base_octave` is `firstOctave`, the modifiers are the amplitudes, and the stored `base_amplitude` makes 26.3's standard normalized layer weight (`normalizationFactor · octave amplitude`, before narrowing) equal 26.2's `valueFactor · amplitudes[i] · valF0 / 2^i` to within rounding. The ignored test `spyglass-noise/tests/versions.rs` checks it on the two bundles: 63 noises, within 5·10⁻¹⁶ relative, every weight the same `f32`. The values still differ, since 26.2 never narrows.

**`getValue(x, y, z)`**:

```text
a = first.getValue(x, y, z)
b = second.getValue(x · 1.0181268882175227, y · 1.0181268882175227, z · 1.0181268882175227)
return (a + b) · valueFactor                              // f64
```

So each octave's weight is `amplitudes[i] · valF0 / 2^i`, applied inside its set, and the two sets are summed separately: no interleaving and no per-layer amplitude. `maxValue = (first.maxValue + second.maxValue) · valueFactor`.

### 26.2 simplex (`SimplexNoise`)

**Construction**: `xo`, `yo`, `zo` = `nextDouble() · 256.0`, then the permutation as Perlin's. These are the draws of 26.3's simplex with offset scale 256 (`int[512] p`, of which only `p[0..256]` is used).

**2D `getValue(x, y)`** is 26.3's [2D `get`](#simplex-simplexnoise) operation by operation, except that it **doesn't add** `xo` and `yo` (26.3's End noise is made with zero offsets to match), and it returns the `f64` `70.0 · ((n0 + n1) + n2)`, not narrowed. 26.3's `x + 0.0` turns a `−0.0` input into `+0.0`, which can only change the sign of an exactly zero result.

**3D `getValue(x, y, z)`** doesn't add the offsets either, and is all `f64`, with the constants `F3 = 0.3333333333333333` and `G3 = 0.16666666666666666` as the bytecode loads them:

1. `s = ((x + y) + z) · F3`; `i`, `j`, `k` = `Mth.floor` of `x + s`, `y + s`, `z + s`; `t = (double)((i + j) + k) · G3`, the sums in wrapping `i32`; `x0 = x − ((double)i − t)`, likewise `y0`, `z0`.
2. The middle corners `(i1, j1, k1)` and `(i2, j2, k2)`: if `x0 ≥ y0`, then `(1,0,0, 1,1,0)` if `y0 ≥ z0`, else `(1,0,0, 1,0,1)` if `x0 ≥ z0`, else `(0,0,1, 1,0,1)`; otherwise `(0,0,1, 0,1,1)` if `y0 < z0`, else `(0,1,0, 0,1,1)` if `x0 < z0`, else `(0,1,0, 1,1,0)`. Every comparison is false for NaN.
3. `x1 = (x0 − i1) + G3`, `x2 = (x0 − i2) + F3`, `x3 = (x0 − 1.0) + 0.5`, likewise y and z.
4. With `ii = i & 255` (likewise `jj`, `kk`), corner `c`'s gradient is `p(ii + di + p(jj + dj + p(kk + dk))) % 12` for its offsets `(di, dj, dk)`: `(0,0,0)`, the two middle corners, `(1,1,1)`.
5. The result is `32.0 · (((n0 + n1) + n2) + n3)`, each corner with base 0.6.

No worldgen in spyglass's scope calls it; it's ported for completeness and checked by the probe.

The End's islands use it through `DensityFunctions$EndIslandDensityFunction`, which compares the `f64` sample against `(double) −0.9f` ([density-functions.md](density-functions.md#end_outer_islands)).

### 26.2 blended noise (`BlendedNoise`)

26.2's [old blended noise](density-functions.md#old_blended_noise), in `f64`.

**Construction** from a source `r`: `minLimit` = `PerlinNoise.createLegacyForBlendedNoise(r, −15..0)`, then `maxLimit` the same, then `main` with `−7..0`. Each is a legacy-constructed octave set with every amplitude 1, so octave 0 is built first, then −1 down to −15 (or −7), none skipped. 26.3 draws in the same order (not confirmed in the bytecode: inferred from the same call order). The source is 26.3's: `LegacyRandomSource(seed)` when `legacy_random_source` is true, else `factory.fromHashOf("minecraft:terrain")`.

Fields: `xzMul = 684.412 · xz_scale`, `yMul = 684.412 · y_scale`, and `xz_factor`, `y_factor`, `smear_scale_multiplier` from data.

**`compute`** at block `(bx, by, bz)`, with `octave(set, i)` = `set.getOctaveNoise(i)` and `wrap` the octave set's:

```text
x = (double) bx · xzMul; y = (double) by · yMul; z = (double) bz · xzMul
mx = x / xz_factor; my = y / y_factor; mz = z / xz_factor
smear = yMul · smear_scale_multiplier
mainSmear = smear / y_factor
main = 0; f = 1.0
for i in 0..8:
    if octave(main, i) exists:
        main = main + octave(main, i).noise(wrap(mx·f), wrap(my·f), wrap(mz·f), mainSmear·f, my·f) / f
    f = f / 2.0
t = (main / 10.0 + 1.0) / 2.0
min = 0; max = 0; f = 1.0
for i in 0..16:
    wx = wrap(x·f); wy = wrap(y·f); wz = wrap(z·f)
    if octave(minLimit, i) exists: min = min + octave(minLimit, i).noise(wx, wy, wz, smear·f, y·f) / f
    if octave(maxLimit, i) exists: max = max + octave(maxLimit, i).noise(wx, wy, wz, smear·f, y·f) / f
    f = f / 2.0
return Mth.clampedLerp(t, min / 512.0, max / 512.0) / 128.0
```

`Mth.clampedLerp(t, a, b)` is `a` if `t < 0`, `b` if `t > 1`, else `a + t·(b − a)`. The game skips the `min` sum when `t ≥ 1` and the `max` sum when `t ≤ 0` (NaN skips neither). A port must skip too: `clampedLerp` ignores the skipped side except at the ends, but at `t = 1` exactly it lerps, reading the skipped `min` as 0 (`0 + 1·(b − 0)` is `b`, where `a + 1·(b − a)` can round off it), and at `t = 0` a skipped `max` can change the sign of a zero. The smear's `yMax` is the unwrapped `y·f` (`my·f` for `main`).

## Testing

Committed unit tests cover `wrap`, the permutation, octave layout, the compensated sum, smeared Perlin's fudge and random use, and the `Mth` helpers ([ADR 0008](../adr/0008-game-derived-test-values-stay-local.md)). The ignored test `spyglass-noise/tests/probe.rs` reads each climate noise's parameters from the bundle, rebuilds it for every golden seed, and compares every `noise.json` sample bit for bit, plus the parity descriptions. Smeared Perlin is checked through old blended noise, in the density probe.

26.2's primitives (`spyglass_noise::v1_21`) have hand-worked unit tests: `ImprovedNoise` on an identity permutation, smeared and not; octave-set construction (new and legacy draws), weights and `edgeValue`; the value factor for amplitude lists with leading, trailing, inner and all zeros; simplex in 2D and 3D; and blended noise's draws and formula. The same probe test reads 26.2's `noise.json`, whose cases name each primitive (`improved_noise`, `perlin_noise` with kinds `new` and `legacy`, `normal_noise` with kinds `create` and `legacy_nether_biome`, `simplex_noise`, `blended_noise`) and hold `f64` values; the bundle's noise form picks the normal noise family.
