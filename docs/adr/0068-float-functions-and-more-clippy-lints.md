# 0068. Float functions and more clippy lints, and zizmor for workflows

- Status: Accepted
- Date: 2026-11-11
- See also: [0005](0005-extra-rust-checks.md), which this extends; [0012](0012-java-trig-results-that-depend-on-the-platform.md), whose rule the new float check enforces

## Context

[ADR 0005](0005-extra-rust-checks.md) set the lint policy: clippy's defaults plus `cast_lossless`, `dbg_macro` and `todo`. A review of the checks in 2026-11 found gaps that matter for a bit-exact port:

- **Float functions Rust doesn't pin down.** `f64::sin`, `ln`, `powf` and the rest come from the platform's math library, and Java's `Math` may answer differently in the last bit. ADR 0012 says each caller needs its own analysis, but nothing found a new caller. When this ADR was written there were 18 such call sites. Some were analyzed (ADR 0012's rings and spawn search, `nextGaussian` in [rng.md](../specs/rng.md)), some were written up but are rare (the density `pow` and `log`, unused in vanilla), and two had no note at all: the carvers' sine table and the dripstone height curve. A third, `PerlinSimplexNoise`'s `powf(2, n)`, has an exact helper (`mth::pow2`) that it wasn't using.
- **Suppressions without an end.** There were 21 `#[allow(...)]` attributes, 6 of them without a reason. One (`room_1x2`'s `too_many_arguments`) no longer suppressed anything.
- **Hash iteration order.** `HashMap` and `HashSet` iterate in an arbitrary order. That's harmless where every element is visited independently, but a bug wherever the order reaches a result, and nothing showed where it happens.
- **GitHub Actions workflows** were checked only for YAML syntax, although `release.yml` attests and publishes builds. Every checkout left the job's token in `.git/config`, and one `run:` line expanded `${{ matrix.version }}` into the script.

## Decision

We will add these checks:

| Check                                                                                                                                                                                                                                                     | Where                       |
| --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------- |
| `disallowed_methods`, configured by `clippy.toml` to cover `f64`'s trig, hyperbolic, exponential, logarithm, `powf`, `powi`, `cbrt`, `hypot` and `mul_add`, and `f32`'s that Java lacks. `sqrt` stays allowed, being correctly rounded in both languages. | `clippy.toml`, `Cargo.toml` |
| `allow_attributes` and `allow_attributes_without_reason`: lints are silenced with `#[expect(lint, reason = "...")]`, which warns once nothing is left to silence                                                                                          | `[workspace.lints.clippy]`  |
| `iter_over_hash_type`, with an `expect` stating why the order doesn't matter wherever it doesn't                                                                                                                                                          | `[workspace.lints.clippy]`  |
| `lossy_float_literal` (no hits; it keeps literals honest), and rustc's `unused_qualifications`                                                                                                                                                            | `[workspace.lints]`         |
| `zizmor`, offline, on every commit that touches a workflow                                                                                                                                                                                                | `.pre-commit-config.yaml`   |

Every remaining float-function call site carries an `#[expect(clippy::disallowed_methods, reason = "...")]` that names what makes it acceptable: ADR 0012's flagging, a spec's note, or narrowing to `f32`. A reason may also say plainly that the site isn't checked yet; it's still visible, which is the point. Sites without a spec note got one ([carvers.md](../specs/carvers.md#shapes), [features.md](../specs/features.md)). `PerlinSimplexNoise` now uses `mth::pow2`.

zizmor's findings were fixed: `persist-credentials: false` on every checkout (no job pushes; `gh` takes `GH_TOKEN`, and `git ls-remote` on the public repository needs no token), and the matrix value passed through `env`. Two are suppressed inline with a reason: the Pages build's mdBook cache, which only `main` and that workflow can write, and `release.yml`'s `./` reusable-workflow path, since GitHub's newer `$/` form is undocumented for workflows and the release can't be tried from a branch.

## Consequences

- A new `sin`, `ln` or `powf` can't land unnoticed. Whoever adds one writes down why its result matches the game, or that it isn't known to.
- Silenced lints say why, and an `expect` that stops being needed fails clippy rather than lingering.
- Hash iteration that could leak its order into a result shows up in review.
- Workflow changes are checked for the usual Actions mistakes before they're pushed.
- **Costs:** `expect` attributes add lines to the code (about 30 so far). A legitimate new float call needs an attribute and a reason. zizmor is one more tool pre-commit downloads (a prebuilt wheel). Its online audits, which need a GitHub token, aren't run.

## Alternatives considered

- **`float_cmp`:** its 20 hits were all exact comparisons ported from Java's `==`, which bit-exact code needs. Silencing each would be noise.
- **`cast_possible_wrap` and `cast_sign_loss`:** 389 hits, nearly all the game's own casts, ported deliberately. With `cast_lossless` on, every remaining `as` already reads as one that can change a value.
- **`unreachable_pub`:** 330 of its 332 hits were in the binary crates (`spyglass-cli`, `spyglass-viewer`, xtask), where `pub` exposes nothing.
- **Porting fdlibm, or the `libm` crate, for every function:** ADR 0012 already rejected it for trig, since `Math` isn't fdlibm on x86_64. The lint makes each site's choice visible without deciding it once for all.
- **actionlint instead of zizmor:** it checks workflow syntax and expressions; zizmor checks their security, which is the gap here. Both could run; syntax errors already show on the first CI run.
- **Mutation testing (`cargo-mutants`), coverage (`cargo-llvm-cov`), Miri, cargo-semver-checks:** considered in the same review. Mutation testing and coverage are worth running by hand, not on every push. Miri has little to check under `unsafe_code = "forbid"`, and semver checks matter only to published crates.
