# m16. Search within a seed

- Status: Completed
- Milestone: [16. Search within a seed](../../roadmap/completed/16-search-within-a-seed.md)
- Started: 2026-10-29
- Completed: 2026-10-29

## Goal

`find` queries. A query with a target (each start of a structure, each sample of a biome, or each point of a grid) and constraints around it, judged at every target in one seed, with matches reported nearest first. It covers the library, a `spyglass find` command and a mode of the viewer's Search tab with presets. It adds what the presets need: terrain conditions and biome tags. This plan covers all four of the milestone's exit criteria.

## Approach

The seed search already judges a seed against constraints around fixed anchors (`spawn`, a point, a structure's starts). A within-seed search is the same evaluation, repeated with one more anchor, `here`, set to each target in turn. So most of the work is in the language and in enumerating targets. The evaluation itself barely changes, and both kinds of search keep one evaluator, which the seed search's tests already check.

**The language** ([ADR 0020](../../adr/0020-search-query-language.md), extended by a new ADR before any code). This is the proposal; the ADR settles the syntax.

```text
# Two witch huts close enough to share an AFK spot.
find minecraft:swamp_hut within 4096
require minecraft:swamp_hut within 160 of here
```

- **A `find` line**, at most one, before every constraint: `find TARGET [place] [sampling] [apart D]`.
  - Targets: a structure or `#tag` (each start), `biome B` (each grid sample whose biome matches), or `point` (each grid point, for base sites, judged only by its constraints).
  - The place is as in constraints. `within R` with no `of` means around the search's origin, which the caller gives: `--from` in the CLI, the view's centre in the viewer. So a preset doesn't hard-code coordinates.
  - Sampling (`at y`, `step`) is for biome and point targets.
  - `apart D` skips a match within D blocks of an earlier match. It defaults to 0 for structures and to 256 for biome and point targets, so one biome patch isn't reported once per sample.
- **`here`**, an anchor allowed only in a `find` query: the target's position. For a structure target, its start is never counted by a `here` constraint on its own structure, as a structure anchor never counts itself ([search.md](../../specs/search.md#positions-and-regions)). So "another hut within 160" doesn't count the hut itself.
- **Biome tags** (`#minecraft:is_deep_ocean`), anywhere a biome is written, in both kinds of search. A sample matches when its biome is in the tag. Ties work as now: possibly matching if any tied biome is in it. The monument preset needs `#minecraft:required_ocean_monument_surrounding`.
- **Terrain conditions**, in both kinds of search, from `Session::height_map`'s `WORLD_SURFACE_WG` heights ([terrain.md](../../specs/terrain.md)). These are the heights before structures, carvers and features, and the spec says so.
  - `height A to B place [step]`: every sample's height is in `A..=B`. Satisfaction is the share in range.
  - `flat D place [step]`: the highest and lowest samples differ by at most D. Satisfaction is 1000 if it holds, else `1000 · D / (max − min)`.
  - Heights are certain, so these are met or failed, never possibly met. They're errors in a dimension with a ceiling (the Nether), as terrain layers are ([render.md](../../specs/render.md#dimensions)). The sample limit is lower than for biomes, since a column's height costs far more than its biome. The plan sets it from `cargo xtask bench`.
- **Separate queries:** a `find` query is an error in the seed search, and `here` is an error without `find`. The canonical form includes the `find` line.

**Order and determinism.**

- **Targets:** enumerated over the find place's bounding box. Structures come from `structures_in`, confirmed or not as asked. Biome and point targets are a grid like biome sampling.
- **Order:** distance from the place's centre (the disc's anchor, or the box's centre, rounded down), then z, then x.
- **Judging:** each target is judged with the query's constraints, `here` set to it, through `check`'s fast path, so cheap required constraints reject first.
- **Matches:** a match is a target whose verdict is reported, by the seed search's rules (met, or possibly met outside certain mode), that isn't within `apart` of an earlier match. Matches are reported in target order, up to `top` (default 100), with their score and constraint results.
- **Threads:** targets are judged in parallel batches, and results are kept in target order, so the matches don't depend on the thread count.
- **Control:** stop, throttle and progress reuse the seed search's `Stop`, `Throttle` and progress rounds, with progress as targets judged out of the total.

**Where it lives.**

- **`spyglass-search`:** the syntax (`Query::find()`, `Anchor::Here`, biome tags, the terrain kinds) and the target grid.
- **`spyglass`:** resolution (`World::find_query` and `Game::find_query`, with dimension inference, conversion of `here` across the Overworld and the Nether, and the End error as for `spawn`).
- **Evaluation:** a `here` in the `Judge`, and `Session::find` and `GameSession::find(&FindQuery, origin, &FindOptions) -> Result<FindOutcome>`.
- **Front ends:** the CLI and viewer only call these.

## Tasks

- [x] **ADR:** "Search within a seed". It covers the `find` line, `here`, `apart`, the default origin, biome tags, the terrain conditions and their satisfaction, why one evaluator serves both searches, and the alternatives: a separate language, or a flag on `search`.
- [x] **Syntax** in `spyglass-search`: parse and print `find`, `here`, biome tags, and `height` and `flat`. Limits for every new form, and errors with line and column for misplaced `find` and `here`. Round-trip tests of the canonical form, and parser fuzz cases: the existing fuzz target, with a corpus entry per new form.
- [x] **Resolution** in `spyglass`: biome tags (from `tags/worldgen/biome`, nested tags expanded), the `find` target's dimension, `here` converted across dimensions, terrain conditions rejected under a ceiling, and every misuse an error naming the line.
- [x] **Evaluation:**
  - Biome tags in presence and coverage.
  - `height` and `flat` from height maps, sampled at their own grid, with a cache per session so overlapping constraints share columns.
  - `here` in the `Judge`, with self-exclusion for structure targets.
  - The fast path's cost order puts terrain conditions last.
  - Unit tests on fixture bundles. The fixture's sloping seabed gives known heights.
- [x] **Targets and the run:** enumeration for each target kind, the order, `apart`, `top`, parallel batches kept in order, stop, throttle and progress. Tests:
  - The same matches at 1 and 8 threads.
  - Matches agree with a plain loop of `evaluate` over the targets.
  - A stopped run's matches are a prefix of the full run's.
- [x] **CLI:** `spyglass find --seed|--world -q|--file [--from x,z] [--top n] [--confirm] [--certain] [--threads n] [--time t]`. Text output (the target, distance and score, then each constraint's outcome), and `--json` documented in cli-json.md. CLI tests on fixtures, and ignored ones on the real bundle.
- [x] **Viewer:**
  - The Search tab has "Seeds" and "This seed" modes, sharing the editor.
  - Results are a list with a count, plus next and previous buttons and N/P keys.
  - The opened match is ringed, with each constraint's area drawn around `here`, as for a seed hit.
  - It runs on the viewer's worker and yields to the map ([ADR 0046](../../adr/0046-the-viewers-search-yields-to-the-map.md)).
  - The origin is the view's centre.
  - Check it with the `run-viewer` skill.
- [x] **Presets** in the viewer (text only, in the language):
  - **Double witch huts:** `find minecraft:swamp_hut within 4096`, plus `require minecraft:swamp_hut within 160 of here`. Check the 160 against hut geometry and the 128-block spawning sphere, and fix it in the ADR.
  - **Monument in deep ocean:** `find minecraft:monument within 4096`, plus `require biome #minecraft:required_ocean_monument_surrounding covers 100% within 128 of here step 16`.
  - **Flat pillager outpost:** `find minecraft:pillager_outpost within 4096`, plus `require flat 4 within 48 of here step 4`.
  - **Rare biome nearby:** `find biome minecraft:mushroom_fields within 4096 step 32`, and the same for ice spikes and pale garden.
  - **Cherry grove by the sea:** `find point within 4096 step 64`, with `require biome minecraft:plains within 0 of here`, `require biome minecraft:cherry_grove within 200 of here` and `require biome #minecraft:is_ocean within 300 of here`.
- [x] **Cost:** `cargo xtask bench` gains find lines, over the radius-4096 area around the origin: the double-hut preset, the flat-outpost preset (terrain), and one rare-biome preset. Record the times here, and the exit criterion's cluster time. The viewer stays responsive while a find runs: pan and zoom during one, checked by eye with `run-viewer`, and the map's tiles keep coming, as ADR 0046 requires.
- [x] **Docs:**
  - search.md, or a new find spec beside it, for targets, `here`, `apart`, order, biome tags and terrain conditions.
  - cli-json.md for `find`.
  - getting-started's command and viewer sections.
  - The changelog.
- [x] `cargo xtask check` and `cargo xtask oracle-test`. Complete the plan and the milestone.

## Verification

- **One evaluator:** every match equals `Session::evaluate` of the query with `here` fixed to the target, and every reported target passes the seed search's verdict rule. A test runs both.
- **Against the library's answers:** on fixture bundles, and in ignored tests on the real bundle, each preset's first match agrees with `structures_in`, `biome_at` and `height_map` over the same places, recomputed by hand in the test. This is the milestone's first exit criterion.
- **Determinism:** the same query, seed and origin give the same matches at every thread count, and a stopped run's matches are a prefix of the full run's.
- **Seed search unchanged:** existing seed-search tests and goldens pass unchanged, so the shared evaluator and parser haven't moved. Biome tags and terrain conditions get new seed-search tests too.
- **Cost:** recorded bench times for the presets. The double-hut search over radius 4096 should finish in seconds. The plan fixes the number once measured, as the milestone's third exit criterion asks.

## Durable outputs

- [x] ADR: search within a seed ([0047](../../adr/0047-search-within-a-seed.md)) (the `find` line, `here`, biome tags, terrain conditions).
- [x] search.md, or a new find.md beside it: everything above that the code must keep matching.
- [x] cli-json.md: `find`'s output.
- [x] terrain.md or render.md: a pointer from the height notes to the terrain conditions that use them, if the spec needs it.

## Notes

- 2026-10-29: **Results.**
  - **Speed:** `cargo xtask bench` (26.3, dist build, new sessions, radius 4096 around the origin): double huts 18 ms, flat outpost 54 ms, mushroom fields 115 ms. So the milestone's two-structure cluster search takes 18 ms.
  - **Viewer:** the viewer runs a find on its worker under the seed search's throttle, so it yields to the map (ADR 0046). Checked by eye with `run-viewer`: the double-hut preset on seed 59, and the cherry-grove preset on seed 12345 (6 matches in 0.4 s), stepping with N and the card's buttons.
  - **Real data:** the ignored preset test passes on 26.2, 26.3, 26.4-snapshot-5 and 26.4-snapshot-6.
- 2026-10-29: **Double-hut distance.** The ADR fixes it at 200, not the plan's 160. The derivation is in the ADR.
- 2026-10-29: **Self-exclusion.** When the real-data test anchors the constraints at the match's own block, it counts the target hut too (satisfaction 1000 against find's 567). That's the self-exclusion rule working, so that preset is checked by hand with `structures_in` instead.
- 2026-10-29: **Step buttons.** ◀ and ▶ drew as coloured emoji in egui's font, so the buttons say Previous and Next.

- 2026-10-29: **Done so far.** The library and CLI parts of `find` are done.

  - **Biome tags** live in `World::biome_query`, so `/locate biome #tag` works too, as in the game.
  - **Certainty with tags:** a tied cell whose candidates are all in a tag certainly matches. For a single id that can't happen, so ids behave as before.
  - **Run speed:** every preset runs in under a second on seed 12345 (radius 4096, unconfirmed, dist build). That includes the terrain one: 6 outposts with a 625-column `flat`.
  - **Cross-check:** a seed search found seed 59 with two huts 170 blocks apart, and `find` reports both, confirmed.
  - **No query fuzz target:** the plan's "parser fuzz cases" became new words in the parser's mutation test (`mutated_queries_never_panic`). The cargo-fuzz targets cover only the NBT and region readers.

- 2026-10-29: the presets come from Chunkbase's advanced search (double huts, a monument in deep ocean, a flat outpost, rare biomes), seen in the side-by-side comparison. The bundles have `#minecraft:required_ocean_monument_surrounding`, the tag the game's own monument check uses, so the monument preset can say what the game means by "surrounded".
