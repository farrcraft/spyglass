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
- [x] Profile again before phase 3 (notes).

### Phase 3: threads within a query

- [x] ADR: how library queries use threads, alongside the search's pool, its throttle and the viewer's tile workers ([ADR 0063](../adr/0063-threads-within-library-queries.md): the current pool, sequential in a search's checks).
- [x] Stronghold ring positions settled in parallel; a test that one thread and many give the same positions and flags (`rings_settle_the_same_on_any_number_of_threads`).
- [x] `nearest_feature` and `features_in` in parallel, with the carved-chunk cache shared: one cell per chunk, so no chunk is carved twice; the same test (`features_are_the_same_on_any_number_of_threads`).
- [x] Bench lines for each, three rounds (notes).
- [x] The viewer checked by eye (`run-viewer`): dungeon tiles at 1:1 while a search runs, so the map still fills and the search still moves (notes).

### Phase 4: build-level gains

- [x] PGO trial on the bench; decision on releases against ADR 0038. Local builds only: `cargo xtask bench --pgo` and `cargo xtask dist --pgo` (notes).
- [x] `mimalloc` trial in the CLI and viewer; every `dist` target and the MSRV build checked. Kept; Windows `dist --pgo` and a Linux build in the profile container pass, and MSRV; macOS is left to the release workflow.
- [x] A faster hasher for the maps the profile shows are hot: `FxHashMap` for `corners_of`'s cells and `NoiseChunk`'s two maps.
- [x] ADR for the changes kept; the numbers for the ones dropped in the notes ([ADR 0064](../adr/0064-build-level-speedups.md)).

### Phase 5: biome bounds

- [x] Prototype: rejection rate and cost on biome presence and coverage queries; decision with the user. Not built: sound bounds over tiles reject nothing, and per-sample ones cost what the evaluation does (notes). The prototype was deleted.
- [x] If kept: ADR, spec, stage, soundness tests, bench lines. Not kept.

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

- [x] ADR: threads within library queries ([0063](../adr/0063-threads-within-library-queries.md)).
- [x] ADR: build-level changes kept (PGO, allocator, hasher), if any ([0064](../adr/0064-build-level-speedups.md)).
- [x] ADR: the biome-bounds stage, if built. Not built.
- [x] [search.md](../specs/search.md): the new stage, if built. Not built.
- [ ] [biome-lookup.md](../specs/biome-lookup.md) and [material.md](../specs/material.md#biomes): the hinted search and the batched zoom, if either spec describes how spyglass computes them.
- [ ] [locate.md](../specs/locate.md) and [structure-placement.md](../specs/structure-placement.md): cost lines, and that ring positions are settled independently.
- [x] [getting-started.md](../getting-started.md): how to profile, and profile-guided builds.
- [ ] `docs/research/`: the GPU and SIMD assessment.
- [x] [architecture.md](../architecture.md), if the use of threads changes what it says about crates or front ends: a paragraph in [front ends](../architecture.md#front-ends).

## Notes

- 2026-11-08: **Biome bounds prototyped**, on 26.3, for `require biome minecraft:cherry_grove within 500 of 0,0` (3,069 samples; 8 to 11% of seeds have one). An estimate that a real stage can only match or lose to: each sample's true climate, widened by twice what the octaves left out can add (each layer's `|amplitude|` times 1.04, Perlin's largest value rounded up), hulled over square tiles, and tested against the parameter list by branch and bound (a target ruled out where its least fitness over the box is above every point's greatest). No seed with the biome was ever rejected.
  - **The slack is too wide.** The climate noises' layer weights fall slowly: continentalness's 9 octaves weigh 0.75, 0.38, 0.38, 0.19, 0.09, … Kept to 1 to 3 octaves, continentalness's slack (doubled) is ±1.35 or more, and 0 to 2% of 200 seeds were rejected at any depth slack.
  - **Even exact climates don't pay.** With every octave kept, so the boxes are the true climate's hull over each tile:

    | Tile | Rejected (of 100) | Box tests a seed |
    | ---- | ----------------- | ---------------- |
    | 128  | 37                | 14 ms            |
    | 32   | 73                | 270 ms           |
    | 8    | 92                | 980 ms           |

    The full evaluation costs 7.4 ms a seed, so every tile size that rejects a useful share costs more than what it saves, before any noise is sampled. Five of continentalness's nine octaves at 32-block tiles reject 55% for 190 ms. A rare biome is small patches, and ruling it out needs nearly the full climate at nearly every sample, which is the evaluation itself.

  - **Coverage** (`plains covers 30% within 1000 step 32`) was slower still: 0.5 to 0.7 s of box tests a seed at 128-block tiles, rejecting none with one octave kept.
  - 26.2's noises are built the same way, so it would fare no better.
  - **Tried harder**, at the user's request: the seed's real stacks summed over the kept octaves at each tile's centre, widened once by the omitted octaves and by each kept layer's change across the tile; and a box test that looks for one point beating every target over the box, trying the last one first (0.8 ms a seed for presence, from 270 ms). It rejected none of 100 seeds at any setting (2 to 5 octaves, tiles of 32 to 128 blocks).
    - **Why:** the boxes came out 20 to 1,000 times wider than the true climate's hull over the same tiles: over a 64-block tile, temperature 1.13 against 0.022 and continentalness 5.6 to 18 against 0.11. Two causes. Perlin's gradient bound (3.3 per lattice unit, measured) is several times its typical slope. And the shifts' octaves reach frequency ½, so across a tile the shift can move a noise's input by its whole range, about 13 quart units of the 21 a tile's centre has to cover. A Taylor bound, the centre's gradient plus a curvature term, halved temperature's width and widened the higher octaves'.
    - **Per sample instead of per tile**, with each sample's shift exact: the first estimate at one sample a tile bounds it, 88% rejected (of 92% possible) with five octaves kept and 8% with three. But five octaves of each noise and the shifts are about 44 of the 52 layers a sample costs, plus a box test for the lookup, so the stage would cost about what the evaluation does.
  - **Decision:** not built. A rare biome is small patches, so ruling it out needs nearly the full climate at nearly every sample, which is the evaluation itself.

- 2026-11-08: **A faster hasher.** The phase 2 profiles' hashing samples, by the first frame above the hasher: on 26.3, `corners_of`'s map is 5.9 to 7.1% of chunk work, dungeon tiles and `nearest_feature`, and nothing else passes 0.3%; on 26.2, `NoiseChunk`'s `surface_levels` and `columns` are 1.9%. Those three maps became `rustc-hash`'s `FxHashMap` (already in the tree); none is iterated, so no answer can change. With NordVPN and a browser each keeping a core busy, three rounds of the usual lines spread by ±25%, so the single-threaded chunk lines ran eight alternating rounds of 40 runs each, against f4d10c3. Minimum and median of the rounds:

  | Line                                 | Before           | FxHash           |
  | ------------------------------------ | ---------------- | ---------------- |
  | `chunk_blocks` with carvers, 26.3    | 15.9 and 24.9 ms | 13.7 and 22.2 ms |
  | `chunk_blocks` without carvers, 26.3 | 19.0 and 27.3 ms | 17.2 and 23.7 ms |
  | `chunk_blocks` with carvers, 26.2    | 80.9 and 133 ms  | 76.7 and 119 ms  |
  | `chunk_blocks` without carvers, 26.2 | 76.4 and 81.7 ms | 51.3 and 77.5 ms |

  Kept: about a tenth off 26.3's chunk work, a little more than the profile's share of hashing, within noise.

- 2026-11-08: **mimalloc** 0.1.52 as the bench's global allocator, against the FxHash build, alternating. Single-threaded chunk work doesn't move (eight rounds of 40 runs: 26.3 with carvers 23.2 and 24.6 → 22.0 and 24.1 ms, minimum and median; without, 20.0 and 26.9 → 23.7 and 27.6 ms). Threaded feature work on 26.3 gains about 9%: eight rounds of the 1:1 dungeon tile gave 1,073 and 1,120 → 980 and 1,022 ms, mimalloc ahead in six. 26.2's tile moved 1% (4,763 and 5,056 → 4,701 and 5,022 ms). It's the CLI's first C code; the viewer already builds C on every target, for `ring` (through `spyglass-extract`'s `ureq`). Kept, by the user's choice (2026-11-08), in the CLI, the viewer and the bench.
- 2026-11-08: **PGO trial**, by hand rather than with `cargo-pgo`: `-Cprofile-generate` on the `dist` bench in its own target directory, trained on the whole bench for 26.3 and 26.2, merged with `llvm-profdata` (the `llvm-tools` component) and rebuilt with `-Cprofile-use`. Against the FxHash build, alternating; minimum and median:

  | Line                                       | FxHash             | PGO                |
  | ------------------------------------------ | ------------------ | ------------------ |
  | `chunk_blocks` with carvers, 26.3 (8 × 40) | 13.6 and 21.7 ms   | 11.9 and 18.8 ms   |
  | `chunk_blocks` without, 26.3 (8 × 40)      | 17.2 and 24.8 ms   | 13.1 and 22.4 ms   |
  | `chunk_blocks` without, 26.2 (8 × 40)      | 51.0 and 78.5 ms   | 49.7 and 65.8 ms   |
  | Dungeon tile at 1:1, 26.3                  | 1,160 and 1,179 ms | 770 and 1,027 ms   |
  | Dungeon tile at 1:2, 26.3                  | 2,353 and 2,380 ms | 1,771 and 2,148 ms |
  | `nearest_feature monster_room`, 26.3       | 1,054 and 1,148 ms | 800 and 1,073 ms   |
  | Biome presence within 500, 26.3 (5 × 20)   | 6.17 and 6.36 ms   | 5.83 and 6.30 ms   |
  | Biome presence within 500, 26.2 (5 × 20)   | 15.5 and 15.5 ms   | 16.3 and 16.4 ms   |

  26.3 gains 10 to 15% in chunk work and more in features; 26.2's chunk work gains a little, its feature lines are mixed and its biome lines lose 5%. The training set is the bench itself, so these flatter it. Kept for local builds only, by the user's choice (2026-11-08): `cargo xtask bench --pgo` and `cargo xtask dist --pgo`, which took 19 minutes on this machine, most of it the instrumented bench. Releases stay as ADR 0038 has them until a training run is shown to repeat.

- 2026-11-08: **Viewer by eye**, dist build, seed 12345 on 26.3, both dungeon features at 1:1, with the default search started while tiles were pending. Both kept moving: tiles left went 78 → 51 → 25 over 30 s, with dungeon markers filling in, while the search went from 3,332 to 10,535 seeds at about 340 a second. `oracle-test` passed on every bundle, both thread tests included.
- 2026-11-07: **Features in parallel**, three alternating rounds against 104721e on 16 threads, on a busier machine than the morning's (the before ranges are wider):

  | Line                                        | 26.3                    | 26.2                    |
  | ------------------------------------------- | ----------------------- | ----------------------- |
  | `nearest_feature monster_room`, new session | 3.74–5.55 → 1.16–1.64 s | 14.0–39.0 → 3.99–4.35 s |
  | 128 × 128 dungeon tile at 1:1, next tile    | 2.51–3.90 → 0.96–1.55 s | 12.7–20.2 → 4.14–4.45 s |
  | 128 × 128 dungeon tile at 1:2, next tile    | 8.29–13.6 → 2.21–3.66 s | 41.4–49.9 → 8.85–9.16 s |
  | Two dungeons near spawn, evaluate           | 3.55–5.56 → 1.49–2.14 s | 20.6–23.0 → 5.31–5.97 s |
  | Two dungeons near spawn, search, seeds/s    | 1.3–1.6 → 1.3–1.6       | 0.3 → 0.2–0.3           |

  What they show:

  - **The shared cache won without a per-thread trial:** a carved chunk is a neighbour of up to 9 placed ones, and a cache per thread would carve most of them again. Threads wait for a chunk another is carving rather than repeat it.
  - **Short of 16×:** a ring of chunks around `nearest_feature`'s centre holds `8r` chunks, and a tile's row only its width, so the first rings and the rows' ends leave threads idle. A chunk ruled out from the outline costs far less than one placed, so the jobs are uneven.
  - **The search doesn't change,** as [ADR 0063](../adr/0063-threads-within-library-queries.md) intends.
  - **Found on review:** parallel work inside a `OnceLock`'s initializer can deadlock, since a waiting thread runs other jobs that may read the same cell. `ring_positions` settles its points before filling `all`, and the ADR states the rule.

- 2026-11-07: **Stronghold rings in parallel.** `locate_structure` stronghold in a new session, three alternating rounds against 104721e: 26.3 0.95–1.14 s → 124–167 ms, 26.2 3.24–3.59 s → 340–386 ms, on 16 threads. `evaluate stronghold` settles only the few points near its area, so it doesn't change (5.7–10.9 ms both ways on 26.3).
- 2026-11-07: **Profiles after phase 2**, as phase 1's (26.3 unless marked). Shares of samples, inclusive unless marked self:

  | Workload                       | Biome lookup                                                        | Density and noise           | Other                                                  |
  | ------------------------------ | ------------------------------------------------------------------- | --------------------------- | ------------------------------------------------------ |
  | Carved chunk                   | 13% lookups (`biome_window` 9.6%), 7% self zoom                     | 27% point path, 18% volumes | SipHash 5% (`corners_of`'s key); first-use loading 12% |
  | 128 × 128 dungeon tile at 1:2  | 19% lookups, 10% of them the outline's `chunk_biomes`; 6% self zoom | 26% point path, 22% volumes | SipHash 5%; ruling chunks out (`none_place`) 13%       |
  | `nearest_feature monster_room` | 17% lookups; 6% self zoom                                           | 26% point path, 24% volumes | SipHash 5%; loading 7%                                 |
  | Biome presence, one evaluation | 77% lookups: 55% climate sampling, 20% parameter search             | (in the lookups)            | loading 19%                                            |
  | Carved chunk, 26.2             | 20% lookups, 18% of them climate sampling                           | `noise_smeared` 32% self    | `preliminary_surface_level` 41%, as before             |

  What they show:

  - **Biome lookup is no longer first in chunk work.** Density and noise lead again, at about half the samples. The zoom went from 12% self to 7%, and the parameter search from 16% to 9%.
  - **What's left of lookups** is mostly windows that overlap their neighbours' and the outline's per-chunk `chunk_biomes`. A cache keyed by quart would take more of it, but each is now under a tenth.
  - **Threads next**, as planned. SipHash (5%) and first-use loading (7 to 19% of short lines) are unchanged: phase 4's hasher, and loading for the CLI.

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
