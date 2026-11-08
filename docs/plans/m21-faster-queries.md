# m21. Faster queries

- Status: In progress
- Milestone: [21. Faster queries](../roadmap/21-faster-queries.md)
- Started: 2026-11-06
- Completed:

## Goal

Every exit criterion of milestone 21:

- a baseline and a profiler;
- cheaper biome lookups in chunk work and searches;
- threads within single queries;
- build-level gains where they measure;
- a biome-bounds stage for searches, if a prototype shows it pays;
- the GPU and SIMD assessment on record.

## Approach

Measure first, as milestone 09 did. Each phase after the first starts from the profile and keeps a change only where the bench, three rounds under `dist`, shows it beats noise.

### Measuring

- **A profiler.**
  - `perf`, or `samply` on Linux, in the Docker image `cargo xtask linux-check` already builds, since perf works there without elevation.
  - Failing that, `samply` on Windows run elevated once per profile.
  - If it takes more than one command, it becomes an `xtask` command ([ADR 0003](../adr/0003-cargo-xtask-for-project-commands.md)).
- **Workloads:** a carved chunk (`chunk_blocks with carvers, bound`), a dungeon tile at 1:2, `nearest_feature monster_room`, a 1:4 height map and `evaluate biome presence`.
- **The 1:4 height map.** Milestone 14 closed at 5.7 s for a tile, and the bench now gives 8.0 s. Bench milestone 14's last commit and HEAD back to back in one sitting, as milestone 09 did for the isolated column. Bisect only if the gap holds.

### Biome lookups

Phase 1's profiles put biome lookup ahead of noise in chunk work ([notes](#notes)). Each fix gives answers identical by construction, and the oracle comparisons and the existing tests check them.

- **A warm-started parameter search** (`SearchTree::lookup_with` in `spyglass-biome`). A lookup can take a hint, a list point whose fitness bounds the minimum from above, and start `best` there instead of at `i64::MAX`.
  - Every point at the minimum is still visited, ties included, and `offer` clears and refills `hits` as before; the hint only prunes more.
  - Built without a new parameter: `lookup_with`'s reused `hits` already holds the previous answer, so its first point is the hint, and an index outside the list is ignored. Batches (`biomes_in`, `biome_grid`, the locate scan) reuse one buffer. `Biomes::biome_at`, which the search's sampling, the structure checks and the renderer call one quart at a time, keeps its buffer per thread, so `Biomes` stays shareable between threads.
  - A test compares hinted and cold lookups, and `brute_force_hits`, over random targets and random hints.
- **Each chunk's biome window looked up once.** `ChunkGenerator::chunk_biomes` covers a 6 × 6 window of quart columns at every y, and the surface's `biomes_in` covers exactly the same window. The outline calls `chunk_biomes` directly as well as through the query's cache, so a placed chunk's window was looked up up to three times. The generator now keeps windows per seed (`biome_window`, up to 2,048 chunks, cleared when full), and all three read them. A window depends only on its chunk, so this is exact. Neighbouring windows still overlap (36 quart columns each for 16 of the chunk's own); a cache keyed by quart is the next step if the profile still shows lookups.
- **The zoom's fiddles once per quart** (`BiomeZoom` in `spyglass-blocks`). `quart` hashes 8 corners per block, each an LCG chain of 8 steps, but a corner's three fiddles depend only on the quart and the seed. A batch form computes each quart's fiddles once and each block's distances from them, with the same operations in the same order ([material.md](../specs/material.md#biomes)). Single blocks keep `quart` as it is, and a test compares the two over random blocks and seeds.

### Threads within a query

- **How a library call uses threads.** Today only the search runner uses rayon, in a pool it owns ([ADR 0021](../adr/0021-search-engine.md)). A `par_iter` inside a query runs on whichever pool it's called from:
  - inside a search's checks, that's the search's pool, where [ADR 0046](../adr/0046-the-viewers-search-yields-to-the-map.md)'s throttle holds threads back;
  - in the viewer's tile workers, plain threads, it's rayon's global pool, which would compete with the workers for the same cores.
  - The ADR decides between using whatever pool is current, a pool passed in through the options, and parallel only when the caller asks. It also says what happens to nested parallelism in a search.
- **Stronghold rings** (`ConcentricRings::positions` in `spyglass-structure`): `points` is sequential and draws the ring's random; `settle_point` gives each point its own fork. Settling the points in parallel and collecting them in order gives the same list ([structure-placement.md](../specs/structure-placement.md#concentric-rings)). The game does the same with its background executor. To check:
  - that `Biomes` is `Sync`;
  - that nothing it caches can change an answer, which matters for tied cells ([biome-lookup.md](../specs/biome-lookup.md#ties)).
- **Features** (`Session::nearest_feature`, `Session::features_in` in `spyglass/src/features.rs`):
  - Each chunk's placing is independent given its neighbours' carved blocks. `features_in`'s rows share those through a row cache, and `nearest_feature`'s rings through one `Cache`. Parallel work needs that cache shared between threads or rebuilt per thread; measure which.
  - Results must come out in the order [locate.md](../specs/locate.md#features) gives: z then x for an area, by distance for nearest. A ring is placed whole before the stopping test, as it is today.
- **Contention.** The per-seed caches (`CornerCache`, `CornerStore`, `SurfaceLevels`) are `Mutex<HashMap>`s, which threads sharing one session would fight over. The profile with threads says whether that matters.

### Build-level gains

- **PGO** with `cargo-pgo`, trained on the bench. A trial first, on the bench's lines.
  - Profiling data from a training run isn't deterministic, which conflicts with reproducible release archives ([ADR 0038](../adr/0038-how-release-archives-are-built.md)). Kept for releases only if the profile can be produced reproducibly, or kept for local `dist` and bench builds only, as the ADR records.
- **A global allocator** (`mimalloc`) in `spyglass-cli` and `spyglass-viewer`. It builds C code, so check every `cargo xtask dist` target and the MSRV build.
- **A faster hasher** (`rustc-hash` or `foldhash`) for the hot maps keyed by coordinates or pointers: the volume memo, the aquifer's maps, the chunk caches. Choose them from the profile, not all 107 `HashMap`s.
- **Each dependency kept** gets its `deny.toml` license check and one ADR covering the build changes.

### Biome bounds in searches

- **The idea.** Biome constraints sample whole climate noises today: biome presence within 500 blocks is 8 ms a seed. Each octave is seeded from its own name (`"octave_<index>"`, [noise.md](../specs/noise.md)), so a noise's low octaves can be built and sampled alone with the same values. The rest add at most the sum of their amplitudes. From that come intervals for the climate parameters, and from those a set of biomes a sample could be. A seed with no possible target at any sample point is rejected without the full evaluation, a necessary condition as [ADR 0021](../adr/0021-search-engine.md) asks.
- **The prototype measures:** how many seeds the bounds reject for biome presence and coverage queries, the cost of a bounded sample against a full one, and how many octaves to build.
  - The bound must hold for the noise as the game sums it: the `NormalNoise` value factor, amplitudes and both stacks.
  - The biome parameter search must be answered over intervals, not points. That is new code in `spyglass-biome`.
- **Only if it pays:** an ADR for the stage, the stage in `spyglass-search` and `spyglass`, the search spec, and soundness tests against `Session::evaluate` over seed ranges, as the placement prefilter has.

## Tasks

### Phase 1: measure

- [x] The bench, three rounds each for 26.3 and 26.2, recorded in the notes.
- [x] A profiler that runs on this machine, documented in [getting-started.md](../getting-started.md). `cargo xtask profile` ([profiling](../getting-started.md#profiling)), with the bench's new `--runs`.
- [x] Profiles of a carved chunk, a dungeon tile at 1:2, `nearest_feature`, a 1:4 height map and a biome-presence evaluation, summarized in the notes.
- [x] The 1:4 height map against milestone 14's close, back to back; bisected and fixed if it's a regression. No regression (notes).
- [x] Targets confirmed or changed with the user, and written into the milestone. Agreed 2026-11-06, with the biome-lookup phase added ahead of threads.

### Phase 2: biome lookups

- [x] Warm-started parameter search; the warm-against-brute-force test (`warm_starts_give_the_cold_answer`); `biome_at`'s per-thread buffer.
- [x] Per-seed chunk biome windows for `chunk_biomes`, the outline and the surface.
- [x] Zoom fiddles kept per quart in `ChunkBiomes`; the kept-against-fresh test (`kept_fiddles_zoom_as_fresh_ones`).
- [x] [biome-lookup.md](../specs/biome-lookup.md#search) and [material.md](../specs/material.md#biomes) updated.
- [x] Bench lines, three rounds: the three changes together against 848fcfd (notes).
- [ ] Profile again before phase 3.

### Phase 3: threads within a query

- [ ] ADR: how library queries use threads, alongside the search's pool, its throttle and the viewer's tile workers.
- [ ] Stronghold ring positions settled in parallel; a test that one thread and many give the same positions and flags.
- [ ] `nearest_feature` and `features_in` in parallel, with the carved-chunk cache shared or per thread, as measured; the same test.
- [ ] Bench lines for each, three rounds.
- [ ] The viewer checked by eye (`run-viewer`): dungeon tiles at 1:1 while a search runs, so the map still fills and the search still moves.

### Phase 4: build-level gains

- [ ] PGO trial on the bench; decision on releases against ADR 0038.
- [ ] `mimalloc` trial in the CLI and viewer; every `dist` target and the MSRV build checked.
- [ ] A faster hasher for the maps the profile shows are hot.
- [ ] ADR for the changes kept; the numbers for the ones dropped in the notes.

### Phase 5: biome bounds

- [ ] Prototype: rejection rate and cost on biome presence and coverage queries; decision with the user.
- [ ] If kept: ADR, spec, stage, soundness tests, bench lines.

### Phase 6: records

- [ ] The GPU and SIMD assessment in `docs/research/`, with sources, and the roadmap's stretch goal updated.
- [ ] Specs' cost lines updated ([locate.md](../specs/locate.md#features) and others the changes touch).
- [ ] Results table in the milestone: baseline against after.
- [ ] `cargo xtask check` and `cargo xtask oracle-test` for every supported version.

## Verification

- `cargo xtask oracle-test` for every supported version after each phase that touches worldgen or queries.
- Tests that each parallel query returns the same answer on a one-thread pool as on many.
- Soundness tests for the biome-bounds stage against `Session::evaluate`, if it's built.
- `cargo xtask bench`, three rounds under `dist`, before and after each change.

## Durable outputs

- [ ] ADR: threads within library queries.
- [ ] ADR: build-level changes kept (PGO, allocator, hasher), if any.
- [ ] ADR: the biome-bounds stage, if built.
- [ ] [search.md](../specs/search.md): the new stage, if built.
- [ ] [biome-lookup.md](../specs/biome-lookup.md) and [material.md](../specs/material.md#biomes): the hinted search and the batched zoom, if either spec describes how spyglass computes them.
- [ ] [locate.md](../specs/locate.md) and [structure-placement.md](../specs/structure-placement.md): cost lines, and that ring positions are settled independently.
- [x] [getting-started.md](../getting-started.md): how to profile.
- [ ] `docs/research/`: the GPU and SIMD assessment.
- [ ] [architecture.md](../architecture.md), if the use of threads changes what it says about crates or front ends.

## Notes

- 2026-11-07: **Phase 2 benched** against 848fcfd, the three changes together, three alternating rounds per version under `dist`. The oracle comparisons pass for every bundle. Ranges, before → after:

  | Line                                          | 26.3                          | 26.2                      |
  | --------------------------------------------- | ----------------------------- | ------------------------- |
  | `chunk_blocks` with carvers, bound            | 19.1–20.9 → 13.9–14.4 ms      | 93.0–106 → 84.4–91.2 ms   |
  | `chunk_blocks` without carvers, bound         | 18.3–22.1 → 15.6–16.8 ms      |                           |
  | `nearest_feature monster_room`, new session   | 2.46–3.17 → 2.01–2.05 s       | 17.5–19.4 → 14.1–16.2 s   |
  | 128 × 128 dungeon tile at 1:1, next tile      | 3.35–4.38 → 2.22–2.35 s       | 16.9–21.4 → 14.6–16.2 s   |
  | 128 × 128 dungeon tile at 1:2, next tile      | 10.8–13.9 → 7.34–7.59 s       | 52.8–61.3 → 44.2–49.1 s   |
  | Biome presence within 500, evaluate           | 6.76–10.5 → 5.94–6.04 ms      | 15.4–17.2 → 15.0–18.7 ms  |
  | Biome coverage, 1000 step 32, evaluate        | 7.27–9.38 → 6.05–6.09 ms      | 15.3–20.2 → 15.5–20.2 ms  |
  | `biome_at`, 256 × 256 quarts                  | 155–207 → 135–153 ms          | 347–425 → 382–396 ms      |
  | `locate_structure #village` (resolved)        | 46.3–49.8 → 35.7–37.0 ms      | 352–382 → 360–398 ms      |
  | Witch huts unconfirmed, 1 thread, seeds/s     | 3,388–4,408 → 5,285–5,350     | 672–775 → 682–792         |
  | Witch huts unconfirmed, all threads, seeds/s  | 31,399–34,026 → 39,916–41,968 | 6,416–6,874 → 6,611–7,143 |
  | Two dungeons near spawn, all threads, seeds/s | 1.2–1.4 → 1.9                 | 0.2–0.3 → 0.3             |

  What they show:

  - **26.3 gains everywhere biomes are looked up**: chunk work by 30%, dungeons by a third, the searches by a fifth, structure checks by 15 to 30%.
  - **26.2 gains in chunk work and features** (dungeons and `nearest_feature` by 15 to 25%), but its climate lines barely move: its climate sampling costs more than the lookup, as phase 1 found.
  - **26.2's lines that no change touches** (`height_map`, relief) read 5 to 10% slower in that run. Rerun alone, three alternating rounds of the `biome_map` lines gave 336–345 → 324–339 ms at 1:4, so it was the machine.
  - The tile of surface biomes at 1:4 didn't move (4.71–5.83 → 4.71–5.86 s): it's surface work, with few lookups a chunk.

- 2026-11-06: **Profiles** (phase 1). perf 6.8 in a privileged container from the `linux-check` image:
  - a Linux `dist` build with line tables and frame pointers (`CARGO_PROFILE_DIST_DEBUG=line-tables-only`, `-C force-frame-pointers=yes`);
  - bundles copied into the container, so the Windows mount's 9p reads stay out of the samples;
  - `cpu-clock` sampling, delayed past `World::open`;
  - the bench's new `--runs n` for short lines, so first-use loading doesn't dominate them.

  Shares of samples, inclusive unless marked self:

  | Workload (26.3 unless marked)  | Biome lookup                         | Density and noise                                            | Other                                                                        |
  | ------------------------------ | ------------------------------------ | ------------------------------------------------------------ | ---------------------------------------------------------------------------- |
  | 128 × 128 dungeon tile at 1:2  | 35% parameter search, 7.5% self zoom | about 20%                                                    | SipHash about 5%; first-use loading 6%                                       |
  | `nearest_feature monster_room` | 28% parameter search, 8% self zoom   | about 20%                                                    | loading 7%                                                                   |
  | Carved chunk                   | 16% parameter search, 12% self zoom  | 26% point path, 18% volumes                                  | SipHash 6.5%, all in `corners_of`'s key                                      |
  | Biome presence, one evaluation | 30% parameter search                 | 60% climate sampling (point path)                            |                                                                              |
  | 256 × 256 height map at 1:4    | none                                 | 75% volumes: `Node::fill` 27% self, `add_to_volume` 25% self | loading 9%                                                                   |
  | Carved chunk, 26.2             | 20%                                  | `noise_smeared` 33% self                                     | `preliminary_surface_level` 41%, through the aquifer's build and the carvers |

  What they show:

  - **Biome lookup is the biggest single cost in chunk work, ahead of noise.** Three causes, and a fix for each whose answers are identical by construction:
    - **The parameter search starts cold.** `SearchTree::lookup_with` starts at `best = i64::MAX`. Seeding `best` with the fitness of a neighbouring quart's answer is still exact: that distance bounds the minimum from above, so every point at the minimum, ties included, is still visited, and `offer` clears and refills `hits` as before. It only prunes more of the tree.
    - **Quarts are looked up more than once.** `ChunkGenerator::chunk_biomes` looks up a 6 × 6 window of quart columns per chunk, every y (3,456 lookups), so overlapping windows look up each quart about 2.25 times. The surface's `biomes_in` asks for many of the same quarts again. A quart's lookup depends only on its position, so a per-query quart cache is exact.
    - **The zoom repeats its hashing.** `BiomeZoom::quart` runs 8 LCG chains of 8 steps for every block. A corner's three fiddles depend only on the quart and the seed, so all 64 blocks of a quart cell share their 8 corners' fiddles. Computing each quart's fiddles once gives the same `f64`s.
  - **SipHash** on `corners_of`'s 52-byte key (`CornersKey`) is about 5 to 6.5% of chunk work: the faster-hasher task, targeted.
  - **Loading on first use** (structure sets, jigsaw templates' NBT) is 5 to 9% even of multi-second lines, on top of `World::open`'s 0.4 s. It matters for the CLI, where every run pays it once.
  - **26.2's surface levels** are recomputed by each `NoiseChunk`. A per-seed cache, as 26.3 has had since milestone 14, may be the cheap fix the milestone allows.
  - **1:4 heights** are density volumes throughout. Nothing new to batch, as milestone 14 found.

- 2026-11-06: **No regression in 1:4 heights.** Benched back to back at 893b11b (milestone 14's close) and HEAD, three alternating rounds: a 1:4 height map took 4.84–4.99 s against 4.87–5.69 s, and a 1:1 height map 0.90–0.96 s against 0.92–0.99 s. The draft's 8.0 s came from a round that started on a busy machine, right after a build.
- 2026-11-06: **Baseline** (`dist`, three rounds per version on a quiet machine; ranges):

  | Line                                           | 26.3                  | 26.2                |
  | ---------------------------------------------- | --------------------- | ------------------- |
  | `World::open`                                  | 404–451 ms            | 380–458 ms          |
  | `locate_structure` stronghold, new session     | 0.97–1.37 s           | 2.16–3.15 s         |
  | `nearest_feature monster_room`, new session    | 2.52–3.11 s           | 15.4–22.5 s         |
  | 128 × 128 dungeon tile at 1:1, next tile       | 3.56–4.69 s           | 17.3–21.3 s         |
  | 128 × 128 dungeon tile at 1:2, next tile       | 11.9–13.9 s           | 49.6–64.7 s         |
  | 256 × 256 height map at 1:4                    | 5.01–6.56 s           | 9.02–13.2 s         |
  | 256 × 256 height map at 1:1                    | 0.87–1.10 s           | 3.64–5.27 s         |
  | Tile biomes at the surface at 1:4, new session | 4.81–6.19 s           | 8.52–11.0 s         |
  | `chunk_blocks` with carvers, bound             | 19.5–20.0 ms          | 93.5–141 ms         |
  | `height_at` one column, bound                  | 0.205–0.209 ms        | 0.531–0.751 ms      |
  | Biome presence within 500, evaluate            | 6.99–7.46 ms          | 16.0–22.6 ms        |
  | Biome coverage, 1000 step 32, evaluate         | 7.04–7.19 ms          | 15.2–23.4 ms        |
  | `locate_biome cherry_grove`                    | 43.3–54.0 ms          | 208–314 ms          |
  | Witch huts unconfirmed, 1 thread               | 3,125–4,251 seeds/s   | 749–827 seeds/s     |
  | Witch huts unconfirmed, all threads            | 28,911–31,988 seeds/s | 5,953–6,702 seeds/s |
  | Two dungeons near spawn, all threads           | 1.1–1.3 seeds/s       | 0.2–0.3 seeds/s     |

- 2026-11-06: **Drafted**, from a survey of the code and one bench round (the table in the milestone).
  - **Threads.** rayon appears only in `spyglass-search`'s runner; the viewer uses plain threads for tiles, search and extraction; `spyglass-world` scopes threads for region files. `nearest_feature` and `features_in` place chunks one at a time at about 30 ms each. `ConcentricRings::positions` maps `settle_point` over the points sequentially.
  - **Search scaling** is fine: 8.3× from 1 to 16 threads on 8 cores.
  - **The placement prefilter isn't the bottleneck:** 394 billion seeds a second, since it rejects 65,536 seeds at once. The cost is in the per-seed float work after it, so the roadmap's GPU placement search would speed up the part that's already cheap.
  - **GPU, for the research note** (to confirm with sources):
    - WGSL has no `f64`, and wgpu supports it only on some native backends.
    - Consumer GPUs run `f64` at 1/32 to 1/64 of their `f32` rate.
    - Shader precision rules allow division and square root to be off by a few ULP, compilers may fuse multiply-adds, and denormals may be flushed to zero.
    - The branchy work (jigsaw, features, loops driven by the RNG) diverges across lanes.
    - A second implementation of every algorithm would go against [ADR 0026](../adr/0026-how-game-versions-coexist.md)'s one code path per algorithm.
  - **SIMD:** nothing new since ADR 0031's prototype.
  - **1:4 heights:** milestone 14 found that at 1:4 each column sits on its own noise-cell corner, so neighbours share nothing. Batching like a chunk's volume doesn't apply; what's open there is the possible regression (there was none; see above).
