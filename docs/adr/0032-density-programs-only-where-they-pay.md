# 0032. Density programs only where they pay

- Status: Accepted
- Date: 2026-10-28
- Supersedes: [0031](0031-flattened-density-evaluation.md)

## Context

[ADR 0031](0031-flattened-density-evaluation.md) decided to compile every bound density function into a flat program, with point and volume runners, for both evaluator families, keeping the interpreter only as a test reference. Milestone 09 then built and measured it (`dist` build, coarse timing; the m09 plan in `docs/plans/`).

- **26.2** recomputed shared subgraphs at every point and had no memo at all. A per-context memo for shared functions, a per-column memo for functions that depend only on the column, stamps shared by a chunk's corners, a compiled program for the chunk's graph, and per-seed cell corners took its isolated column from 1.9 to 2.4 ms to about 600 µs, and village layouts from 111 to 169 ms to 15 to 21 ms. The program itself gave 4 to 9% once the memos were in; the memos and the shared corners gave the rest.
- **26.3** already had a memo, and its interpreter's overhead turned out to be its copies, not its tree-walking. Keeping only shared nodes in the volume memo halved a region's time, and per-seed corner caches made repeated layouts nearly ten times faster. Its point path already borrows the memo once per sample, so a program would mostly inline constants: a few percent, by phase 1's measurements, for a rewrite of about 3,000 lines of point and volume evaluation.
- **SIMD** didn't pay: a bit-identical vectorized octave loop was slower except at the lowest frequencies, with or without AVX2.

## Decision

We will compile bound functions into programs only where measurements show the interpreter's structure is the cost:

- **26.2's chunk graph is a program** (`v1_21::program`), as ADR 0031 describes, with the bound functions kept as its reference in a differential test (`the_program_matches_the_functions`). 26.2's plain router and climate functions, off the hot path, stay bound functions.
- **26.3's family keeps its interpreter as the evaluator**, with the measured fixes: the volume memo keeps only shared nodes, and `interpolated` and `upscale` keep their position-only inputs' corners per seed. It gets a program only if a later profile shows tree-walking is the cost.
- **Results shared across queries must be exact by construction**, argued in the spec: a value is kept only where it depends on its position alone (26.2's corners, 26.3's position-only corners), never where the volume's extent can change it.
- ADR 0031's other choices stand: no SIMD dependency, and speed measured with the `dist` profile.

## Consequences

- **Two evaluation styles coexist**: 26.2's program and 26.3's interpreter. Each is the faster one for its family; new density types reach the interpreter as before, and 26.2's program only when 26.2's family gains them, which it won't.
- **No differential test for 26.3**, since nothing replaces its evaluator; the oracle comparisons and its hand-worked tests remain its checks.
- **Per-seed caches use memory**: up to about 15 MB of 26.2 corners and about 1 MB per 26.3 interpolator per seed, each cleared when full.
- **The isolated column's target became 235 µs** (met: 227 to 233 µs, against about 500 µs in the game), not milestone 09's original 150 µs, which would need fewer positions evaluated than the game evaluates.

## Alternatives considered

- **Build 26.3's program anyway, as ADR 0031 decided:** a few percent for a large rewrite and a second evaluator to keep exact.
- **Drop 26.2's program, keeping only its memos:** it's already built and tested, it's 4 to 9% on every 26.2 line, and the reference test keeps it honest.
