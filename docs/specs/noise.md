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

**Normalization factor:** `sum` = the octaves' `|amplitude|` summed with `DoubleStream.sum()`, which uses **Kahan compensation** (`Collectors.sumWithCompensation`; the result is the running sum minus the compensation). `deviation = sqrt(Σ (0.2702247831245211 · |amplitude|)²)`, a plain loop. The factor is 0 if `deviation` is 0, else `sum · (1/3) / (deviation · sqrt(2))`. `legacy` normalization recomputes it differently. It isn't ported yet, and spyglass refuses it with an error.

**Range** (`range()`): `±(float)(sum · 0.3333333333333333 · 6.0)`, with the same `sum`. Density functions use it to compile `min` and `max` ([density-functions.md](density-functions.md#ranges)).

**Creation** from a random source (`create`): fork two positional factories, `first` then `second`. For each octave, in order, make `PerlinNoise(first.fromHashOf("octave_" + index))` and `PerlinNoise(second.fromHashOf(same))`, both with amplitude `(float)(factor · octave amplitude)`. The first is at the octave's frequency, the second at `frequency · 1.0181268882175227`. The layers alternate first, second. The result is a noise stack.

**In a world:** `RandomState` seeds a positional factory from the world seed (`XoroshiroRandomSource(seed).forkPositional()` when `legacy_random_source` is false). Each noise is `NormalNoise.create(factory.fromHashOf(noise id))`, with the id in full, e.g. `minecraft:temperature`.

## Parity descriptions

The game describes a noise's setup with `parityConfigString`: per Perlin octave, `xo=%.3f, yo=%.3f, zo=%.3f, p0=%d, p255=%d` (the offsets as `float`, `perms[0]` and `perms[255]` as signed bytes). For a normal noise, the first stack's octaves come in order, with `null` for skipped ones, then the second stack's. The probe records them. Comparing them to spyglass's layers separates a construction bug from a sampling bug.

## Testing

Committed unit tests cover `wrap`, the permutation, octave layout, the compensated sum, smeared Perlin's fudge and random use, and the `Mth` helpers ([ADR 0008](../adr/0008-game-derived-test-values-stay-local.md)). The ignored test `spyglass-noise/tests/probe.rs` reads each climate noise's parameters from the bundle, rebuilds it for every golden seed, and compares every `noise.json` sample bit for bit, plus the parity descriptions. Smeared Perlin is checked through old blended noise, in the density probe.
