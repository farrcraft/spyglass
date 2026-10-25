# m02. RNG and noise

- Status: In progress
- Milestone: [02. RNG, noise and biomes](../roadmap/02-rng-noise-biomes.md)
- Started: 2026-10-25
- Completed: —

## Goal

Bit-exact ports of the game's random sources and noise primitives in `spyglass-rng` and `spyglass-noise`, checked against values the real 26.3 game produces. Covers the milestone's second exit criterion and the RNG and noise parts of the third.

This is the first of three plans for milestone 02. [m02-density](m02-density.md) builds the climate density functions on top of it, and [m02-biomes](m02-biomes.md) the biome lookup and the full golden comparison.

## Facts established up front

Checked against the unpacked 26.3 server jar with `javap` on 2026-10-25. **26.3's noise is not 1.21's.** Ports and prior art written for 1.18–1.21 (cubiomes and others) don't match it, so everything here is ported from the 26.3 classes.

- **RNG classes** (`net.minecraft.world.level.levelgen`): `Xoroshiro128PlusPlus` (`seedLo`, `seedHi`, `nextLong`), `XoroshiroRandomSource` (`nextInt`, `nextInt(int)`, `nextLong`, `nextBoolean`, `nextFloat`, `nextDouble`, `nextGaussian`, `consumeCount`, `fork`, `forkPositional`; `nextBits` underneath), `XoroshiroRandomSource$XoroshiroPositionalRandomFactory` (`at(x, y, z)`, `fromHashOf(String)`, `fromSeed`), `LegacyRandomSource` (the Java LCG: `next(int)`, `fork`, `forkPositional`, with `LegacyPositionalRandomFactory`), `MarsagliaPolarGaussian`, and `RandomSupport` (`mixStafford13`, `upgradeSeedTo128bit`, `upgradeSeedTo128bitUnmixed`, `seedFromHashOf(String)` via Guava's MD5, `GOLDEN_RATIO_64`, `SILVER_RATIO_64`). `RandomSupport$Seed128bit` has `xor` and `mixed`.
- **World randomness.** `RandomState` builds `random = algorithm.newInstance(seed).forkPositional()`, with the algorithm chosen by `legacy_random_source` (`false` for the overworld, so Xoroshiro). A noise is created by `Noises.instantiate`: `random.fromHashOf(key.identifier())`, then `NormalNoise.create(that)`.
- **Noise classes** (`levelgen.synth`): the old `ImprovedNoise`/`PerlinNoise`(octaves) pair is gone. Now:
  - `GradientNoise` (abstract): a `byte[] perms` table, `offsetX/Y/Z`, `permute`, `permuteToGrad`, `gradDot(int, float, float, float)` returning **`float`**, and `wrap(double)` with `ROUND_OFF = 33554432`, `HALF_ROUND_OFF`, `Math.floor` and `Math.nextDown`;
  - `PerlinNoise extends GradientNoise`: `get(x, y, z)` and `get(x, z)` return **`float`**; `sampleAndLerp(int, int, int, float, float, float, float)`, `Mth.floor(double)` and `Mth.smoothstep(float)`; also `noiseWithDerivative`;
  - `SmearedPerlinNoise extends PerlinNoise` (`fudgeYScale`, `computeFudgeY`), `SimplexNoise`, and `BlendedNoise` (`old_blended_noise`, not needed for climate);
  - `NoiseStack` with `Layer(noise, double frequency, float amplitude)` and subclasses `NoiseStack$Perlin` and `NoiseStack$SmearedPerlin`, built through `NoiseStack$Builder`;
  - `NormalNoise`: parameters `baseAmplitude`, `baseOctave`, `octaveCount`, `normalize` (`NormalNoise$Normalization`: `DISABLED`, `ENABLED`, `LEGACY`) and `amplitudeModifiers`. `create(RandomSource)` forks two positional factories, builds a `PerlinNoise` per octave from `fromHashOf(octaveInfo.seed())` where the seed string is `"octave_" + octaveIndex`, and adds the second set at `INPUT_FACTOR = 1.0181268882175227`. Construction uses `buildOctaves`, `getAmplitudeModifier`, `computeNormalizationFactor` and `estimateDeviation`. Constants: `TARGET_DEVIATION = 1/3`, `PERSISTENCE = 0.5`, `LACUNARITY = 2.0`, `MAX_OCTAVE_COUNT = 32`.
- **Noise JSON shape** (`worldgen/noise/*.json`, 63 files in 26.3) matches `NormalNoise$Parameters`: `{"base_amplitude": 1.2453007926713473, "base_octave": -10, "octave_count": 6, "amplitude_modifiers": [1.5, 0.0, 1.0, 0.0, 0.0, 0.0]}` for `temperature`. No file sets `normalize`, so the codec's default applies.
- **Transcendental functions.** In the climate path's noise code the only `java.lang.Math` calls are `floor`, `nextDown`, `min`, `max`, `sqrt` (correctly rounded in both Java and Rust) and `pow`, and every `pow` is `pow(2.0, n)` or `pow(0.5, n)` for an integer `n`: an exact power of two. So no libm-dependent function is needed yet.
- **Java semantics that matter here:** `float` and `double` arithmetic is strict IEEE 754 in Java 17+, as in Rust, and Rust never contracts to FMA on its own. `(int)` and `(long)` casts of floating-point values saturate and send NaN to 0 in both languages. Integer arithmetic wraps in Java, so Rust uses `wrapping_*` and `i32`/`i64` explicitly. `>>` vs `>>>` maps to signed vs unsigned shifts.
- **The game can describe its own noise.** `NormalNoise.parityConfigString`, `PerlinNoise.parityConfigString`, `XoroshiroPositionalRandomFactory.parityConfigString` and `NoiseUtils.parityNoiseOctaveConfigString` print a noise's octave layout, offsets and permutation table. That's a cheap structural check before comparing values.

## Approach

1. **Values from the game.** The golden files hold biomes only, so the oracle gains a `probe` subcommand that writes the game's raw values as JSON into `oracle-out/<version>/probe/`. For this plan:
   - `rng.json`: for a few seeds (including `0`, `-1`, `i64::MIN`, `i64::MAX`), sequences from `XoroshiroRandomSource` and `LegacyRandomSource` (`nextLong`, `nextInt`, `nextInt(bound)` for awkward bounds, `nextFloat`, `nextDouble`, `nextGaussian`, `nextBoolean`), `fork` and `forkPositional` results, `at(x, y, z)` at small, negative and border coordinates, `fromHashOf` for real noise ids, `seedFromHashOf`, `mixStafford13`, `upgradeSeedTo128bit`;
   - `noise.json`: for each of the 6 climate noises (`temperature`, `vegetation`, `continentalness`, `erosion`, `ridge`, `offset`) and each seed in the golden set, the noise's `parityConfigString` and `get(x, y, z)` at a fixed point list (origin, negative, ±1,000,000, the world border, non-integer coordinates, and the actual scaled inputs the climate functions use). Noises come from `RandomState.getOrCreateNoise`, as the game creates them.

   Floats and doubles are written as raw bit patterns (hex), so nothing depends on decimal parsing. The format goes in `docs/specs/oracle.md`.

2. **`spyglass-rng`.** `Xoroshiro128PlusPlus`, `XoroshiroRandom`, `LegacyRandom` (Java LCG), a `RandomSource` trait for what both share, `PositionalRandomFactory` for both kinds, `Seed128` with `mix_stafford13`, `upgrade_seed_to_128bit` and `seed_from_hash_of` (MD5), and the Marsaglia polar Gaussian. Names follow the game's, with the game class and method cited in doc comments. No dependencies except MD5 (task below).
3. **`spyglass-noise`.** `GradientNoise` state and `PerlinNoise` (`f32` results, `f64` coordinates, `wrap` exactly as the game does it), `NoiseStack`, and `NormalNoise` built from a `NormalNoiseParameters` struct (plain Rust, no JSON: the noise crate doesn't depend on `spyglass-data`). `SmearedPerlinNoise`, `SimplexNoise` and `BlendedNoise` are left for the terrain and End work; the plan only ports what climate needs, plus whatever `NormalNoise` construction requires.
4. **Tests at two levels.**
   - Committed unit tests with values from sources that aren't Mojang's: `java.util.Random` sequences (the JDK), Xoroshiro128++ against the reference C implementation, MD5 against RFC 1321's test suite, SplitMix/Stafford mixing against published constants.
   - Ignored tests that read `oracle-out/<version>/probe/` and compare every value bit for bit, like the existing golden-file test. No game-derived values are committed ([ADR 0008](../adr/0008-game-derived-test-values-stay-local.md)).

## Tasks

- [x] ADR 0008: game-derived test values stay local (decided 2026-10-25). Either every value from the game stays in locally generated probe files (ignored tests, as with golden files), or a spec allows a small committed set (say, a few dozen noise samples) so `cargo test` checks noise without the oracle. CLAUDE.md currently says generated, not committed, unless a spec says otherwise.
- [x] ADR 0009: MD5 from the `md-5` crate (decided 2026-10-25). Either add the RustCrypto `md-5` crate (MIT or Apache-2.0, both already allowed in `deny.toml`), keeping `spyglass-rng` otherwise dependency-free, or hand-write MD5 (about 100 lines, tested against RFC 1321). It's `spyglass-rng`'s first dependency, so it gets a record.
- [x] Oracle `probe` subcommand, RNG and noise sections (`oracle/src/spyglass/oracle/Probe.java`), plus `cargo xtask oracle probe` and inclusion in `cargo xtask oracle golden`. Spec the output in `docs/specs/oracle.md`.
- [x] Confirm by `javap -c` the details the spec must pin down (all in `docs/specs/rng.md` and `noise.md`): the Normalization codec default when `normalize` is absent; `nextInt(bound)` and `nextFloat`/`nextDouble` bit counts; `at(x, y, z)` hashing (`Mth.getSeed`); `LegacyPositionalRandomFactory`; and how `PerlinNoise`'s constructor consumes the random (offsets, then the permutation shuffle).
- [x] `spyglass-rng`: Xoroshiro128++, `XoroshiroRandom`, positional factory, `Seed128`, MD5 name seeding. Unit tests against reference vectors (the JDK's own `jdk.internal.random.Xoroshiro128PlusPlus`, RFC 1321, SplitMix64).
- [x] `spyglass-rng`: Java LCG (`LegacyRandom`) and its positional factory, and the Marsaglia polar Gaussian. Unit tests against `java.util.Random` vectors.
- [x] Ignored probe test for `spyglass-rng`: every `rng.json` value matches (665 cases, first run).
- [x] `spyglass-noise`: `PerlinNoise` (`wrap`, permutation table, `gradDot`, `smoothstep`, `sampleAndLerp`), with a structural check against `parityConfigString` output.
- [x] `spyglass-noise`: `NoiseStack` and `NormalNoise` (octave list, amplitude modifiers, normalization factor, second stack at `INPUT_FACTOR`). Powers of two computed exactly, not through `powf`.
- [x] Ignored probe test for `spyglass-noise`: every `noise.json` sample matches bit for bit, for every golden seed (2,030 cases, first run; a deliberate one-ulp change to `smoothstep` breaks 1,915 of them).
- [ ] `docs/specs/rng.md`: the random sources, seeding (128-bit upgrade, MD5 name seeding, positional `at`), and the Java semantics they depend on.
- [ ] `docs/specs/noise.md`: Perlin with `wrap`, the noise stack, normal-noise construction and normalization, and what is `f32` vs `f64` at each step.
- [ ] `docs/specs/README.md`: list the new specs.

## Verification

- Unit tests pass under `cargo test` with no oracle output present.
- With `cargo xtask oracle probe` run, the ignored tests (`cargo test -p spyglass-rng -p spyglass-noise -- --ignored`) match every probe value bit for bit, for every seed in the golden set.
- `parityConfigString` output matches spyglass's own description of each climate noise, so a failure can be told apart as setup (wrong octaves or permutations) or sampling.

## Durable outputs

- [ ] `docs/specs/rng.md`
- [ ] `docs/specs/noise.md`
- [x] `docs/specs/oracle.md`: `probe` output format and command
- [x] ADR 0008: game-derived test values
- [x] ADR 0009: MD5 dependency
- [ ] `docs/getting-started.md`: running the probe and the ignored tests (coordinate with whoever is editing it)

## Notes

- 2026-10-25: `spyglass-noise` matches all 2,030 `noise.json` cases. Two details a source-level port would likely miss: `NormalNoise`'s normalization sums amplitudes with `DoubleStream.sum()`, which is Kahan-compensated, and `normalize` defaults to `ENABLED` in the codec. `LEGACY` normalization isn't ported (no 26.3 noise uses it) and is refused with an error.
- 2026-10-25: `spyglass-rng` matches all 665 `rng.json` cases. Ported from `javap -c` bytecode rather than decompiled source, since bytecode is exact about widths and operation order. The Gaussian uses `Math.log`, a transcendental whose last bit could differ between Java's intrinsic and Rust's libm; all 18 probed Gaussian sequences match, and climate doesn't use Gaussians, so it's noted in the spec rather than solved.
- 2026-10-25: Plan drafted. The `javap` survey above shows 26.3 reworked noise to return `float` and reworked density functions into compiled samplers (see [m02-density](m02-density.md)). Old-version references are background only.
