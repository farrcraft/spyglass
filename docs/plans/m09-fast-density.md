# m09. Fast density evaluation

- Status: In progress
- Milestone: [09. Fast density evaluation](../roadmap/09-fast-density.md)
- Started: 2026-10-27
- Completed: not yet

## Goal

Replace the density graph interpreter with a flattened evaluation, through both the point and the volume paths, for every evaluator family. Values must stay the same bit for bit, and terrain must cost well under the game's. This covers all three of the milestone's exit criteria:

- **Every density, height and structure oracle comparison still passes**, for every supported version.
- **`height_at` for one isolated column in a bound session takes at most 150 µs** (the `dist` profile, one thread, 26.3).
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

The interpreter stays as the reference for differential tests, behind `cfg(test)` or a test-only feature. ADR 0026's one code path per algorithm means it never evaluates outside tests.

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

- [x] Find why the isolated column went from 243 µs (milestone 07's close, 0ef3687) to 393 µs: bench both commits on the same machine in one sitting, then bisect if it's real. Fix it if it's a mistake rather than a cost of supporting versions. Mostly the bench's noise; the real regression is codegen, not code (see the notes). Fixed by measuring with a `dist` profile (fat LTO, one codegen unit), decided 2026-10-27.
- [x] Bench lines before, for every supported version: the bench's terrain lines, plus `structure_layout`, confirmed search and region lines, recorded in the notes. Three rounds per version under `dist`; ranges in the notes.
- [ ] A profile of an isolated column, a nearby column, a 40 × 25 region and a village layout, for 26.3 and 26.2, split into tree-walking, allocation, memo, noise, `find_top_surface`, aquifer and the rest. Done for both families (notes), with timing spans behind a `timing` feature and the `density_timing` example, since this machine has no profiler that runs unelevated.
- [x] Prototype the hottest part of the breakdown, to check the design reaches 150 µs before building all of it. Two prototypes (notes): a vectorized noise kernel (no), and the point path's split inside `find_top_surface`. Projection: about 200 to 220 µs from removing the interpreter's overhead; 150 µs would need more (see phase 4).
- [x] ADR: the flattened evaluation. Covers the program form, the runners, buffers, which kernels are SIMD and how (auto-vectorization, `std::arch` with runtime detection, or a crate), the families, and what happens to the interpreter. [ADR 0031](../adr/0031-flattened-density-evaluation.md), accepted: no SIMD dependency for now.

### Phase 2: the program, 26.2's family

Reordered on 2026-10-27, with the user: 26.2 first, since its point evaluations have no memo and its gains are several-fold, against about 8 to 10% for 26.3's (see the notes).

- [ ] 26.2's evaluator on the program, with `NoiseChunk`'s caches and interpolation as operations, and its single-point evaluations sharing subgraphs. First step done: a per-context memo for shared functions, shared corner and point stamps, and a per-column memo for columnar functions (e657cae, f61f361). The program itself is next.
- [ ] Columns without a `NoiseChunk` each, where the spec shows the answers are unchanged.
- [ ] Differential tests against the interpreter on 26.2's functions, points and cells; `oracle-test` for 26.2.
- [ ] Measure 26.2's lines against its before lines and its target (within 2× of 26.3's isolated column).

### Phase 3: kernels

- [ ] Noise octaves (`ImprovedNoise`, `SimplexNoise`, the blended noise) over buffers, bit-exact per lane; SIMD where the profile says it pays.
- [ ] Splines, clamps and unary operations over buffers.
- [ ] Bench after each kernel.

### Phase 4: surface levels and the aquifer

- [ ] `find_top_surface` batched across the aquifer's 49 samples, with the point path's arithmetic.
- [ ] Prototype a per-seed cache of y-independent values at quart positions, shared between the volume path's cell corners and `find_top_surface`'s points, and across columns. First show the two paths give the same bits for those nodes; then measure nearby columns, regions, layouts and the isolated column.
- [ ] Whatever else the profile shows in the aquifer's scan.
- [ ] The isolated column at or under the target set after phase 5 (26.3). Exit criterion 2.

### Phase 5: the program, 26.3's family

- [ ] Compile a bound `Sampler` into a `Program`, keeping binding's choices; a point runner over it.
- [ ] The volume runner, with an arena of buffers reused between calls; y-independent operations once per column; `upscale`, `interpolated`, `slice`, `domain_warp`, `find_top_surface` and the caches as operations.
- [ ] Differential tests against the interpreter on every bundle's functions, points and volumes.
- [ ] Switch the climate router, terrain, spawn and corner constraints to the program. `oracle-test` passes for 26.3, both 26.4 snapshots and the test pack.

- [ ] Measure every line against the before lines, then decide the isolated column's target with the user (agreed 2026-10-27: decided after the programs are built, not before).

### Phase 6: close-out

- [ ] Move the interpreter behind `cfg(test)` or a test-only feature, as the differential tests' reference.
- [ ] Bench lines after, for every version, beside the before lines. Exit criterion 3.
- [ ] Specs: density-functions.md, noise.md and terrain.md describe the evaluation where it's observable (never values; those don't change). architecture.md's density row.
- [ ] `cargo xtask check` and `cargo xtask oracle-test` for every version. Exit criterion 1. Move this plan to `completed/`, and milestone 09 with it.

## Verification

- `cargo xtask oracle-test` for every supported version and the test pack, after each phase: probe values, volume digests, heights and block columns, structures and layouts.
- Differential tests against the interpreter, committed without game data (they read local bundles, as the other ignored tests do).
- Hand-worked tests for each kernel's edge cases: NaN, signed zeros, the ends of the `int` range, and steps that aren't powers of two.
- The bench before and after each phase, for each family.

## Durable outputs

- [x] ADR: the flattened density evaluation ([0031](../adr/0031-flattened-density-evaluation.md)).
- [ ] `docs/specs/density-functions.md`, `docs/specs/noise.md`, `docs/specs/terrain.md`: where evaluation order or sharing is observable.
- [ ] `docs/architecture.md`: the density row.

## Decisions

Agreed on 2026-10-27: the interpreter stays as a test reference, 26.2's target is as proposed, and measurements use a `dist` profile. The SIMD approach is settled in phase 1's ADR.

- **How SIMD is done.** Researched on 2026-10-27 (see [SIMD options](#simd-options)). The lead candidate is `fearless_simd`, unless the profile shows the column's time is mostly tree-walking and allocation. In that case, auto-vectorized loops with `multiversion` may be enough and add no SIMD API.
- **The interpreter survives as a test reference** (behind `cfg(test)` or a test-only feature) after the switch, for the differential tests. It never evaluates outside tests.
- **Measurements use the `dist` profile:** release with fat LTO and one codegen unit, so speed depends on the code, not on how crates are split (see the notes). `cargo xtask bench`, the exit criterion and binaries for users use it; tests stay on release.
- **A target for 26.2:** its isolated column within 2× of 26.3's.
- **The isolated column's target is decided after the programs are built** (2026-10-27), with the flattened program measured, since phase 1's projection is 200 to 220 µs against the milestone's 150 µs. The milestone asks that every family be covered but sets no number.

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

- 2026-10-27: **26.2 after its memo** (`dist`, coarse timing, quiet machine, three rounds): isolated column 635 to 646 µs (1.9 to 2.4 ms before), nearby column 344 to 351 µs (490), warm village layout about 50 ms (67), region about 75 ms (73, unchanged). oracle-test passes for 26.2 after each step.
  - **What did it.** A function the binder sees bound twice is remembered per evaluation context. Corners of a `NoiseChunk` keep a stamp each, so the functions interpolated at a corner share their common work. Single-point computes at one block share a stamp. Columnar functions (no y, and only `cache_2d` or `flat_cache` markers) are remembered per column, which matches the game: its `cache_2d` returns the value for the last column asked, and the aquifer's `find_top_surface` walks (7 or 8 steps each) were recomputing it at every step.
  - **What's left.** An isolated column evaluates about 16,900 nodes, most of them small arithmetic nodes, at about 1,000 points (cell corners and walk steps); noise is the minority. At 5 to 10 ns of call and memo overhead per node, the flattened program should take off about 100 µs, which would put 26.2 near its target (2× 26.3's isolated column, about 500 to 530 µs in the same build).
  - **A clippy fix on the way:** 26.2's bound function grew, which made the spawn search's enum trip `large_enum_variant`; its climate sampler is boxed.

- 2026-10-27: **26.3's volume memo is essential; the gain left is in its copies.** Skipping the volume path's memo store (values are unchanged, since recomputing a node gives the same bits) makes the isolated column 248 → 457 µs, a nearby column 188 → 396 µs, a 40 × 25 region 23 → 28.6 ms, and a warm village layout 30 → 58 ms. Shared subgraphs, including ones shared between interpolated corner volumes, are most of the work saved. What a program can still remove in 26.3's volume path is the memo's copying (13 to 19 µs per column) and per-child allocations (a few µs): about 8 to 10% of a column. 26.2's single-point router evaluations have no memo at all, which is where several-fold gains are.

- 2026-10-27: **The point path's split inside `find_top_surface`.** A point evaluation of its density visits about 40 nodes: about 6 shifted-noise samples, 2 shift samples, 2 splines, and arithmetic nodes and constants. A constant costs about 30 ns per visit, which is all tree-walking and memo overhead. So perhaps 0.2 to 0.4 µs of each 1.24 µs point evaluation is overhead a flattened program removes; the rest is noise and spline arithmetic. Batching the 49 positions can't skip evaluations: each needs one, with the point path's arithmetic.
  - **Projection for 26.3's isolated column** (about 266 µs now): about −20 µs from a flattened point program, about −25 µs from the volume path without memo copies and per-node allocations, little from batching or SIMD noise. That gives about 200 to 220 µs, 2.3 to 2.5 times faster than the game, but not 150 µs.
  - **What could go further**: the `find_top_surface` positions are on the quart grid that `final_density`'s cell corners use, and the y-independent work (continents, erosion and ridges noise, the shaping splines) at those positions could be computed once per seed and shared between both paths and across columns. Unproven: it needs the two paths to give the same bits for those nodes. It helps nearby columns, regions and layouts more than isolated columns. Phase 4 prototypes it.
  - **Decided with the user**: build phase 2, measure, and settle the target then.

- 2026-10-27: **Prototype: a batched noise kernel doesn't pay.** 26.3's per-octave volume loop (`PerlinNoise::add_to_volume`), rewritten to compute 8 y positions' fractions first and then their smoothsteps and `lerp3`s in a fixed-width loop the compiler can vectorize, with the same operations in the same order. Bit-identical over 106,112 values (four scales, three volume shapes, 16 noises). Against the current loop:

  | Shape                         | Lowest frequency (1/1024) | Mid (1/64) | High (1/4, 1) |
  | ----------------------------- | ------------------------- | ---------- | ------------- |
  | Block column, 1 × 384 × 1     | 1.33×                     | 1.18×      | 0.70 to 0.74× |
  | Cell corners, 1 × 49 × 1      | 1.24×                     | 0.69×      | 0.74×         |
  | A chunk's corners, 5 × 49 × 5 | 1.24×                     | 0.71×      | 0.75×         |

  Sharing the 8 corner gradients when all lanes are in one y cell is what makes the lowest frequency faster; copying them per lane otherwise costs more than the vector lerps save. Built for this CPU (`-C target-cpu=native`, AVX2 on a Ryzen 7 5800X) it's no better, and worse at high frequencies. The current loop already costs about 7 ns per value, mostly in per-value `f64` work (wrap, floor, conversions) and table lookups, which don't vectorize. So SIMD noise isn't the 2× the plan counted on. The prototype is kept outside the tree.
  - **What this changes.** The 150 µs target has to come mostly from structure, not SIMD: fewer point evaluations for the aquifer's surface levels (up to 29% of an isolated column), no interpreter copies and allocations (10 to 20%), and computing y-independent work once for many positions. Noise kernels can still win where corners are shared (the lowest-frequency octaves, about 1.3×), as a later refinement. The ADR should choose no SIMD dependency for now.

- 2026-10-27: **Where 26.2's time goes** (coarse spans in its `NoiseChunk`).

  | Scenario                     | Total         | Biggest cost                                                                          |
  | ---------------------------- | ------------- | ------------------------------------------------------------------------------------- |
  | Isolated column, bound       | 1.9 to 2.4 ms | 78% in 126 single-point router computes for the aquifer, about 12 µs each             |
  | Nearby column, bound         | 490 µs        | 59% in cell corners (`fillSlice`, 733 per column), 29% in point computes              |
  | 40 × 25 region               | 73 ms         | per-block interpolation and corners (timing's own cost is 21 ms of it: 316,000 spans) |
  | Village layout, same session | 67 ms         | 70% in cell corners                                                                   |

  What they show:

  - **An aquifer point compute is 8 times 26.3's**: about 12 µs against 1.5 µs. It runs a router entry through the wrapped graph at a single point, with no memo, so shared subgraphs are recomputed. The isolated column's surface levels and barrier and fluid-level samples go this way.
  - **Corners are the volume work.** Each `fillSlice` corner is a point evaluation of `final_density`'s inputs, about 0.4 µs. A program that evaluates corners over a cell's slice at once, as 26.3's volume path does, is the family's counterpart to 26.3's volume runner.
  - **For the 2× target** (26.2's isolated column at most twice 26.3's, so about 300 µs or less), the aquifer's point computes must mostly go: batched over the 49 surface-level positions, and with shared subgraphs computed once.

- 2026-10-27: **Where 26.3's time goes** (`cargo run --profile dist -p spyglass --features timing --example density_timing -- 26.3`). Coarse spans cost about 2% and give the split between stages; a span per node shows the split inside volumes but slows everything (and with about 100,000 spans per village layout, distorts it). Run-to-run noise on this machine is large, so these are shares, consistent across runs:

  | Scenario (coarse)               | Density volumes | `find_top_surface`'s point samples | Rest |
  | ------------------------------- | --------------- | ---------------------------------- | ---- |
  | Isolated column, bound (266 µs) | 66%             | 29% (62 point evaluations)         | 5%   |
  | Nearby column, bound (216 µs)   | 84%             | 9%                                 | 7%   |
  | 40 × 25 region                  | 81%             | 5%                                 | 14%  |
  | Village layout, same session    | 75%             | under 1%                           | 25%  |

  What the per-node spans and the totals show:

  - **Inside volumes, noise is about half.** Plain noise and the smeared (blended) noise together are 45 to 57% of exclusive time in nearby columns, regions and layouts. Splines are 3 to 8%, and each arithmetic node type is a few percent.
  - **The interpreter's own cost is 10 to 20%.** Memo stores, which copy each shared node's whole volume, are 5 to 15%, highest in regions; per-call allocation and setup a few percent more.
  - **What it takes to reach 150 µs from about 270 µs (−45%).** Batching `find_top_surface`'s point samples into volumes (up to −29% of an isolated column), noise kernels about twice as fast (about −25%), and the flattened program without per-node copies and allocations (about −10%). Flattening alone falls short, so the noise kernels are needed. That settles the SIMD question towards `fearless_simd` over plain auto-vectorization, to be confirmed by phase 1's prototype.
  - **Cold village layouts** (a new session each time) measured between 110 and 307 ms across passes. The noise drowns the split between density and the rest, which needs the per-seed state that a new session builds. Measure again with a quieter machine before drawing conclusions.

- 2026-10-27: **Bench lines before** (`dist`, three rounds per version; ranges). 26.4-snapshot-6's rounds ran while the machine was busier, so its ranges are wide.

  | Line                                     | 26.2             | 26.3               | 26.4-snapshot-5    | 26.4-snapshot-6    |
  | ---------------------------------------- | ---------------- | ------------------ | ------------------ | ------------------ |
  | `height_at` one column, bound            | 2.0 ms           | 296 to 313 µs      | 254 to 399 µs      | 233 to 403 µs      |
  | `height_at` one nearby column, bound     | 506 to 600 µs    | 197 to 230 µs      | 194 to 305 µs      | 191 to 289 µs      |
  | `height_at` one column, new session      | 1 to 2 ms        | 598 to 692 µs      | 728 to 923 µs      | 698 to 964 µs      |
  | `heights_in` 40 × 25 columns             | 67 to 74 ms      | 20 to 22 ms        | 26 to 35 ms        | 24 to 32 ms        |
  | `height_map` 256², scale 1               | 4.4 to 5.5 s     | 1.4 to 1.5 s       | 1.6 to 2.5 s       | 1.3 to 2.8 s       |
  | `height_map` 256², scale 4               | 28 to 36 s       | 10.8 to 11.8 s     | 13.0 to 15.8 s     | 11.3 to 28.0 s     |
  | `structure_layout` village_plains, bound | 111 to 169 ms    | 80 to 85 ms        | 73 to 91 ms        | 83 to 116 ms       |
  | witch huts confirmed, 1 thread           | 400 to 473/s     | 3,204 to 3,312/s   | 2,166 to 3,316/s   | 2,324 to 4,052/s   |
  | witch huts confirmed, all threads        | 3,984 to 4,234/s | 26,824 to 28,201/s | 15,878 to 27,602/s | 17,074 to 30,264/s |
  | `biome_at` 256² quarts                   | 676 to 754 ms    | 194 to 233 ms      | 194 to 216 ms      | 162 to 341 ms      |
  | `render` 1024² at scale 16               | 12.3 to 13.2 s   | 3.2 to 3.7 s       | 3.1 to 5.3 s       | 2.4 to 7.0 s       |

- 2026-10-27: **The 243 → 393 µs question.** Timed with a throwaway program (400 isolated columns, 400 nearby, 30 village layouts per run), alternating builds over several rounds, since this machine (Balanced power plan, a browser open) drifts by tens of percent between runs.
  - **The isolated column barely moved.** Medians of 246 to 278 µs at 0ef3687 against 268 to 277 µs at HEAD, and minimums of 186 against 208 µs. The bench's 393 µs was its 20-sample median of a quantity that varies by ±30%. The target needs about 1.8× from today's typical column, not 2.6×.
  - **Jigsaw layouts got 55 to 70% slower.** A plains village went from 66 to 79 ms to 114 to 127 ms. Bisected to 5f9d4ac, "Add 26.2's f64 noise family", which changed no 26.3 code on the sampling path.
  - **The cause is codegen.** With fat LTO and one codegen unit, both commits measure the same (columns from 180 µs minimum, layouts 75 to 80 ms). Under the default profile (16 codegen units, no cross-crate LTO), builds with identical settings but different metadata hashes split the noise and density crates differently. Some splits inline the hot noise functions and some don't. The standard `target/release` build happened to get a slow split after 5f9d4ac; another target directory, or thin LTO, got a fast one for the timing program but a slow one for the bench. So today's numbers say as much about the luck of the split as about the code.
  - **Fat LTO with one codegen unit is fast and stable** in both binaries. Bench, two rounds against the default: layouts 75 to 83 ms against 103 to 114 ms; the isolated column 235 to 240 µs against 264 to 291 µs; everything else the same or better. It costs build time: rebuilding after touching `spyglass-density` takes 29 s against 10 s. Every release test binary that `oracle-test` and `ci` build would pay that.
  - **Consequences for this plan.** Measurements must use a build whose inlining doesn't depend on the split, or they measure noise. The hot path itself should be written so its speed doesn't rely on lucky inlining: explicit `#[inline]` on small hot functions, and the flattened kernels in one place.

- 2026-10-27: Researched the SIMD options for stable Rust; findings in [Decisions](#simd-options).
- 2026-10-27: Drafted from the code at c98dbeb, today's bench of all four versions and milestone 07's measurements.
