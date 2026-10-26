# m06. Seed search

- Status: In progress
- Milestone: [06. Seed search](../roadmap/06-seed-search.md)
- Started: 2026-10-25
- Completed: —

## Goal

Describe a world in a small query language, and get back the seeds that fit it best, ranked, from the library, `spyglass search` and the viewer. One plan for the whole milestone, in phases that can each be verified on their own. It covers every exit criterion:

- The query language has a spec and an ADR: phase 1.
- Every reported seed meets all its required constraints, verified against the plain implementations: phases 3 and 4.
- Scores are deterministic: the same query and seed range give the same ranked list: phases 1 and 5.
- Benchmarks exist for the main constraint types: phase 8.

## Facts established up front

From the code as of 67bcbc6 and `cargo xtask bench` numbers recorded in earlier plans.

- **What needs only the lower 48 bits.** Random-spread placement (`RandomSpread::cell_chunk`, via `setLargeFeatureWithSalt`), all four frequency reducers, exclusion zones between random-spread sets, the weighted structure pick (`setLargeFeatureSeed`) and slime chunks (`seedSlimeChunk`) all seed a `LegacyRandomSource`, which keeps 48 bits of state. Additions and XORs of the seed with other terms only carry upward, so each result depends on `seed mod 2^48` alone. Biome checks, the spawn origin and concentric rings (strongholds) depend on the climate, so they need all 64 bits.
- **The biome check can only remove starts.** Placement gives the chunks where a set may start; `Starts::check` then rejects or keeps them. So a placement-only count is an upper bound on the real count, and an "at least N" constraint that fails on placement alone fails for every one of the 65,536 seeds sharing those lower 48 bits.
- **Per-seed costs today** (release, one thread): binding a session 0.1 ms; `spawn_origin` in a new session 1.2 ms; one `biome_at` about 2.4 µs (65,536 in 158 ms); stronghold ring positions about 1 s in a new session (128 biome searches). Placement of one random-spread cell is one LCG seeding and a few draws, tens of nanoseconds.
- **Consequence for 48 → 64-bit expansion.** Each lower-48 value that survives the placement stage costs 65,536 session bindings, about 6.5 s on one core, before any check runs. The placement stage has to be strict, and binding only what a query needs (climate without spawn targets, say) may be worth doing; phase 8 measures it.
- **Uncertainty.** Most Overworld starts are candidates (`certain: false`) until terrain exists ([milestone 07](../roadmap/07-terrain-and-validity.md)): swamp huts, villages, outposts and most others. Only strongholds, ancient cities and trial chambers can be certain. The spawn origin is exact; the final spawn block is somewhere in the 11 × 11 chunks around it ([specs/spawn.md](../specs/spawn.md)). Tied biome cells make some biome answers uncertain too.
- **`spyglass-search` doesn't exist.** The stub was deleted in m05-cleanup, to be created by the milestone that needs it. `rayon` isn't a dependency yet.

## Approach

### Layers

- **`spyglass-search`** (new; depends on rng, biome, structure, data, as [architecture.md](../architecture.md) plans): the query language (text to syntax tree, with a canonical printer), the **placement prefilter** over the lower 48 bits (on `spyglass-structure`'s seed-only placement), seed sources and order, and the parallel runner with ranking. It knows nothing about `World` or `Session`: `spyglass` hands it the query's structure targets resolved to set indices, and a per-seed check to call on the seeds the prefilter keeps.
- **`spyglass`**: `World::search_query(text)` parses and resolves ids and tags (errors name the line and column). `Session::evaluate(&query)` is the **plain implementation**: it judges one seed completely, every constraint, with details, from the session's own queries (`structures_in` and its per-set ranges, `biome_at`, `slime_chunks`, `spawn_origin`). `Session::check(&query, threshold)` is the fast path the runner calls: the same per-constraint functions, cheapest first, stopping at the first failed required constraint, and skipping preferred constraints that can't lift the score past `threshold`. `World::search(&query, &options, progress)` runs the engine with `check`. Final hits are re-evaluated with `Session::evaluate` for their details (matched features, distances), and that doubles as a check: a hit whose plain verdict or score differs from the engine's is a bug (a debug assertion, and every test).
- So the optimizations the exit criterion asks to verify are the prefilter, the 48 → 64-bit expansion, cost ordering, early exit and the scoring bound. All of them are checked against `Session::evaluate`.
- **Front ends**: `spyglass search` and a search panel in the viewer, both over `World::search`.

Rough API, names to change during the work:

```rust
impl World {
    pub fn search_query(&self, text: &str) -> Result<SearchQuery>;
    pub fn search(
        &self,
        query: &SearchQuery,
        options: &SearchOptions,
        progress: impl FnMut(&SearchProgress) -> ControlFlow<()>,
    ) -> Result<SearchOutcome>;
}
impl Session<'_> {
    pub fn evaluate(&self, query: &SearchQuery) -> Evaluation;
}
pub struct SearchOptions {
    pub source: SeedSource,      // All (lower 48 outer, upper 16 inner), Range(i64..=i64), List(Vec<i64>)
    pub from: u64,               // index into the source to start at
    pub budget: Budget,          // Seeds(u64), Time(Duration), or both
    pub top: usize,              // how many hits to keep
    pub threads: Option<usize>,  // default: all cores
    pub certain: bool,           // required constraints must be certainly met, not just possibly
}
pub struct Evaluation { pub verdict: Verdict, pub score: u32, pub constraints: Vec<ConstraintResult> }
pub enum Verdict { Met, PossiblyMet, Failed }   // three-valued, see below
pub struct SeedHit { pub seed: i64, pub index: u64, pub evaluation: Evaluation }
pub struct SearchProgress { pub evaluated: u64, pub next: u64, pub stages: Vec<StageCount>, pub top: Vec<SeedHit> }
```

### The language

A line-oriented text format with a hand-written parser and no new dependency. Each line is a constraint, `require` or `prefer`, with an optional `weight N` at the end of a `prefer`; `#` starts a comment. The grammar is in [specs/search.md](../specs/search.md); an example:

```text
# A witch-hut farm near spawn, with a village not far off.
require 2 minecraft:swamp_hut within 600 of spawn
require cluster 2 minecraft:swamp_hut within 200 of each other within 600 of spawn
require no minecraft:pillager_outpost within 300 of spawn
prefer 3 #minecraft:village within 400 of spawn weight 2
prefer biome minecraft:cherry_grove within 500 of spawn
prefer biome minecraft:jungle covers 25% within 1000 of 0,0 step 32
prefer 6 slime within 64 of spawn
prefer #minecraft:village within 300 of minecraft:ancient_city within 3000 of spawn
prefer spawn within 200 of 0,0
```

- **Targets**: a structure id or `#tag` (as `/locate` takes them), `biome <id>`, `slime` (chunks), `spawn`.
- **Counts**: `N` means at least N (default 1); `no` means none. `cluster N <structure> within D of each other`: N starts, every two at most D blocks apart (two structures share an AFK spot within R of both when they're at most 2R apart).
- **Regions**: `within R of <anchor>`, or a box `in x1,z1 to x2,z2`; every constraint has one, so every check is bounded. Anchors are `spawn`, a point `x,z`, or, for structure and slime counts, a structure in a region of its own (so "a village within 300 of an ancient city" means some pair is that close).
- **Biome sampling**: biomes are sampled on a grid, at y 64 and every 16 blocks by default, with `at y` and `step` to change them; `covers P%` is the share of grid points in the region.
- **Distances** are horizontal and Euclidean between `/locate` positions (chunk plus the set's locate offset), in integer arithmetic. Bounds-based distances need jigsaw and terrain, so they wait for milestone 07.
- **Spawn** is the spawn origin's climate position ([specs/spawn.md](../specs/spawn.md)). The spec says the real spawn block can be anywhere in the 11 × 11 chunks around the origin chunk.

The canonical printer turns any parsed query back into text that parses to the same tree. The viewer's editor and `--json` output use that text, and the round trip is tested.

### Three-valued results and scoring

Each constraint evaluates to **met**, **possibly met** or **failed**. A count of structures uses certain starts for its lower bound and certain plus candidate starts for its upper bound; a biome cell that's tied counts as possible; a platform-dependent spawn makes spawn constraints possible at best.

- **Required**: by default a seed is kept if every required constraint is met or possibly met, and the hit says which are only possible. With `certain` (`--certain`), only met counts. Milestone 07's terrain turns many "possibly" into one or the other.
- **Preferred**: each yields a satisfaction in thousandths (0 to 1000), times its weight. Each of the first N matches, nearest first, earns `500 + 500·(R − d) / R` (`d` from an integer square root; 1000 in a box), summed and divided by N, so more and closer both count; clusters earn their largest group's share of N; coverage its share of P; `no` is all or nothing. The default mode scores the best case (candidates included), `certain` the worst case. Integer arithmetic throughout, so scores are exact and identical on every platform.
- **Ranking**: by score, highest first, then by index in the seed source. This is a total order, so merging per-thread results gives the same list whatever the thread timing.

### Seed sources, order and budgets

- **`All`** (default): every 64-bit seed, with the lower 48 bits as the outer loop and the upper 16 inner: index `i` is seed `(i mod 2^16) << 48 | i >> 16`. A lower-48 value that fails the placement stage skips its 65,536 seeds at once.
- **`Range`**: 64-bit seeds in numeric order, e.g. the text-seed range `-2^31..=2^31-1`. **`List`**: seeds from a file, one per line, text seeds hashed as `--seed` hashes them.
- **Budgets**: a seed count (exact: the evaluated set is the first N indices from `from`), a time limit (stops at a batch boundary), or both. Every outcome reports `next`, the first index not evaluated, so `--from next` continues, and a time-limited run can be reproduced exactly with `--count`.
- **Determinism**: work is split into fixed batches of indices; each batch's top N is merged into the running top N under the total order above. The same query, source, `from` and seed count give the same ranked list for any thread count; tests run 1, 2 and 8 threads.

### Staging: cheap checks first

Each constraint provides a **necessary condition** for each stage it can be checked at, or none. A stage may reject a seed only when the full check would too; that's the soundness the exit criterion asks for, and phase 4 tests it exhaustively on seed ranges.

1. **Placement (lower 48 bits)**, in `spyglass-search`. Upper bounds from placement only, for required constraints: structure counts, clusters and structure anchors over random-spread sets, plus slime chunks, which are exact here. Only "at least" forms filter (an upper bound can't refute `no`). Spawn anchors use a disc widened by 2,560 blocks, the farthest the spawn search can move from (0, 0) (radius 2,048, then 512 more). Needs a seed-only placement path in `spyglass-structure` that doesn't bind biomes; `Placements` is rebuilt on top of it so there's one implementation.
2. **Full seed, cheapest first**, in `Session::check`. Biome checks on the structure starts; spawn origin; biome presence and coverage (stopping as soon as the answer is decided); concentric rings last.
3. **Scoring bound.** Once the top list is full, a seed whose required constraints pass but whose best possible remaining score can't beat the current Nth is dropped without finishing its preferred constraints. The Nth score only rises, so the final list is unchanged (phase 8, measured before it's kept).

### Front ends

- **`spyglass search`**: `-q <text>` or `--query <file>`, `--top`, `--count`, `--time`, `--from`, `--range a..b` or `--seeds <file>`, `--threads`, `--certain`, `--json`, plus the usual `--game-version`, `--bundle`, `--bundles`. Progress (rate, seeds evaluated, survivors per stage, current best) on stderr; results on stdout when the budget runs out or on Ctrl-C (the best so far). `--json` adds a `search` section to [specs/cli-json.md](../specs/cli-json.md): the canonical query, source, `from`, `next`, `evaluated`, and the hits with each constraint's outcome and matched features.
- **Viewer**: a search panel with a monospace query editor (parse errors under it with line and column, re-parsed as you type), options (top N, budget, source, threads, certain), Run and Stop, live progress, and the top list updating as it grows. Clicking a hit opens that seed on the map, centred on the query's first anchor, with the structure layers the query names switched on and each constraint's matched features ringed (from `Session::evaluate`). Searches run on their own thread with a rayon pool one core smaller than the machine, so tiles keep rendering. The last query is kept in settings.

Specs and ADRs that govern this: [ADR 0014](../adr/0014-public-api-world-and-session.md) (the public API), [specs/structure-placement.md](../specs/structure-placement.md) (placement and certainty), [specs/locate.md](../specs/locate.md) (locate positions), [specs/biome-lookup.md](../specs/biome-lookup.md) (quarts and ties), [specs/spawn.md](../specs/spawn.md), [specs/render.md](../specs/render.md) (the default y), [ADR 0008](../adr/0008-game-derived-test-values-stay-local.md).

## Tasks

### Phase 1: spec and ADRs

- [x] ADR: the query language (a line-oriented DSL with a hand-written parser and canonical printer, over TOML or JSON; why): [0020](../adr/0020-search-query-language.md).
- [x] ADR: the search engine: `spyglass-search` and its boundary with `spyglass`, `rayon`, staging by necessary conditions, the total order and batch merging that make results deterministic, `ctrlc` for the CLI. Licences checked against `deny.toml`: [0021](../adr/0021-search-engine.md).
- [x] `docs/specs/search.md`: grammar, every constraint's meaning and evaluation (counts, clusters, regions, anchors, biome grids, distances), three-valued outcomes, scoring arithmetic, ranking, seed sources and index order, budgets and `next`, determinism.
- [x] `docs/specs/structure-placement.md`: which results depend only on the lower 48 bits, and why.

### Phase 2: the language

- [x] Create `spyglass-search` (`[lints] workspace = true`; architecture table updated).
- [x] Syntax tree, parser with line and column errors, canonical printer.
- [x] Tests: every spec example parses; print-then-parse round trips; malformed input gives errors, never panics (including 20,000 mutated example lines).
- [x] `World::search_query`: resolve structure ids and tags (as `structure_query`), biome ids (as `biome_query`); unknown ids are errors with positions.

### Phase 3: the plain implementation

- [x] `Session::evaluate` from the session's own queries, with `Evaluation`, `Outcome` and per-constraint results (outcome, value, satisfaction, matched features).
- [x] Tests on `spyglass-oracle` fixture bundles: hand-checked answers for each constraint type, including candidates, ties and a platform-dependent spawn; `certain` mode. The platform-dependent spawn is a unit test of the outcome rule: no fixture or known seed has one.

### Phase 4: the fast path

- [x] Seed-only placement in `spyglass-structure` (random-spread cells, frequency, exclusion zones; concentric rings and exclusion zones against them report "can't decide"), with `Placements` built on it. Golden tests still pass.
- [x] The placement prefilter in `spyglass-search`: necessary conditions per required constraint over the lower 48 bits.
- [x] `Session::check`: cost order, early exit on a failed required constraint.
- [x] Soundness and equality tests: over seed ranges on fixture bundles (and, `#[ignore]`d, the 26.3 bundle), for every seed, the prefilter never rejects a seed `Session::evaluate` keeps, and `check`'s verdict and score equal `evaluate`'s. Queries chosen so that a useful share of seeds pass each stage.

### Phase 5: running a search

- [x] Seed sources and index mapping; batches; budgets; `next`; progress callback with stage counts; cancellation through the callback.
- [x] `World::search` on `rayon`, with a thread-count option; top list merged under the total order; final hits re-evaluated with `Session::evaluate`.
- [x] Determinism tests: same results for 1, 2 and 8 threads, and for one run against the same range split into two with `from`.

### Phase 6: `spyglass search`

- [x] Command and arguments; progress on stderr; Ctrl-C prints the best so far.
- [x] Human-readable output, and `--json` with its section in `cli-json.md`.
- [x] Integration test on a fixture bundle: the command's hits equal `World::search`'s.

### Phase 7: the viewer

- [ ] Search panel: editor with live errors, options, Run and Stop, progress, live top list.
- [ ] Opening a hit: seed, centre, layers, matched features ringed; markers checked against `Session::evaluate`.
- [ ] Settings persistence for the query and options; unit tests for the non-drawing parts (option building, hit-to-view mapping).

### Phase 8: speed, benchmarks and close-out

- [ ] Bench lines (`cargo xtask bench`), in seeds per second on one thread and on all: structure count (placement stage only, and with biome checks), cluster, structure-to-structure distance, slime, biome presence, biome coverage, spawn, and a mixed query.
- [ ] Measure binding cost per seed in the full-seed stage; if it dominates, bind only what the query needs (climate without spawn targets), in the library.
- [ ] Stronghold constraints: compute ring positions one ring at a time if ring 1 alone settles most queries; checked by the structure golden tests.
- [ ] The scoring bound, kept only if it measurably helps.
- [ ] Update architecture.md (search no longer planned), README, getting-started (a first search).
- [ ] `cargo xtask check` and `cargo xtask oracle-test`; move this plan and the milestone to `completed/`.

## Verification

- Exhaustive soundness and equality tests of the staged evaluator against `Session::evaluate` over seed ranges (phase 4), on fixture bundles in the normal suite and on the 26.3 bundle in the ignored suite. No game-derived values are committed (ADR 0008).
- Determinism tests across thread counts and split ranges (phase 5).
- The CLI integration test (phase 6).
- `cargo xtask bench` lines for each constraint type, recorded in Notes (phase 8).
- A manual check: the witch-hut query above from the CLI and the viewer gives the same top list for the same count; each hit opened in the viewer shows the matched huts, and `spyglass locate structure` agrees with their positions.

## Durable outputs

- [ ] `docs/specs/search.md` (new)
- [x] `docs/specs/structure-placement.md`: lower-48-bit results
- [x] `docs/specs/cli-json.md`: `search`
- [x] ADR: the query language ([0020](../adr/0020-search-query-language.md))
- [x] ADR: the search engine ([0021](../adr/0021-search-engine.md))
- [ ] `docs/architecture.md`, `README.md`, `docs/getting-started.md`

## Decisions

Agreed on 2026-10-25, as recommended:

- **Candidates in required constraints:** seeds whose required constraints are possibly met are kept, and the hit says so; `--certain` keeps met only. Requiring certainty now would make most structure queries (swamp huts, villages) return nothing until milestone 07.
- **Language form:** the line DSL above, not TOML or JSON. It's short enough for `-q` on a command line and readable in the viewer's editor; a JSON form of the tree can come later if scripts need one.
- **Preferred scoring:** graded counts and distances in integer thousandths, so "best" means closer and more, not only "passes".
- **Default seed order:** lower 48 bits outer, upper 16 inner, from index 0, so the placement stage skips 65,536 seeds at a time and `--from` resumes. Random sampling of the space can be added as another source.
- **Biome grid default:** y 64 (as the map) and every 16 blocks, settable per constraint.
- **One plan**, phased like m05-viewer. Phases 6 and 7 can become a second plan if this one grows.

## Notes

- 2026-10-25: Phase 6 done. A first real search, `require 2 minecraft:swamp_hut within 400 of 0,0`, over the first 4,194,304 seeds of `All` (64 values of the lower 48 bits): 146 s on all cores, the prefilter skipped 18 of the 64 (1,179,648 seeds), 3,014,656 were checked and 10,626 reported. The top hit's two huts agree with `spyglass locate structure`. Checking is about 20,000 seeds a second across the machine, nearly all of it binding sessions and the huts' biome checks: phase 8's subject. The run also showed `reported` counting only seeds that reached the top list's threshold; `check` now says `Failed`, `Below` or `Scored`, so the count is every reported seed and doesn't depend on timing.
- 2026-10-25: Phases 4 and 5 done. `SeedPlacements` answers placement from the lower 48 bits, and `Placements` now asks it first; the structure golden tests and `/locate` checks still pass, and on 26.3 it decides every random-spread set exactly (every exclusion zone there is against a random-spread set). Over 200 fixture seeds and 6 queries the prefilter never rejected a seed `evaluate` reports, and `check` equalled `evaluate` at six thresholds in both modes; the same holds for 40 seeds and 5 queries on 26.3 (an ignored test). The runner works in rounds of 1,024-index batches, 4 per thread, merged in rank order; the threshold it passes to `check` is the current Nth score, ties included, so pruning can't change the list. Indices are counted in `u128` inside the runner so `All`'s 2⁶⁴ seeds have an end. `largest_group` moved to `spyglass-search`, which the prefilter needs too, with a brute-force test over every subset of 12 points.
- 2026-10-25: Phases 2 and 3 done. `Session::evaluate` takes 9.2 ms in a new session on 26.3 for four constraints (two swamp-hut counts, villages, cherry grove presence within 500), most of it the cherry grove's 3,000 biome samples. Places are now limited to 65,536 blocks on a side (radius 32,767): without it a count like `slime within 30000000 of spawn` would enumerate 10¹³ chunks. The real bundle caught the spec's example using `minecraft:village`, which is only a tag (`#minecraft:village`). The fixture world with structures moved from the public API's tests into `spyglass_oracle::fixtures::structured_overworld`, for the search tests to share. Evaluations keep both cases' scores, so `certain` is an option of the search, not of evaluating a seed.
- 2026-10-25: Phase 1 done. Writing the grammar settled three things the draft left open. The weight goes at the end of the line (`weight N`), since `prefer 3 …` would otherwise be ambiguous between a weight and a count. A cluster is N starts every two at most D apart, which stays in integers, unlike a common centre. Each constraint is evaluated in a best and a worst case, which gives the three outcomes and the two scoring modes. The engine's layering changed too: per-seed evaluation lives in `spyglass` (`Session::evaluate`, plain, and `Session::check`, fast, over shared per-constraint functions), and `spyglass-search` holds the language, the placement prefilter and the runner, so constraint semantics aren't written twice (ADR 0021).
- 2026-10-25: Started, with the open questions decided as recommended.
- 2026-10-25: Drafted from milestone 06, the code at 67bcbc6 and bench figures from m03-spawn, m05-cleanup and m05-viewer.
