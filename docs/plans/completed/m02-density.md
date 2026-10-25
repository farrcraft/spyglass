# m02. Climate density functions

- Status: Completed
- Milestone: [02. RNG, noise and biomes](../../roadmap/completed/02-rng-noise-biomes.md)
- Started: 2026-10-25
- Completed: 2026-10-25

## Goal

Load the overworld's noise settings and density functions from a version bundle, and evaluate the six climate router entries (`temperature`, `vegetation`, `continents`, `erosion`, `depth`, `ridges`) at any block position, bit for bit with the game. Covers the milestone's `spyglass-density` scope and the density-function part of the third exit criterion.

The second of three plans for milestone 02. Needs [m02-rng-noise](m02-rng-noise.md); [m02-biomes](m02-biomes.md) builds on it.

## Facts established up front

Checked against `bundles/26.3/` and the 26.3 jar (`javap`) on 2026-10-25.

- **26.3 density functions are a new engine.** They live in `net.minecraft.world.level.levelgen.densityfunction` (with `generator/` and `op/` subpackages). A `DensityFunction` is compiled to a `DensitySampler` by `DensityFunctionCompiler`, after the rewrite rules `DfRewriteRule.INLINE_REFERENCE` and `SLICE_UNIFORM_AXES` (which drops axes a subtree doesn't depend on). Samplers have `sampleValue(SamplerContext, int x, int y, int z)` returning **`float`**, and `sampleVolume` over a `DensityBuffer`. Constants are `float`. The old 1.21 `DensityFunction.compute(FunctionContext)` returning `double` is gone.
- **Codec names** registered by `DensityFunctions.bootstrap` include `constant`, `noise`, `end_outer_islands`, `distance_to_point`, `gradient`, `shift_a`, `shift_b`, `shift`, `pow`, `spline`, `lerp`, `clamp`, `range_choice`, `interval_select`, `cache`, `blend_density`, `interpolated`, `slice`, `find_top_surface` and `old_blended_noise`, plus the unary and binary operators (`abs`, `square`, `add`, `mul`, …) and `blend_alpha`, `blend_offset`, `beardifier`. Compared with 1.21: `noise` now takes optional `shift_x`/`shift_z` (absorbing `shifted_noise`), `gradient` with an `axis` and `tiling` replaces `y_clamped_gradient`, and a single `cache` replaces `flat_cache`, `cache_2d`, `cache_once` and `cache_all_in_cell`.
- **Noise settings shape** (`worldgen/noise_settings/overworld.json`): keys `aquifers`, `debug_functions`, `default_block`, `default_fluid`, `disable_mob_generation`, `legacy_random_source` (`false`), `material_rule`, `noise` (`{"min_y": -64, "height": 384}`, no size fields), `noise_router`, `sea_level` (63), `spawn_target`. The router (`NoiseRouter`) has exactly eight entries: `temperature`, `vegetation`, `continents`, `erosion`, `depth`, `ridges`, `chunk_surface_level`, `final_density`, each a reference such as `"minecraft:overworld/continents"`. Aquifer functions moved out of the router into `aquifers`.
- **The climate closure is small.** Walking the six climate entries through references and nested splines reaches 10 named functions (`overworld/{temperature, vegetation, continents, erosion, depth, ridges, offset, ridges_folded}`, `shift_x`, `shift_z`) and these types only: `noise` (5), `shift_a`, `shift_b`, `cache` (6), `add` (4), `mul`, `abs` (2), `gradient` (1, `axis: "y"`), `lerp`, `spline` (1, with 53 nested cubic splines), `blend_alpha`, `blend_offset`, plus number constants. It uses 6 noises: `temperature`, `vegetation`, `continentalness`, `erosion`, `ridge`, `offset`. The full overworld router uses 29 types and 31 noises; that's milestone 07's.
- **Shapes seen**:
  - `overworld/temperature`: `{"type": "minecraft:noise", "noise": "minecraft:temperature", "shift_x": "minecraft:shift_x", "shift_z": "minecraft:shift_z", "xz_scale": 0.25, "y_scale": 0.0}`;
  - `shift_x`: `cache` of `{"type": "minecraft:shift_a", "noise": "minecraft:offset"}` (`ShiftNoiseFunction.COORDINATE_FACTOR = 0.25`, `VALUE_FACTOR = 4.0f`);
  - `overworld/depth`: `add` of a `gradient` on y (−64 → 1.5, 320 → −1.5, no `tiling`, so the codec default) and `overworld/offset`;
  - `overworld/offset`: `cache` of `lerp(alpha: blend_alpha, first: blend_offset, second: add(-0.50375, spline(...)))`, where the spline's coordinates are `overworld/continents`, `overworld/erosion` and `overworld/ridges_folded`, with `location`, `value` (a number or a nested spline) and `derivative` per point;
  - `overworld/ridges_folded`: `mul(add(abs(add(abs(ridges), -0.6666667)), -0.33333334), -3.0)`.
- **Classes to port from**: `generator.NoiseFunction` (and its `ShiftedXzSampler`), `generator.ShiftNoiseFunction` (`ShiftA`, `ShiftB`), `generator.GradientFunction` (`TilingMode`: `CLAMP_TO_EDGE`, `REPEAT`, `MIRRORED_REPEAT`), `op.BinaryFunction` (and its constant-specialized samplers such as `ConstAddSampler`, `ConstMulSampler`), `op.UnaryFunction`, `op.LerpFunction` (`Mth.lerp(float, float, float)`), `op.CacheFunction`, `op.SplineFunction` over `net.minecraft.util.CubicSpline` (`CubicSpline$Multipoint`: `float[] locations`, `float[] derivatives`, `findIntervalStart`, `linearExtend`, `Mth.lerp`, `Math.min`/`max` on floats).
- **Climate sampling.** `NoiseRouter.createClimateSampler` binds the six functions into a `Climate$Sampler`, whose `sample(qx, qy, qz)` calls each one's `sampleValue` at `QuartPos.toBlock` of each coordinate (block = quart × 4, no centring offset) and passes the six floats to `Climate.target`. `Climate.quantizeCoord(float)` is `(long)(f * 10000.0f)`: a float multiply, then truncation.
- **The bundle reader today** (`spyglass_data::bundle`) writes, verifies and reads `bundle.json`, but has no API for reading worldgen resources by id. `spyglass-noise` must not depend on `spyglass-data` (architecture table), so JSON → noise parameters happens in `spyglass-density`.

## Approach

1. **Bundle access** in `spyglass-data`: open a bundle (checking `bundle.json`'s format), and read a resource by registry and id (`density_function`, `minecraft:overworld/continents` → `data/minecraft/worldgen/density_function/overworld/continents.json`), and a report by path. Bytes or `serde_json::Value`; no worldgen types here.
2. **Parsing** in `spyglass-density`: JSON → a `DensityFunction` tree (an enum), following the game's codecs, including their shorthands: a number is a constant, a string is a reference to a named function, an object is inline. Named references are loaded once and shared. Unknown types fail at load time with the type name and the path to it, rather than at evaluation. Noise parameters (`worldgen/noise/*.json`) parse into `spyglass_noise::NormalNoiseParameters`. Noise settings parse just far enough for `noise_router`, `noise` and `legacy_random_source`.
3. **Binding to a seed**: walk the tree, create each referenced noise once per seed (as `RandomState.getOrCreateNoise` does, via [m02-rng-noise](m02-rng-noise.md)'s RNG), and produce a bound, evaluable form.
4. **Evaluation**: a point interpreter, `sample(x, y, z) -> f32`, for the climate types. It matches `sampleValue` exactly, including the game's constant specializations and the float operation order of each sampler. `cache` is transparent for point evaluation. Buffered volume evaluation, flattening and SIMD are later work (architecture's "later: flattened, SIMD-friendly evaluation").
5. **Values from the game**: extend the oracle's `probe` with `density.json`: for each golden seed, `RandomState.sampleBlockValueUncached` of each of the 10 named functions at a fixed point list (including negative, far and border coordinates, and many y levels for `depth`), and the six `Climate$Sampler` floats and `TargetPoint` longs at the same points. Also record the chunk path's buffered values for the same cells, to show whether point and volume evaluation agree bit for bit (see open questions).

## Tasks

- [x] ADR 0010 (accepted 2026-10-25): loading worldgen JSON into typed structures. Where parsing lives (`spyglass-data` gives bytes or `Value`; each layer parses its own types), whether to use serde derive or a hand-written walk over `serde_json::Value` (codec shorthands and the number/string/object union fit a hand-written walk better), how references are resolved and shared, and how unknown types are reported. Affects every later layer, so record it before writing the parser.
- [x] `spyglass-data`: bundle resource access by registry and id, and report access, with unit tests on a synthetic bundle (`spyglass_data::bundle::Bundle`).
- [x] Confirm by `javap -c`: `GradientFunction`'s default `tiling` and its float math; what `blend_alpha` and `blend_offset` return with no blending (expected 1 and 0) and whether `lerp(1.0f, a, b)` is exactly `b` in float (it need not be); `ShiftA`/`ShiftB` argument order; `NoiseFunction`'s sampler when `y_scale` is 0 and how `SLICE_UNIFORM_AXES` fixes the dropped axis (and whether `y * 0.0` giving `-0.0` for negative y can change a noise value); and whether each `Const*Sampler` computes the same float as the general sampler.
- [x] `spyglass-density`: parser for the climate types and shorthands, with errors that name the unsupported type. Unit tests on small hand-written JSON (not game data).
- [x] `spyglass-density`: noise parameter and noise settings parsing.
- [x] `spyglass-density`: cubic spline evaluation (`CubicSpline$Multipoint` sampling, `findIntervalStart`, `linearExtend`, constant leaves), all in `f32`.
- [x] `spyglass-density`: binding to a seed and point evaluation for `constant`, `noise` (with shifts), `shift_a`, `shift_b`, `cache`, `add`, `mul`, `abs`, `gradient`, `lerp`, `spline`, `blend_alpha`, `blend_offset`.
- [x] `spyglass-density`: load the real 26.3 overworld router from the bundle; an ignored test that every density function in the bundle parses or is rejected with a clear "unsupported type" error, and that the six climate entries parse fully.
- [x] Oracle `probe`: `density.json` section as above. Spec it in `docs/specs/oracle.md`.
- [x] Ignored probe test: all 10 named functions and the six climate floats and longs match bit for bit for every golden seed.
- [x] `docs/specs/density-functions.md`: the JSON forms, the float semantics of each supported type, the evaluation contract (`f32`, block coordinates), which types are supported so far, and the climate sampler (quart → block, quantization).
- [x] `docs/specs/bundle-format.md`, "Reading": codec shorthands for density functions.
- [x] `docs/architecture.md`: update the Density row if the evaluation model or crate dependencies change (no change needed: it still reads "graph interpreter", depending on noise and data).

## Verification

- Unit tests on hand-written graphs and splines pass without a bundle.
- With a 26.3 bundle and `cargo xtask oracle probe` output, the ignored tests match every probed density value and climate target bit for bit, for every golden seed, including negative, far and border coordinates.

## Durable outputs

- [x] ADR 0010: typed loading of worldgen JSON
- [x] `docs/specs/density-functions.md`
- [x] `docs/specs/oracle.md`: `density.json` probe section
- [x] `docs/specs/bundle-format.md`: reading density functions
- [x] `docs/specs/README.md`: list the new spec
- [x] `docs/architecture.md`, if anything changed (nothing did)

## Open questions

Both answered by the probe on 2026-10-25:

- **Point and volume evaluation are bit-identical** for the six climate functions in every probed chunk (40 chunks, every cell), so the point interpreter is enough. The probe test fails if that ever changes.
- **The compiler's rewrites don't change climate values:** the point interpreter, which applies neither rewrite, matches the game bit for bit, chunk digests included.

The questions as first asked:

- **Are point and volume evaluation bit-identical?** The game computes biomes for chunks through `sampleVolume` on buffers, but the oracle spike found the point path (`createUncachedResolver`) gave identical _biomes_ on every cell tested. If volume evaluation rounds differently anywhere (a different operation order in a vectorized loop, say), spyglass's point interpreter would need to follow the volume path instead. The probe answers it directly.
- **Do the compiler's rewrites change values?** `INLINE_REFERENCE` shouldn't. `SLICE_UNIFORM_AXES` evaluates a y-independent subtree at a fixed y; if that differs from evaluating at the real y (signed zero, for instance), the interpreter must apply the same rewrite.

## Notes

- 2026-10-25: Done. All 2,040 `density.json` cases match on the first run, including per-chunk digests of every climate float (about 1.5 million values). The digests were added after a mutation check: reassociating the spline's `t·(1−t)·lerp` changed no value at the 200 sampled points per function but breaks 6 chunk digests. Two findings now in specs and ADRs: the game reads `float` fields with `Float.parseFloat` of the JSON text (ADR 0011, accepted 2026-10-25), and it compiles division by a constant to multiplication by the reciprocal (not yet needed). 28 of 26.3's 55 density functions load; the rest need terrain types.
- 2026-10-25: Plan drafted from a walk of `bundles/26.3/` and the 26.3 jar.
