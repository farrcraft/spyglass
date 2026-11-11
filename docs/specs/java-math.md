# Java's `Math`

Status: draft. Covers what Java's `Math.sin`, `cos`, `log` and `pow` return, which worldgen calls, how spyglass computes them (`spyglass-math`), and how that was measured. [ADR 0070](../adr/0070-java-math-correctly-rounded.md) records the decision; [ADR 0012](../adr/0012-java-trig-results-that-depend-on-the-platform.md) the flagging that stays.

## What the game computes

`Math` (unlike `StrictMath`) may return anything within 1 ulp of the true value, and HotSpot replaces several of its methods with intrinsics, so the game's own answer depends on the machine it runs on:

- **x86-64:** `sin`, `cos`, `log` and `pow` are HotSpot's intrinsics, which are correctly rounded for nearly every argument (below).
- **AArch64** (Apple silicon): reported by Horizon as fdlibm's `sin` and `cos` with fused multiply-adds, and `StrictMath` (fdlibm 5.3) for the rest ([prior art](../research/prior-art.md)). Not yet measured here.
- **`StrictMath`** is fdlibm 5.3 everywhere, by the Java SE specification.

spyglass computes every function **correctly rounded**, the same bits on every platform, which is x86-64's answer for all but the cases in the last column below. Before, it called the platform's libm through Rust's `f64` methods, so its Windows, Linux and macOS builds could disagree.

## Measurements

`cargo xtask oracle math` writes, for each function and set of arguments, `Math`'s, `StrictMath`'s and the correctly rounded result into `oracle-out/math/<os>-<arch>/`. The correctly rounded result is computed in `BigDecimal` to 60 digits and rounded once. `cargo xtask math-report` compares candidates with them. The game isn't loaded: these are the JDK's values.

The sets: the carvers' sine table (`i / 10430.378350470453`, all 65,536), stronghold angles (`nextDouble() · π · 2`), the spawn search's `float` angles, a broad set (uniform over ±1,000 and log-uniform from 2^-30 to 2^19), and arguments within 8 ulps of 20,000 multiples of π/2 for `sin` and `cos`; the Gaussian's `s`, dripstone's `x`, random positive doubles, and arguments near 1, powers of two and subnormals for `log`; dripstone's `x^(4/3)` and `x^(2/3)`, integer powers of 2 and ½, `float` bases and exponents, and a broad set for `pow`.

Windows 11, x86-64, Temurin 25.0.4, 2026-11-11: the share of cases where each answer differs from `Math`, all by 1 ulp.

| Set                      | Cases     | `StrictMath` | Platform (MSVC) | `libm` crate | Correctly rounded |
| ------------------------ | --------- | ------------ | --------------- | ------------ | ----------------- |
| `sin` table              | 65,536    | 3.35%        | 3.10%           | 3.11%        | 0.12%             |
| `sin` table, as `float`  | 65,536    | 0            | 0               | 0            | 0                 |
| `sin` angles             | 200,000   | 3.35%        | 3.14%           | 3.15%        | 0.12%             |
| `sin` `float` angles     | 200,000   | 3.31%        | 3.07%           | 3.10%        | 0.11%             |
| `sin` broad              | 1,000,000 | 2.44%        | 2.27%           | 2.29%        | 0.09%             |
| `sin` near k·π/2         | 342,110   | 0            | 0               | 0            | 0                 |
| `cos` angles             | 200,000   | 3.38%        | 3.20%           | 3.16%        | 0.12%             |
| `cos` broad              | 1,000,000 | 2.48%        | 2.36%           | 2.31%        | 0.09%             |
| `log` Gaussian           | 500,000   | 7.23%        | 0.15%           | 7.18%        | 0.002%            |
| `log` dripstone          | 200,000   | 6.96%        | 0.03%           | 7.04%        | 0                 |
| `log` broad              | 1,000,000 | 0.11%        | 0.001%          | 0.11%        | 0                 |
| `pow` dripstone, `x^4/3` | 200,000   | 9.39%        | 0.05%           | 9.39%        | 0.05%             |
| `pow` integer powers     | 4,092     | 0            | 0               | 0            | 0                 |
| `pow` `float`s, as float | 200,000   | 0            | 0               | 0            | 0                 |
| `pow` broad              | 300,000   | 9.70%        | 0.05%           | 9.70%        | 0.05%             |

The rows for `cos`'s table and `float` angles, dripstone's `x^(2/3)` and `pow` on `float`s before narrowing are like their neighbours. So on x86-64:

- HotSpot's `Math` is correctly rounded but for about 0.1% of `sin` and `cos` arguments, 0.002% of `log`'s and 0.05% of `pow`'s, never by more than 1 ulp.
- Microsoft's libm, which spyglass's Windows build called, rounds `sin` and `cos` otherwise for about 3% of arguments. Its `log` and `pow` are close to correctly rounded.
- The `libm` crate (musl's) tracks `StrictMath` for `log` and `pow`, 7 to 10% from x86-64's `Math`.
- **Narrowed to `float`, nothing differs:** all 65,536 entries of the carvers' sine table, and the `float` `pow` sample, are the same under every implementation measured.

Linux x86-64 and Apple silicon aren't measured yet. The **Math** workflow, run by hand, measures Linux, macOS and Windows from the JDK alone (`java oracle/src/spyglass/oracle/JavaMath.java <out dir>`, no game needed) and uploads each system's files as an artifact; put each in `oracle-out/math/<os>-<arch>/` (`linux-x86_64`, `macos-aarch64`) for `math-report`.

## spyglass's functions

`spyglass-math` computes `sin`, `cos`, `log` and `pow` in double-double arithmetic (two `f64`s per value, about 106 bits) from error-free transformations, with no fused multiply-add and no call into the platform's libm:

- **`sin` and `cos`:** reduced by π/2 held as three doubles, to |r| ≤ π/4, then a Taylor series of 15 terms. Below 2^-26 `sin x` is `x`; below 2^-27 `cos x` is 1. From |x| = 2^20 on, the `libm` crate answers, not correctly rounded; no worldgen call comes near.
- **`log`:** `x = m·2^e` with `m` in [√½, √2), `ln x = e·ln 2 + 2·atanh((m − 1)/(m + 1))`, 23 terms. Java's special cases: NaN for negative arguments, −∞ at 0, 0 at 1.
- **`pow`:** Java's special cases first (`pow(x, 0)` is 1 and `pow(x, 1)` is `x`, NaN included; zeros, infinities and negative bases by the parity of an integer exponent). Then `exp(y·ln x)`: `exp` reduced by ln 2 to a 25-term series and scaled by a power of two. Results below 2^-1021 are rounded twice, and may be an ulp off.

Each result is accurate to about 2^-100 of its value (2^-94 for `pow` near overflow), and is rounded once when the interval it lies in rounds the same way at both ends (Ziv's test): 2^-95 of the value for `sin`, `cos` and `log`, 2^-88 for `pow`. Where the test leaves the rounding open, the double-double's own rounding is returned. Of the 6.2 million measured arguments, 9 were left open, and all 9 still matched: 7 arguments of `log` within a few ulps of 1, where `ln(1 + d) ≈ d − d²/2` lies close to a rounding boundary, and `pow`'s two results of 2^-1022.

`math-report` checks spyglass's functions against the correctly rounded results: they match in every case measured. `spyglass-math/tests/correctly_rounded.rs` holds 766 of those cases, about 40 from each set, and checks them bit for bit on every system CI runs.

**Cost.** A call takes 0.24 µs for `sin`, 0.39 µs for `log` and 0.92 µs for `pow` (release build, Ryzen 7 5800X), 50 to 100 times the platform's. No call site is hot: the carvers' table is built once per process (65,536 calls, about 15 ms), the rings and the spawn search make a few hundred, and only the `clamped_normal` float provider draws Gaussians. `cargo xtask bench`, run before and after back to back on 2026-11-11, moved by a median of 0% across its 112 lines, each within the machine's noise.

## Call sites

| Call                                           | Function     | Narrowed | Status                                                                         |
| ---------------------------------------------- | ------------ | -------- | ------------------------------------------------------------------------------ |
| Stronghold rings (`structure-placement.md`)    | `cos`, `sin` | no       | flagged where 4 ulps change the rounded product (ADR 0012)                     |
| Spawn search (`spawn.md`)                      | `sin`, `cos` | no       | flagged likewise                                                               |
| Carvers' sine table (`carvers.md`)             | `sin`        | `float`  | every entry the same under every implementation measured                       |
| Density functions' cosine mode                 | `sin`        | `float`  | the same table; unused in vanilla                                              |
| Gaussian (`rng.md`)                            | `log`        | no       | x86-64's `Math` but for about 0.002% of `s`                                    |
| Dripstone height (`features.md`)               | `pow`, `log` | no       | x86-64's `Math` but for about 0.05% of `pow`; the height is rounded to a block |
| Density `pow` and `log` (density-functions.md) | `pow`, `log` | `float`  | unused in vanilla                                                              |

`clippy.toml` disallows the platform's float functions everywhere else ([ADR 0068](../adr/0068-float-functions-and-more-clippy-lints.md)).
