# 0012. Java trig results that depend on the platform

- Status: Accepted
- Date: 2026-10-25

## Context

Some worldgen code calls `java.lang.Math.sin` and `Math.cos`: stronghold ring positions (`ChunkGeneratorStructureState.generateRingPositions`) and the world spawn search (`NoiseSpawnFinder.radialSearch`) in 26.3. Bit-exact parity needs their results, but they aren't reproducible:

- `Math` (unlike `StrictMath`) may use any implementation within 1 ulp, and HotSpot replaces it with per-CPU intrinsics. On x86_64 that's a table-driven implementation in HotSpot's assembler, licensed GPLv2 with the Classpath Exception. Other CPUs use other code.
- Measured on 2026-10-25 with Temurin 25.0.4 on Windows x86_64, over 2 million inputs in [0, 128) (uniform, ring-like angles and near multiples of π/2): `Math.sin` differed from `StrictMath.sin` (fdlibm) on 44,849 inputs and `Math.cos` on 45,074. Rust's `f64::sin` and `cos` (the MSVC runtime's) differed from `Math` on 42,093 and 42,228 inputs, and from `StrictMath` on 30,956 and 31,120. Every difference was 1 ulp.
- So the game's own answer depends on the JVM and CPU it runs on, and no single implementation matches "the game".
- Rust's `f64::sin` doesn't promise an accuracy, and it differs between platforms too.

The callers in 26.3 always round the product immediately: `Math.round(Math.cos(angle) * distance)` for rings, and an `int` cast for spawn. A difference of a few ulps changes the result only when the exact product is within a few ulps of a rounding boundary, which is very rare. But "very rare" isn't bit-exact, and nothing tells us when it happens.

## Decision

We will compute these with Rust's `f64::sin` and `f64::cos`, and treat a result as **platform-dependent** when changing the sin or cos value by up to 4 ulps either way changes the rounded result. A platform-dependent result is reported as such, with every answer the perturbation can produce, the way tied biome cells are ([biome-lookup.md](../specs/biome-lookup.md#ties)). Each spec that uses trig names the call and its rounding step.

## Consequences

- Answers we don't flag are the same on every platform the game runs on, for any implementation within 2 ulps of the true value, ours included. That covers the 1 ulp the measurements show, with headroom for platforms we haven't measured.
- No ported code and no new dependency. GPL code stays out of an MIT crate.
- Flagged results need an API shape that carries more than one answer, as ties already do. That's more for callers to handle, such as a stronghold ring position with two candidates.
- The 4-ulp margin is a judgement, not a proof. A JVM with a worse `Math.sin` could still differ unflagged. The oracle comparison would catch that on the platform it runs on, but not on others.
- The check costs a few extra multiplies and roundings per call. That doesn't matter at 128 ring positions or a few hundred spawn samples.
- The check is per call. A later caller that doesn't round right away (feeding sin into more arithmetic) needs its own analysis, recorded in its spec.

## Alternatives considered

- **Port HotSpot's x86_64 intrinsic:** matches the game on the most common platform only, and it's GPL code in an MIT project. Rejected.
- **Port fdlibm (`StrictMath`):** deterministic and permissively licensed, but it measurably isn't what `Math` returns on x86_64, so it's no closer to the game than `f64::sin`. It's more code for no gain, since flagging is needed anyway.
- **The `libm` crate:** deterministic across our platforms, but it's a dependency that still doesn't match the game, for the same reason.
- **Ignore it:** the error is rare, but silent. That contradicts the bit-exact rule and the oracle's purpose.
