# m24. Java's `Math`

- Status: In progress
- Milestone: [24. Java's `Math`, as the game computes it](../roadmap/24-java-math.md)
- Started: 2026-11-11
- Completed:

## Goal

Every exit criterion of milestone 24: measure HotSpot's `Math` where spyglass calls it, choose fixed implementations from the measurements, and route every float call site through them, so spyglass's builds agree with each other and with the game on x86-64.

## Approach

Measure first, then decide, then change the call sites. The measurements decide the implementation, not Horizon's model.

### The call sites

From ADR 0068's `#[expect(clippy::disallowed_methods)]`s, as of 2026-11-11:

| Site                                                    | Function     | Game call                                  | Today                        |
| ------------------------------------------------------- | ------------ | ------------------------------------------ | ---------------------------- |
| Stronghold rings (`spyglass-structure`, `placement.rs`) | `cos`, `sin` | `Math.round(Math.cos(angle) * distance)`   | flagged within 4 ulps (0012) |
| Spawn search (`spyglass-biome`, `spawn.rs`)             | `sin`, `cos` | `(int) (Math.sin(angle) * radius)`         | flagged within 4 ulps (0012) |
| Carvers' sine table (`spyglass-blocks`, `carver.rs`)    | `sin`        | `(float) Math.sin(i / 10430.378350470453)` | unchecked; vanilla uses it   |
| Density's cosine mode (`spyglass-density`, `volume.rs`) | `sin`        | the same table                             | unchecked; unused in vanilla |
| Gaussian (`spyglass-rng`, `gaussian.rs`)                | `ln`         | `Math.log(s)`                              | every probe matches (rng.md) |
| Dripstone height (`spyglass-feature`, `dripstone.rs`)   | `powf`, `ln` | `Math.pow`, `Math.log`, kept in `double`   | unchecked                    |
| Density `log` and `pow` (`spyglass-density`, 4 sites)   | `ln`, `powf` | `(float) Math.log`, `(float) Math.pow`     | unchecked; unused in vanilla |

### Measuring

A new oracle command, `math`, needs only the JDK, not the game. It writes, for each function and argument set, the arguments with `Math`'s and `StrictMath`'s results, as raw bits:

- **Call-site sets:** the sine table's 65,536 arguments; stronghold and spawn angles as the callers draw them; dripstone's `x` over `(0, 0.384]` and its exponents; Gaussian `s` over `(0, 1)`.
- **Broad sets:** a million uniform arguments per function over its useful range, with a fixed seed.
- **Edges:** near multiples of π/2, tiny and huge arguments, subnormals, powers of two, arguments near 1 for `log`, and integer and half-integer exponents for `pow`.

These are the JDK's values, not the game's, so a small table of them can be committed as test data, as rng.md's `java.util.Random` sequences are ([ADR 0008](../adr/0008-game-derived-test-values-stay-local.md)). The full sets stay in `oracle-out/`.

A Rust comparison, `cargo xtask math-report`, reads the sets and counts, per function, set and platform, how often each candidate differs from `Math`, and by how many ulps:

- the platform's own `f64` functions (what spyglass does today);
- the `libm` crate (musl's, which descends from fdlibm);
- a port of fdlibm 5.3 (`StrictMath`'s definition), with and without fused multiply-adds for `sin` and `cos`;
- correctly rounded `sin`, `cos` and `log` in double-double arithmetic, checked against `StrictMath` and a high-precision reference where they disagree.

Platforms: the dev machine (Windows x86-64, Temurin 25), a Linux x86-64 CI runner, and an Apple silicon runner, the last two from a `workflow_dispatch` job that uploads the sets.

### Deciding

An ADR records, from the report:

- **Which implementation** each function uses. The expectation, to be confirmed: correctly rounded `sin`, `cos`, `log` and `pow` (what x86-64 HotSpot is reported to give), since that's where most players are. If the `libm` crate measures as well as a hand port, the ADR weighs depending on it ([ADR 0012](../adr/0012-java-trig-results-that-depend-on-the-platform.md) rejected it for trig, before these measurements).
- **Where it lives:** the lowest layer, since `spyglass-rng`'s Gaussian needs `log`. Either a module of `spyglass-rng` or a new crate below it. A new crate updates [architecture.md](../architecture.md#crates), which milestone 23's layer check reads.
- **ADR 0012's margin:** whether 4 ulps still bounds the game's own variation across the measured platforms, or can shrink.
- **Licences:** fdlibm's notice, and Horizon's (MIT) if any of its code is adapted, kept in the module and in the release's third-party notices.

### Changing the call sites

- The module exposes `java_math::{sin, cos, log, pow}`, each documented with the platforms it matches and how often, from the spec. It's the only code with a `disallowed_methods` expectation.
- Each call site in the table calls it instead. The ADR 0012 sites keep their flagging around the new values.
- The sine table is computed once per process either way, so its cost doesn't matter. The dripstone height runs per column of a large dripstone. Correctly rounded functions are slower than libm's, so `cargo xtask bench` runs before and after.
- `clippy.toml`'s reasons point at the module.

## Tasks

- [x] Oracle `math` command: the argument sets and the bit files, in the core (no adapter: the JDK's `Math` doesn't change with the game version)
- [x] `cargo xtask math-report`, with the platform's functions and the `libm` crate as the first candidates
- [x] An fdlibm 5.3 port: not needed. `StrictMath` itself is measured, and the `libm` crate stood in for fdlibm's algorithm; both are 2 to 10% from x86-64's `Math`, so correctly rounded functions were built instead
- [x] Correctly rounded `sin`, `cos` and `log` (and `pow` if the measurements ask for it), with unit tests against high-precision values
- [x] Measure on Windows x86-64 locally
- [ ] Measure on Linux x86-64 and Apple silicon: the Math workflow's `measure` job, run by hand once the branch is pushed
- [x] docs/specs/java-math.md: the measurements, per function, set and platform
- [x] The ADR: implementation, crate, ADR 0012's margin, licences
- [x] The module in its chosen crate; the call sites through it; clippy.toml's reasons
- [x] Specs updated where they describe these calls: rng.md, carvers.md, features.md, density-functions.md, structure-placement.md, spawn.md
- [x] `cargo xtask bench` before and after
- [x] CI: the module's unit tests on macOS as well as Linux and Windows
- [x] `cargo xtask oracle-test` for every version on Windows: all 8 passed, 2026-11-11
- [ ] The oracle CI jobs on Linux, once the branch is pushed

## Verification

- Unit tests with committed `StrictMath` and high-precision values, the same bits on all three CI systems.
- `math-report` shows, per call site, how often spyglass now differs from `Math` on each measured platform; the spec records it.
- The oracle comparisons pass for every supported version. Where they change, the old answer was a platform libm's and the new one is the game's, checked against the golden files.
- Bench lines within their usual noise (about 20% on the dev machine), or the cost is recorded.

## Durable outputs

- [x] docs/specs/java-math.md
- [x] The ADR, and ADR 0012's status if it's superseded
- [x] The specs that describe the calls
- [x] architecture.md, if a crate is added

## Notes

- 2026-11-11, Windows x86-64, Temurin 25: `Math` is correctly rounded but for 0.1% of `sin` and `cos`, 0.002% of `log` and 0.05% of `pow`; Microsoft's libm differs from it in 3% of `sin` and `cos`; the `libm` crate and `StrictMath` 2 to 10%. Narrowed to `float`, the carvers' table and the `float` `pow` sample are the same under all of them. So correctly rounded won, and `spyglass-math` was built for it, checked against the oracle's `BigDecimal` results: it matches all 6.2 million cases. One bug on the way: `cos` returned 1 below 2^-26, where from about 2^-26.5 the answer is the double below 1 (0.5% of the broad set).
- The correctly rounded reference is computed by the oracle itself, in `BigDecimal`, so no high-precision library was needed on the Rust side.
- Each call is 50 to 100 times the platform's (0.24 to 0.92 µs), but no call site is hot: the bench, back to back, moved by a median of 0%. A first after-run was 30% slower on every line, unrelated ones too, which was the machine.

- Horizon's `horizon_core::jmath` (MIT) is the lead: correctly rounded `sin`, `cos` and `log` in double-double arithmetic for x86-64 and Windows, fdlibm for `atan2`, `asin` and the rest, and fdlibm with fused multiply-adds on AArch64 ([prior art](../research/prior-art.md)). Its numbers are its own until the report confirms them.
