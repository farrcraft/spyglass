# 21. Faster queries

- Status: Completed
- Depends on: [19](19-features.md)
- Plans: [m21-faster-queries](../../plans/completed/m21-faster-queries.md)

## Goal

Single queries and searches take less time on the same machine, with the same answers bit for bit. Milestones [09](09-fast-density.md) and [14](14-viewer-performance.md) made terrain and the map's tiles cheaper. What's slow now:

- chunk work that looks up biomes more often, and from further away, than it needs to;
- single queries that use one core while the rest sit idle;
- searches that bind and sample biomes in full for every seed;
- a build that leaves gains on the table which can't change arithmetic.

GPU computation and SIMD are assessed and the result recorded, not built.

## Where the time goes

`cargo xtask bench`, `dist` build, on the dev machine (Ryzen 7 5800X, 8 cores, 16 threads), three rounds on a quiet machine, 2026-11-06 (the plan has more lines and 26.2's):

| Line (26.3)                                 | Time or rate            |
| ------------------------------------------- | ----------------------- |
| `locate_structure` stronghold, new session  | 0.97–1.37 s             |
| `nearest_feature monster_room`, new session | 2.52–3.11 s             |
| 128 × 128 dungeon tile at 1:2               | 11.9–13.9 s             |
| 256 × 256 height map at 1:4                 | 5.01–6.56 s             |
| Biome presence within 500, one evaluation   | 6.99–7.46 ms            |
| Witch-hut search, 1 thread                  | 3,125–4,251 seeds/s     |
| Witch-hut search, all threads               | 28,911–31,988 seeds/s   |
| Placement prefilter rejecting everything    | 257–384 billion seeds/s |

- The search already scales about 8× on 16 threads. Within a single query, nothing runs in parallel: rayon is used only by the search's runner.
- Biome lookup (the parameter search and the zoom) is the largest single cost in chunk work, ahead of noise: about 40% of a dungeon tile ([the plan's profiles](../../plans/completed/m21-faster-queries.md#notes)).

## Scope

- **A profiler that works on the dev machine.** Milestone 09 had to time with spans because no profiler ran unelevated. The fixes below are chosen from a real profile of a carved chunk, a dungeon tile and a biome-presence evaluation.
- **The 1:4 height map** checked against milestone 14's close: no regression, as it turned out.
- **Cheaper biome lookups**, each giving answers identical by construction:
  - the parameter search starting from a nearby quart's answer instead of from nothing;
  - each quart looked up once per query, not once per chunk window that covers it;
  - the zoom's fiddles computed once per quart, not once per block.
- **Threads within a query**, where each part's answer is independent of the others:
  - stronghold ring positions, whose biome searches each draw from their own fork of the ring's random;
  - `nearest_feature`'s rings of chunks and `features_in`'s chunks.
  - The same treatment applies to any later query of the same shape, such as loot's nearest container once milestone 20 has it.
  - An ADR says how library calls use threads inside a search's pool and the viewer's tile workers without fighting [ADR 0046](../../adr/0046-the-viewers-search-yields-to-the-map.md)'s throttle.
- **Build-level gains** that can't change arithmetic, since Rust never applies fast-math optimizations: profile-guided optimization, a global allocator and a faster hasher for the hot maps. Each is kept only where the bench shows it beats noise, and without breaking reproducible release builds ([ADR 0038](../../adr/0038-how-release-archives-are-built.md)).
- **Fewer full biome samples in searches.** Each octave is seeded by its own name, so a noise's low octaves can be built alone, giving the same values. The high octaves' contribution is bounded by their amplitudes. Together they give an interval for each climate value, and a stage that rejects seeds whose intervals can't reach the target biomes. That is a necessary condition, as [ADR 0021](../../adr/0021-search-engine.md) requires. Built only if a prototype shows it pays.
- **GPU and SIMD, assessed and recorded** in `docs/research/`, with the roadmap's GPU stretch goal updated to match.

Not in scope:

- Anything that changes results: every change matches the game exactly, and the oracle comparisons confirm it.
- GPU computation and SIMD dependencies. [ADR 0032](../../adr/0032-density-programs-only-where-they-pay.md) stands unless the profile shows a kernel bound by arithmetic width.
- The viewer's tile scheduling, which was milestone 14's.
- 26.2's carved chunks (about 100 ms, against 17 to 30 ms in 26.3), unless the profile shows a fix that's cheap.

## Results

Baseline (848fcfd) against after, `dist` builds back to back on the dev machine, five alternating rounds, 2026-11-08, minimum and median. The machine was busier than for the baseline above (NordVPN and a browser each keeping a core busy), so the baseline's own times came out higher, and the ratios say more than the times:

| Line                                        | 26.3                             | 26.2                             |
| ------------------------------------------- | -------------------------------- | -------------------------------- |
| `locate_structure` stronghold, new session  | 0.97 and 1.00 → 0.12 and 0.12 s  | 2.17 and 2.56 → 0.26 and 0.30 s  |
| `nearest_feature monster_room`, new session | 3.97 and 4.41 → 0.89 and 1.06 s  | 16.6 and 23.1 → 4.05 and 5.39 s  |
| 128 × 128 dungeon tile at 1:2               | 11.0 and 11.6 → 1.82 and 2.01 s  | 53.4 and 55.3 → 8.71 and 8.86 s  |
| The same tile, one thread                   | 11.5 and 12.5 → 7.01 and 10.8 s  | 49.9 and 61.8 → 39.5 and 40.1 s  |
| `chunk_blocks` with carvers, bound (5 × 40) | 18.1 and 18.6 → 15.0 and 16.2 ms | 96.6 and 127 → 96.6 and 103 ms   |
| Biome presence within 500 (5 × 20)          | 7.03 and 7.63 → 6.15 and 6.47 ms | 16.3 and 18.3 → 15.0 and 15.9 ms |

What they show:

- **Met:** the stronghold, at an eighth of the baseline, from settling ring points in parallel.
- **Met at the minimum, not the median:** `nearest_feature` (1 s target; rounds of 0.89 to 1.27 s), the one-thread tile (9 s; 2 of 5 rounds under, 7.0 to 13.1 s) and `chunk_blocks` (15 ms; 15.0 to 19.4 ms). Each gained a fifth to three quarters, but a quieter machine is needed to say whether the median is under.
- **Missed:** biome presence, about 15% faster from the cheaper lookups. The biome-bounds stage meant to meet it didn't pay (the plan's notes).
- Loot's queries, added to the scope's threads, gained 2.5 to 8× (the plan's notes).

## Exit criteria

- [x] A baseline: the bench, three rounds for 26.3 and 26.2, and a profile of each workload named in the scope, recorded in the plan.
- [x] Targets set from the baseline and the profile, written here, and met. Accepted by the user on 2026-11-09 as the [results](#results) left them: the stronghold met, three met at the minimum but not the median, biome presence missed (follow-ups in the [roadmap](../README.md#follow-ups)). Agreed 2026-11-06, for 26.3 on the dev machine, under `cargo xtask bench`:
  - `locate_structure` stronghold, new session: 0.3 s or less (baseline 0.97–1.37 s);
  - `nearest_feature monster_room`, new session: 1 s or less (2.52–3.11 s);
  - 128 × 128 dungeon tile at 1:2, one thread: 9 s or less (11.9–13.9 s);
  - `chunk_blocks` with carvers, bound: 15 ms or less (19.5–20.0 ms);
  - biome presence within 500, one evaluation: 5 ms or less (6.99–7.46 ms). The biome-bounds prototype didn't pay, so the stage wasn't built and the target stays (2026-11-08, the plan's notes).
- [x] Every parallel query gives the same answer on one thread as on many, checked by tests.
- [x] Every build change kept beats the bench's noise over three rounds; each one dropped is recorded with its numbers.
- [x] The GPU and SIMD assessment is in `docs/research/` ([gpu-and-simd.md](../../research/gpu-and-simd.md)).
- [x] `cargo xtask oracle-test` passes for every supported version, as well as `cargo xtask check`.
