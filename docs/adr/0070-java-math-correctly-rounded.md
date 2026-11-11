# 0070. Java's `Math`, correctly rounded, in a crate of its own

- Status: Accepted
- Date: 2026-11-11
- See also: [0012](0012-java-trig-results-that-depend-on-the-platform.md), whose flagging this keeps and whose implementation choice it replaces; [0068](0068-float-functions-and-more-clippy-lints.md)

## Context

Worldgen calls `Math.sin`, `cos`, `log` and `pow`: the stronghold rings, the spawn search, the carvers' sine table, the Gaussian, the dripstone height and density functions' `log` and `pow`. spyglass called the platform's libm for each, through Rust's `f64` methods. ADR 0012 accepted that for `sin` and `cos`, flagging any rounded result that 4 ulps could change, since the game's own answer depends on its JVM and CPU and no single implementation matches "the game".

Two things made that worth revisiting. The platform's libm differs between spyglass's own builds, Microsoft's on Windows, glibc's on Linux and Apple's on macOS, so unflagged answers could differ between them. And Horizon reported that HotSpot's x86-64 intrinsics are correctly rounded for nearly every argument ([prior art](../research/prior-art.md)), which would make "the game's answer on x86-64", where most players are, computable.

The oracle's new `math` command measured it on Windows x86-64 with Temurin 25 ([java-math.md](../specs/java-math.md#measurements)): `Math` is correctly rounded but for 0.1% of `sin` and `cos` arguments, 0.002% of `log`'s and 0.05% of `pow`'s, each by 1 ulp. Microsoft's libm rounds `sin` and `cos` otherwise for 3%. The `libm` crate and `StrictMath` are 2 to 10% away from it. Narrowed to `float`, as the carvers' table is, no implementation differed at all.

## Decision

We will compute Java's `Math.sin`, `cos`, `log` and `pow` correctly rounded, in `spyglass-math`, a new crate below `spyglass-rng`, and route every worldgen call through it. It uses double-double arithmetic with no fused multiply-add and no platform libm, and returns a result only when Ziv's rounding test settles it to a bound well above its error. Java's special cases for `log` and `pow` are kept.

ADR 0012's flagging stays as it is, with its 4-ulp margin: the game's answer on AArch64 (fdlibm with fused multiply-adds) and on other JVMs isn't x86-64's, and the measured 1-ulp gaps fit well within the margin. `clippy.toml` keeps the platform's functions disallowed, and `spyglass-math` is the only code that may call the `libm` crate, for `sin` and `cos` beyond 2^20.

## Consequences

- Every spyglass build gives the same bits for these functions, whatever system it runs on, and they're x86-64 HotSpot's but for the measured fractions. The Gaussian's `log` goes from 0.15% different on Windows to 0.002%, and the rings' and spawn's trig from 3% to 0.1%, before ADR 0012's flags.
- The checks are repeatable: `cargo xtask oracle math` and `math-report` on any machine add its platform, and 766 committed cases test `spyglass-math` bit for bit wherever CI runs.
- **Costs:** a crate of about 500 lines of numerical code to keep correct, where the platform's functions cost nothing. Each call is several times slower than libm's, which matters only where a call is hot (measured with `cargo xtask bench`, java-math.md). The cases x86-64's `Math` isn't correctly rounded stay wrong, as they would with any implementation but HotSpot's own. `sin` and `cos` of |x| ≥ 2^20, and `pow` results below 2^-1021, aren't correctly rounded.

## Alternatives considered

- **The platform's libm, as before:** zero cost, but builds disagree with each other, and Windows differs from the game on x86-64 for 3% of `sin` and `cos`.
- **The `libm` crate everywhere:** the same on every platform, but 7 to 10% from x86-64's `log` and `pow`, and 2 to 3% from its `sin` and `cos`. It stays only as the fallback for huge trig arguments.
- **fdlibm (`StrictMath`), ported:** the same on every platform and closest to AArch64's `Math`, but 2 to 10% from x86-64's. Most players are on x86-64.
- **HotSpot's x86-64 intrinsics, ported:** would match x86-64 exactly, but they're GPLv2 code in an MIT project, as ADR 0012 found.
- **A per-platform model, as Horizon's `jmath`:** Horizon returns what vanilla returns on the machine it runs on, which suits a game. spyglass predicts a world that could be generated on any machine, so one fixed answer, flagged where platforms could differ, fits better.
- **Adapting Horizon's MIT code, or a correctly rounded C library (CORE-MATH):** Horizon's `exact` module covers `sin`, `cos` and `log` but not `pow`, and a C dependency needs a C toolchain in every build. The same technique, written for spyglass and checked against the oracle's `BigDecimal` results, needed neither.
