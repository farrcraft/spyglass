# 24. Java's `Math`, as the game computes it

- Status: Planned
- Depends on: none
- Plans: [m24-java-math](../plans/m24-java-math.md)

## Goal

spyglass computes Java's `Math` functions the way HotSpot does on the platforms players use, instead of with the platform's own libm. Then two things are true:

- Every spyglass build gives the same answers, whatever system it runs on. Today the Windows, Linux and macOS archives call three different libms, which disagree in the last bit for a few percent of arguments, so the results [ADR 0012](../adr/0012-java-trig-results-that-depend-on-the-platform.md) doesn't flag can differ between them.
- The float call sites that are unchecked today ([ADR 0068](../adr/0068-float-functions-and-more-clippy-lints.md)) match what the game computes on x86-64, or are measured and stated not to.

Horizon's `jmath` is the lead: it models HotSpot as correctly rounded `sin`, `cos`, `tan`, `exp`, `log`, `log10` and `pow` on x86-64, fdlibm for `atan2`, `asin` and the rest, and fdlibm with fused multiply-adds for `sin` and `cos` on AArch64 ([prior art](../research/prior-art.md)). spyglass measures that for itself before relying on it.

## Scope

- **Measuring.** The oracle writes `Math` and `StrictMath` results for the arguments spyglass's call sites take, and for broad and edge sets: on the dev machine (Windows x86-64), on CI's Linux x86-64, and on an Apple silicon runner.
- **The functions.** Java's `Math` for the functions the game calls in worldgen: `sin`, `cos`, `log` and `pow`, `exp` if a call site needs it, each in a fixed implementation (correctly rounded, or fdlibm) chosen from the measurements. They live in one module, the only one that may call the `f64` functions `clippy.toml` disallows.
- **The call sites.** Every `#[expect(clippy::disallowed_methods)]` goes through that module instead. ADR 0012's flagging stays, since the game's own answer still depends on its platform, with its margin reconsidered from the measurements.
- **Agreement across systems.** Unit tests with committed expected bits, computed outside the game, pass on every CI system.

Not in scope:

- Modelling every JVM. The model is HotSpot on x86-64 and AArch64, JDK 25; other JVMs stay within ADR 0012's margin or are flagged.
- `f32` functions: Java has none, and the game narrows `double` results.

## Exit criteria

- [ ] The measurements are recorded in a spec: per function and platform, how often HotSpot's `Math` differs from each candidate implementation.
- [ ] An ADR records the implementation chosen, the crate it lives in, and how it relates to ADR 0012.
- [ ] No spyglass code outside that module calls a disallowed float function.
- [ ] `cargo xtask oracle-test` passes for every supported version, on Windows and on Linux.
- [ ] Unit tests of the functions pass, with the same bits, on Windows, Linux and macOS in CI.
