# 0031. Flattened density evaluation

- Status: Accepted
- Date: 2026-10-27
- See also: [0011](0011-parse-json-numbers-from-their-text.md), [0022](0022-terrain-crate-and-volume-evaluation.md), [0026](0026-how-game-versions-coexist.md)

## Context

Terrain is density evaluation. Every height, block column, confirmed structure start and jigsaw layout samples a dimension's `final_density`, and its aquifer samples more. `spyglass-density` evaluates a bound function as a tree interpreter. A `Sampler` is an `Arc<Node>` tree that binding builds, making the game's compile-time choices. Two paths walk it:

- the point path, with a per-thread memo so a shared node is computed once per position and a y-independent node once per column;
- the volume path, which fills each node over a whole volume, children first, allocating a buffer per node per call and copying shared nodes' values into a memo.

26.2's `f64` family ([ADR 0026](0026-how-game-versions-coexist.md)) is a second interpreter, evaluated through a `NoiseChunk` with no volume path.

Milestone 09 asks for terrain cheap enough to use freely, without changing a bit. Phase 1 of its plan measured where the time goes (`docs/plans/`, m09-fast-density's notes), with a `dist` build (fat LTO, one codegen unit). Under the default release profile, how the compiler happened to split crates changed speed by up to 1.7×, which drowned real differences.

- **26.3's isolated column** (about 266 µs):
  - two thirds in density volumes, half of that noise;
  - 29% in `find_top_surface`'s 62 point evaluations for the aquifer's surface levels, each about 1.24 µs;
  - the interpreter's own copying, allocation and tree-walking, 10 to 20%.
- **26.2's isolated column** (about 2 ms): 78% in 126 single-point router evaluations at about 12 µs each, which recompute shared subgraphs because that path has no memo.

What makes this hard:

- **Bit-exactness.** Each value must come from the same IEEE operations in the same order as the game. The point and volume paths have different arithmetic in places (noise stacks scale differently, `upscale` and `interpolated` interpolate differently), so they can't be merged.
- **Two families.** 26.2's evaluation is a different algorithm throughout.
- **SIMD helps less than expected.** A bit-identical prototype of 26.3's per-octave noise loop batched 8 heights at a time for the compiler to vectorize. It was 1.2 to 1.3× faster at the lowest frequencies and 0.7× elsewhere, and building it for AVX2 didn't help. The loop is bound by per-value `f64` work and table lookups, not by arithmetic width. Stable Rust has no `std::simd`; the stable crates were surveyed in the plan.

## Decision

We will compile each bound density function into a flat **program** and evaluate it with **runners**, replacing the interpreter outside tests:

- **The program.** Binding still makes the game's compile-time choices, then lowers the tree to a list of operations in dependency order over numbered registers:
  - a shared subtree appears once;
  - constants are operands, not nodes;
  - each operation is marked y-dependent or not.
- **Two runners per program**, each with its own path's arithmetic, operation by operation as today:
  - a point runner;
  - a volume runner.
- **Buffers come from a per-thread arena reused between calls**, so a warm volume call allocates nothing and shares values without copying. Y-independent operations run once per column of a volume.
- **One program form per family.** 26.2's `f64` family gets its own operations, including `NoiseChunk`'s caches and interpolation, and a runner of the same structure. The families share the program's shape and the runners' machinery, never arithmetic.
- **No SIMD dependency for now.** Kernels are plain Rust written so they could vectorize. A SIMD crate (`fearless_simd` was the leading candidate) is reconsidered only if a later profile shows a kernel bound by arithmetic width.
- **The interpreter stays as the reference** for differential tests, behind `cfg(test)` or a test-only feature. It never evaluates outside tests.
- **Speed is measured with the `dist` profile**, so a result reflects the code rather than how the compiler split crates.

## Consequences

- **What it should buy:**
  - The interpreter's overhead goes: tree-walking, per-node allocation and memo copies.
  - 26.2's point evaluations stop recomputing shared subgraphs, which is most of its isolated column's cost.
  - Phase 1 projects 26.3's isolated column at about 200 to 220 µs, against the milestone's 150 µs. The target is revisited after the programs are built and measured.
- **Two evaluators exist during the change.** The differential tests compare the runners with the interpreter on every bundle's functions, at random points and volumes, before switching. The interpreter's continued presence is a maintenance cost, accepted for the test coverage it gives.
- **New forms cost a little more.** Each density type is now an operation in each runner it can reach, plus its lowering. A new type in a future version means touching the program as well as the loader.
- **Bit-exactness stays per operation.** Each operation must perform its node's exact arithmetic. The flattening itself changes only when operations run, never which run or in what order within a value, as the memo already did.
- **Batching doesn't remove work.** `find_top_surface`'s 49 aquifer positions each still need a full point evaluation. Removing that work needs sharing y-independent values across paths or columns, which the plan prototypes separately and may need its own decision.
- **SIMD is left on the table for now**, where the profile shows little to gain. If noise becomes the bottleneck after flattening, this is revisited, with the plan's survey as the starting point.

## Alternatives considered

- **Keep the interpreter and tune it** (cheaper memo, fewer `Arc`s): smaller changes, but the overhead is structural (a call and a memo check per node, per position or per volume), and 26.2's interpreter would need a memo of its own anyway.
- **SIMD noise kernels through a crate** (`fearless_simd`, `wide`, `pulp`) **or auto-vectorization with `multiversion`**: the prototype showed the noise loop isn't width-bound, so a dependency would add risk (unfused multiply-adds, per-level testing) for little gain now.
- **Generate code** (JIT, or Rust generated per function at build time): the fastest evaluation, but functions come from bundles at run time, a JIT is a large dependency with unsafe code, and generated code would be far harder to keep bit-exact and to review.
- **One arithmetic for both paths**: simpler, but not bit-exact. The game's point and volume paths round differently, and both are observable (the aquifer and `find_top_surface` sample points; columns and chunks sample volumes).
