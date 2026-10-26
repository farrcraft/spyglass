# Random sources

Status: stable. Implemented in `spyglass-rng` for 26.3; checked against the game by the oracle's [probe files](oracle.md#probe-files).

Worldgen draws all its randomness from two random sources and the factories that derive per-position and per-name sources from them. Every call must consume exactly the values the game's does: one extra or missing call shifts everything after it. Ported from the 26.3 bytecode (`net.minecraft.world.level.levelgen`).

## Java semantics

- Integer arithmetic wraps (`wrapping_*`), at the declared width: `int` is `i32`, `long` is `i64`.
- `>>>` is a logical shift (shift the unsigned value); `>>` is arithmetic.
- `float` is `f32`, `double` is `f64`; `(float)` of a `long` or `int` and `(int)` of a floating-point value behave as Rust's `as`.
- `String.hashCode()` hashes UTF-16 code units: `h = 31h + unit`, wrapping, so a character outside the BMP counts as two.

## Seeds

- `mixStafford13(z)`: `z = (z ^ z>>>30) * 0xBF58476D1CE4E5B9; z = (z ^ z>>>27) * 0x94D049BB133111EB; z ^ z>>>31`. This is SplitMix64's finalizer.
- `upgradeSeedTo128bitUnmixed(seed)`: `lo = seed ^ SILVER_RATIO_64` (`0x6A09E667F3BCC909`), `hi = lo + GOLDEN_RATIO_64` (`0x9E3779B97F4A7C15`). `upgradeSeedTo128bit` mixes both halves with `mixStafford13`.
- `seedFromHashOf(name)`: the MD5 of the name's UTF-8 bytes, read as two big-endian longs: bytes 0–7 are `lo`, 8–15 are `hi` ([ADR 0009](../adr/0009-md5-from-the-md-5-crate.md)).
- `Mth.getSeed(x, y, z)`: `s = (long)(x * 3129871) ^ (long)z * 116129781 ^ (long)y`, with the `x` product computed in `int` (it overflows before widening), then `s = s * s * 42317861 + s * 11` and `s >> 16`.

## Xoroshiro (`XoroshiroRandomSource`)

The source for `legacy_random_source: false`, which the overworld uses.

- **Generator** (`Xoroshiro128PlusPlus`): `result = rotl(lo + hi, 17) + lo; hi ^= lo; lo = rotl(lo, 49) ^ hi ^ (hi << 21); hi = rotl(hi, 28)`. An all-zero state is replaced by `lo = GOLDEN_RATIO_64, hi = SILVER_RATIO_64`.
- **Construction:** from a `long`, through `upgradeSeedTo128bit`. From two longs, unmixed.
- `nextLong()` is the generator's output; `nextInt()` its low 32 bits; `nextBoolean()` its lowest bit.
- `nextFloat()`: the top 24 bits times 2^-24. `nextDouble()`: the top 53 bits times 2^-53.
- `nextInt(bound)` (Lemire): `m = (nextInt() as unsigned) * bound`; if `m & 0xFFFFFFFF < bound`, draw again while it's below `(-bound) mod bound` (unsigned); the result is `m >> 32`. A non-positive bound is an error.
- `consumeCount(n)`: `n` generator steps.
- `fork()`: a source from the next two longs, unmixed. `forkPositional()`: a factory from the next two longs.
- **Positional factory** (`lo`, `hi`): `at(x, y, z)` is the source `(getSeed(x, y, z) ^ lo, hi)`; `fromHashOf(name)` is `seedFromHashOf(name) ^ (lo, hi)`; `fromSeed(s)` is `(s ^ lo, s ^ hi)`. All unmixed.

## Legacy (`LegacyRandomSource`)

`java.util.Random`'s 48-bit LCG, for `legacy_random_source: true` and older parts of worldgen.

- `setSeed(s)`: `(s ^ 0x5DEECE66D) & (2^48 − 1)`. `next(bits)`: `seed = (seed * 0x5DEECE66D + 11) & (2^48 − 1)`, return `(int)(seed >> (48 − bits))`.
- `nextInt()` is `next(32)`; `nextLong()` is `((long)next(32) << 32) + next(32)`; `nextBoolean()` is `next(1) != 0`; `nextFloat()` is `next(24) * 2^-24`; `nextDouble()` is `(((long)next(26) << 27) + next(27)) * 2^-53`.
- `nextInt(bound)`: for a power of two, `(int)((bound * (long)next(31)) >> 31)`; otherwise draw `bits = next(31)`, `value = bits % bound` until `bits − value + (bound − 1)` doesn't overflow negative.
- `consumeCount(n)`: `n` calls to `nextInt()`.
- `fork()`: a source seeded with `nextLong()`. `forkPositional()`: a factory seeded with `nextLong()`.
- **Positional factory** (`seed`): `at(x, y, z)` is a source seeded with `getSeed(x, y, z) ^ seed`; `fromHashOf(name)` uses `String.hashCode()` (not MD5): `(long)hash ^ seed`; `fromSeed(s)` ignores the factory's seed.

## Worldgen seeding (`WorldgenRandom`)

`WorldgenRandom` wraps a source; when it's a `LegacyRandomSource`, `next(bits)` is the inner source's, so drawing from it is drawing from that source. Its seeding methods, all on a legacy source, are in `spyglass_rng::worldgen`. Arithmetic wraps; `(long)` widens an `int`.

- `setLargeFeatureSeed(seed, x, z)`: `setSeed(seed)`, `a = nextLong()`, `b = nextLong()`, then `setSeed((long)x * a ^ (long)z * b ^ seed)`.
- `setLargeFeatureWithSalt(seed, x, z, salt)`: `setSeed((long)x * 341873128712 + (long)z * 132897987541 + seed + salt)`.
- `seedSlimeChunk(x, z, seed, salt)`: a new legacy source seeded with `seed + (long)(x * x * 4987142) + (long)(x * 5947611) + (long)(z * z) * 4392871 + (long)(z * 389711) ^ salt`. The products in `(long)(...)` are `int` products, wrapped in 32 bits; `(long)(z * z)` is widened before the multiply by 4392871.

## Gaussian

Both sources use `MarsagliaPolarGaussian`: draw `v1 = 2·nextDouble() − 1` and `v2` likewise until `s = v1² + v2²` is in `(0, 1)`; then `m = sqrt(−2·ln(s) / s)`, return `v1·m` and cache `v2·m` for the next call. `setSeed` clears the cache. Other calls don't disturb it.

`ln` is a transcendental function: Java's `Math.log` and Rust's `f64::ln` aren't guaranteed to agree in the last bit. Every probed Gaussian matches, and climate sampling doesn't use Gaussians, so no exact `log` is ported yet. If a mismatch appears, port fdlibm's `log` (what `StrictMath.log` uses) and check `Math.log` against it.

## Testing

Committed unit tests use only values from outside the game ([ADR 0008](../adr/0008-game-derived-test-values-stay-local.md)): `java.util.Random` sequences, the JDK's own `jdk.internal.random.Xoroshiro128PlusPlus`, RFC 1321's MD5 suite, SplitMix64's first output and `String.hashCode()`. The ignored test `spyglass-rng/tests/probe.rs` replays every case of `rng.json`.
