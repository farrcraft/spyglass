# m09. Fast density evaluation

- Status: Draft
- Milestone: [09. Fast density evaluation](../roadmap/09-fast-density.md)
- Started: 2026-10-27
- Completed: not yet

## Goal

Replace the density graph interpreter with a flattened evaluation, through both the point and the volume paths, for every evaluator family. Values must stay the same bit for bit, and terrain must cost well under the game's. This covers all three of the milestone's exit criteria:

- **Every density, height and structure oracle comparison still passes**, for every supported version.
- **`height_at` for one isolated column in a bound session takes at most 150 µs** (release, one thread, 26.3).
- **Bench lines for columns, regions, confirmed search and jigsaw layouts**, recorded before and after.

## Facts established up front

From the code at c98dbeb, the bench (`cargo xtask bench <version>`, 2026-10-27, this machine) and milestone 07's measurements.

- **How evaluation works now.**
  - A bound function is a `Sampler`: an `Arc<Node>` tree that `Binder::bind` builds, making the game's compile-time choices as it goes (`sampler.rs`, about 1,700 lines).
  - The point path (`Sampler::sample`) recurses through the tree. A per-thread memo computes each shared node once per position, and each y-independent node once per column.
  - The volume path (`Sampler::sample_volume`, `volume.rs`, about 1,200 lines) fills each node over the whole volume, children first. It allocates a `Vec<f32>` per node per call, and keeps each node's last volume in a `Vec` indexed by slot.
  - Noise sampling is already batched per octave over a volume (`NoiseStack::add_to_volume`).
  - 26.2's family (`v1_21`) is a separate `f64` evaluator. It has no volume path, its caches live in a `NoiseChunk`, and it builds one `NoiseChunk` per column.
- **Where an isolated column's time went at milestone 07's close** (26.3, 243 µs): about 185 µs for the `final_density` volume, and about 140 µs for the aquifer's surface levels (49 `find_top_surface` samples), partly overlapping. Splitting a column into four 64-block volumes cost 205 µs against 184 µs for the whole column, because each volume call has a large fixed cost. The game's `getBaseHeight` takes about 500 µs a column.
- **Bench lines now** (26.3; 26.2 in brackets):

  | Line                                     | 26.3          | 26.2        |
  | ---------------------------------------- | ------------- | ----------- |
  | `height_at` one column, bound            | 393 µs        | 3.1 ms      |
  | `height_at` one nearby column, bound     | 307 µs        | 364 µs      |
  | `height_at` one column, new session      | 865 µs        | 2.3 ms      |
  | `heights_in` 40 × 25 columns             | 23 ms         | 82 ms       |
  | `height_map` 256², scale 1               | 2.2 s         | 5.3 s       |
  | `structure_layout` village_plains, bound | 133 ms        | 118 ms      |
  | witch huts confirmed, 1 thread           | 2,822 seeds/s | 430 seeds/s |
  | `World::open`                            | 426 ms        | 251 ms      |

  The isolated column is 393 µs, not milestone 07's 243 µs. Something since then has made it slower: milestone 08's anchors, switches, `upscale` or the multi-version loaders, or noise on the machine. The first task finds out. The target needs about 2.6× from 393 µs, or 1.6× from 243 µs.

- **26.2 is 4 to 8 times slower than 26.3** on isolated columns and searches, and close to it where columns are near each other. The difference is in the family's structure, not in its arithmetic.
- **The architecture already names this step.** [architecture.md](../architecture.md) lists the density layer as "graph interpreter (later: flattened, SIMD-friendly evaluation)".
- **Constraints from the project.**
  - Stable Rust, MSRV 1.95. `std::simd` isn't stable, so SIMD means auto-vectorized loops, `std::arch` intrinsics, or a crate, which is an ADR and a `deny.toml` license check.
  - Bit-exact means the same IEEE operations in the same order for each value. Rust doesn't contract `a * b + c` into a fused multiply-add unless asked, but explicit `mul_add` and intrinsic FMA are out.
  - The noise gradient lookups are table gathers, which SIMD handles poorly without AVX2.
  - The point and volume paths have different arithmetic in places ([density-functions.md](../specs/density-functions.md)). The flattened form must keep each path's own arithmetic, not unify them.

## Approach

### Measure first

Before choosing a design, break down where an isolated column's time goes today, in each family, and how much each of these costs:

- tree-walking and `Arc` chasing;
- allocation;
- the memo;
- noise arithmetic;
- `find_top_surface`;
- the aquifer.

Use a sampling profiler (`samply` works on Windows and Linux) and counters behind a feature. The design should follow the breakdown. For example, if noise arithmetic dominates, flattening the graph alone won't reach 150 µs, and the noise kernels matter most.

### A flattened program per bound function

To be decided in an ADR after the measurement. The likely shape:

- **Compile, then evaluate.** Binding compiles a `Sampler`'s tree into a `Program`: a list of operations in dependency order over numbered registers.
  - Shared subtrees appear once.
  - Each operation is marked y-dependent or not, so y-independent work runs once per column.
  - The game's compile-time choices stay where they are made today, in binding.
- **Two runners over the same program.** A point runner and a volume runner, each with the arithmetic of its own path. The volume runner works over flat buffers from a per-thread arena that's reused between calls, so a call allocates nothing once warm. This targets the fixed cost per volume.
- **Batched, SIMD-friendly kernels where the arithmetic allows.** Noise octaves, splines, and the clamp and unary operations over whole buffers, written so each lane does exactly the scalar operations.
- **One program form per family.** 26.2's `f64` family compiles to the same kind of program with its own operations, or to a variant the ADR names. The point is to share the runner's structure, not the arithmetic ([ADR 0026](../adr/0026-how-game-versions-coexist.md)).

The interpreter stays only as long as it's useful as a reference for differential tests. ADR 0026's one code path per algorithm means it doesn't stay as a second way to evaluate. The ADR decides whether it survives behind `cfg(test)`.

### Cheaper surface levels and 26.2's columns

- **`find_top_surface`** walks down from an upper bound one `cell_height` step at a time ([density-functions.md](../specs/density-functions.md)). Batch the aquifer's 49 samples into one pass, sharing the work their columns have in common, without changing which values are computed.
- **26.2's columns** each build a `NoiseChunk`. Milestone 08 found that column and chunk answers differ (`flat_cache` is quantized to the quart corner inside the chunk's window), and chose each column's own `NoiseChunk` because the oracle can't tell. Any sharing has to keep each column's answers exactly; the spec's 26.2 section says what may be shared.

### Checking it

- **Differential tests:** the new runners against the interpreter, over every density function of every bundle on the machine, at random points and random volumes (sizes, steps, offsets, including steps that aren't powers of two). This catches a mismatch long before an oracle file does.
- **The oracle:** `cargo xtask oracle-test` for every version, and the test pack, after each phase.
- **The bench:** every phase reports before and after on the four lines the exit criterion names, for each family.

Specs and ADRs that govern this: [density-functions.md](../specs/density-functions.md), [noise.md](../specs/noise.md), [terrain.md](../specs/terrain.md), [oracle.md](../specs/oracle.md), [ADR 0011](../adr/0011-parse-json-numbers-from-their-text.md), [ADR 0022](../adr/0022-terrain-crate-and-volume-evaluation.md), [ADR 0026](../adr/0026-how-game-versions-coexist.md).

## Tasks

### Phase 1: measure and decide

- [ ] Find why the isolated column went from 243 µs (milestone 07's close, 0ef3687) to 393 µs: bench both commits on the same machine in one sitting, then bisect if it's real. Fix it if it's a mistake rather than a cost of supporting versions.
- [ ] Bench lines before, for every supported version: the bench's terrain lines, plus `structure_layout`, confirmed search and region lines, recorded in the notes.
- [ ] A profile of an isolated column, a nearby column, a 40 × 25 region and a village layout, for 26.3 and 26.2, split into tree-walking, allocation, memo, noise, `find_top_surface`, aquifer and the rest.
- [ ] Prototype the hottest part of the breakdown, to check the design reaches 150 µs before building all of it.
- [ ] ADR: the flattened evaluation. Covers the program form, the runners, buffers, which kernels are SIMD and how (auto-vectorization, `std::arch` with runtime detection, or a crate), the families, and what happens to the interpreter.

### Phase 2: the program, 26.3's family

- [ ] Compile a bound `Sampler` into a `Program`, keeping binding's choices; a point runner over it.
- [ ] The volume runner, with an arena of buffers reused between calls; y-independent operations once per column; `upscale`, `interpolated`, `slice`, `domain_warp`, `find_top_surface` and the caches as operations.
- [ ] Differential tests against the interpreter on every bundle's functions, points and volumes.
- [ ] Switch the climate router, terrain, spawn and corner constraints to the program. `oracle-test` passes for 26.3, both 26.4 snapshots and the test pack.

### Phase 3: kernels

- [ ] Noise octaves (`ImprovedNoise`, `SimplexNoise`, the blended noise) over buffers, bit-exact per lane; SIMD where the profile says it pays.
- [ ] Splines, clamps and unary operations over buffers.
- [ ] Bench after each kernel.

### Phase 4: surface levels and the aquifer

- [ ] `find_top_surface` batched across the aquifer's 49 samples.
- [ ] Whatever else the profile shows in the aquifer's scan.
- [ ] The isolated column at or under 150 µs (26.3). Exit criterion 2.

### Phase 5: 26.2's family

- [ ] 26.2's evaluator on the program, with `NoiseChunk`'s caches and interpolation as operations.
- [ ] Columns without a `NoiseChunk` each, where the spec shows the answers are unchanged.
- [ ] Differential tests and `oracle-test` for 26.2.

### Phase 6: close-out

- [ ] Remove the interpreter, or keep it as the ADR says.
- [ ] Bench lines after, for every version, beside the before lines. Exit criterion 3.
- [ ] Specs: density-functions.md, noise.md and terrain.md describe the evaluation where it's observable (never values; those don't change). architecture.md's density row.
- [ ] `cargo xtask check` and `cargo xtask oracle-test` for every version. Exit criterion 1. Move this plan to `completed/`, and milestone 09 with it.

## Verification

- `cargo xtask oracle-test` for every supported version and the test pack, after each phase: probe values, volume digests, heights and block columns, structures and layouts.
- Differential tests against the interpreter, committed without game data (they read local bundles, as the other ignored tests do).
- Hand-worked tests for each kernel's edge cases: NaN, signed zeros, the ends of the `int` range, and steps that aren't powers of two.
- The bench before and after each phase, for each family.

## Durable outputs

- [ ] ADR: the flattened density evaluation.
- [ ] `docs/specs/density-functions.md`, `docs/specs/noise.md`, `docs/specs/terrain.md`: where evaluation order or sharing is observable.
- [ ] `docs/architecture.md`: the density row.

## Decisions

Open for the user, to settle in phase 1's ADR:

- **How SIMD is done.** Researched on 2026-10-27 (see [SIMD options](#simd-options)). The lead candidate is `fearless_simd`, unless the profile shows the column's time is mostly tree-walking and allocation. In that case, auto-vectorized loops with `multiversion` may be enough and add no SIMD API.
- **Whether the interpreter survives as a test reference** (behind `cfg(test)`) after the switch, or is deleted.
- **A target for 26.2.** The milestone asks that every family be covered but sets no number. Proposed: 26.2's isolated column within 2× of 26.3's.

### SIMD options

What spyglass needs:

- stable Rust within the MSRV (1.95), since `std::simd` is still nightly-only;
- runtime CPU dispatch, because release builds target baseline x86-64 (SSE2);
- the same bits at every instruction level, so plain IEEE add, multiply and divide only, never a fused multiply-add;
- Java's `Math.min`/`max`, `Mth.floor` and `(int)` casts, which we write ourselves with any crate;
- the noise tables' gathers, which are the hard part.

| Option                                          | Dispatch at run time                 | Assessment                                                                                                                                                                                                                                 |
| ----------------------------------------------- | ------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `fearless_simd` 1.1 (1.2.0 released 2026-10-25) | Yes: `Level`, `#[simd]`, `dispatch!` | Best fit. MIT/Apache-2.0, MSRV 1.89, no dependencies beyond an optional `libm`. Safe API over SSE2, SSE4.2, AVX2, AVX-512, NEON, WASM and a scalar fallback. Fixed-width `f64x4` and native-width types, and `kernel!` for raw intrinsics. |
| `wide` 1.7                                      | No                                   | Mature and widely used, but gets AVX2 only with `-C target-cpu`, so generic release builds would stay at SSE2.                                                                                                                             |
| `pulp` 0.22                                     | Yes                                  | Built for faer. Native-width API only, undocumented fixed widths, verbose dispatch, pre-1.0, no stated MSRV.                                                                                                                               |
| `macerator` 0.5                                 | Yes                                  | Built for burn. Thin documentation, no safe intrinsics, AVX-512 needs nightly.                                                                                                                                                             |
| Auto-vectorization with `multiversion` 0.9      | Yes, via `multiversion`              | A credible baseline: plain loops and no SIMD API, but whether a loop vectorizes can change between compiler versions.                                                                                                                      |
| `std::arch`                                     | Write it yourself                    | Last resort: unsafe code for each platform.                                                                                                                                                                                                |

Findings that bear on bit-exactness:

- **`fearless_simd`'s `mul_add` may be fused or not, by platform**, so it must never be used. `to_int_precise` saturates and maps NaN to 0, as Java's `(int)` does. Its `min`/`max` don't document NaN and signed-zero behavior, so Java's would be built from compares and selects.
- **No crate offers gather, and hardware gather wouldn't help.** AMD Zen has no native gather, and LLVM's gathers have measured 1.75 to 4 times slower than scalar loads. The noise kernels should look up tables per lane in scalar code, and vectorize the gradient, fade and lerp arithmetic.
- **To confirm:** whether `fearless_simd` can force a lower `Level`. If it can, the differential tests can check that the scalar, SSE2, AVX2 and NEON paths give identical bits.

Housekeeping:

- Every candidate's license is already allowed in `deny.toml`.
- `fearless_simd` 0.4.1 is already in the tree through the viewer (`epaint` → `vello_cpu`), so 1.x would be a second copy, visible only in the viewer.
- Pin a release at least 7 days old, as Dependabot's cooldown does for actions: 1.1.x until 1.2.0 has aged.

Sources:

- [The state of SIMD in Rust in 2026](https://shnatsel.github.io/state-of-simd-rust-2026/), by a `fearless_simd` maintainer;
- the [`fearless_simd` 1.1.0 documentation](https://docs.rs/fearless_simd/1.1.0);
- crates.io metadata for each crate, fetched 2026-10-27.

## Notes

- 2026-10-27: Researched the SIMD options for stable Rust; findings in [Decisions](#simd-options).
- 2026-10-27: Drafted from the code at c98dbeb, today's bench of all four versions and milestone 07's measurements.
