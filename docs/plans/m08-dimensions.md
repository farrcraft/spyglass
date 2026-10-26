# m08. The Nether and the End

- Status: In progress
- Milestone: [08. Nether, End and multiple versions](../roadmap/08-dimensions-and-versions.md)
- Started: 2026-10-26
- Completed: not yet

## Goal

Answer every question spyglass answers for the Overworld in the Nether and the End too: biomes, structure starts (confirmed with terrain), heights, locate, maps, world saves and search. Add search constraints that cross dimensions, such as a fortress near the Overworld spawn's portal position. The library comes first, then `spyglass` and the viewer, with a dimension switch. All of it is for 26.3.

This plan covers the milestone's first exit criterion: **Nether and End outputs match the oracle**. The other two, two versions in CI and a guide for adding a version, are the second plan's: m08-versions (not yet written; see [Decisions](#decisions)).

## Facts established up front

These come from the code as of 0ef3687 and the 26.3 bundle. They were checked against 26.2 and 26.4-snapshot-5 bundles, extracted for this plan.

- **The data is already extracted.** `bundles/26.3` holds everything both dimensions need:
  - The `normal` world preset:
    - The Nether is `multi_noise` with preset `minecraft:nether`, settings `minecraft:nether`.
    - The End is biome source `minecraft:the_end`, settings `minecraft:end`.
  - The Nether's biome parameter report.
  - Both dimension types: Nether `coordinate_scale` 8, height 256, logical height 128, `has_ceiling`; End height 256.
  - Both noise settings, `nether/` and `end/` density functions, the three dimension-only structure sets, and their templates.
  - The extractor needs no change.
- **Legacy randomness is the main blocker.** Both noise settings have `legacy_random_source: true` and no `aquifers` key. Every place that binds a graph refuses legacy: `ClimateRouter::load`, `Binder::new` (Xoroshiro only) and `TerrainSettings::load`. The noise spec refuses `legacy` normalization. What legacy changes in `RandomState`, from 1.21 and to be confirmed for 26.3:
  - a `LegacyRandomSource` positional factory;
  - the Nether climate's `createLegacyNetherBiome` (seed and seed + 1);
  - the shift override;
  - `old_blended_noise` seeded from `LegacyRandomSource(seed)`.

  `LegacyRandomSource` itself is ported (`spyglass-rng`).

- **The End needs new noise.**
  - `end/islands` is `cache(max(slice(...distance_to_point...), end_outer_islands))`.
  - `distance_to_point` and `end_outer_islands` load but don't bind.
  - `end_outer_islands` is `EndIslandDensityFunction`: a `LegacyRandomSource(seed)`, 17,292 draws skipped, then **simplex noise**. Simplex noise isn't ported.
  - `TheEndBiomeSource` isn't implemented at all, and nothing biome-related works without a `ParameterList` today: the palette, `World::biomes`, structure-set resolution, ties.
- **The Nether's climate is simple.** Only `temperature` and `vegetation` are non-zero (`nether/temperature`, `nether/vegetation` noises with `y_scale` 0). It has five biomes in a `multi_noise` list, so the existing search tree and ties apply. Biomes still vary with y through the lookup's depth term.
- **Terrain mostly carries over.** Aquifers are already optional, and with none the fluid picker gives lava up to sea level 32 in the Nether and air in the End. The noise range is 0 to 128 in both, so columns stop at 128. `WORLD_SURFACE_WG` in the Nether finds the solid ceiling (about 128 everywhere), so heights mean little there; the bedrock ceiling itself is the material rule's.
- **Structures, by dimension.**
  - Nether:
    - `nether_complexes` (fortress weight 2, bastion weight 3; spacing 27).
    - `nether_fossils` (spacing 2, separation 1).
    - The Nether's ruined portal, which sits in the shared `ruined_portals` set.
  - End: `end_cities` (spacing 20, triangular).
  - What decides each, in `Starts::find`:
    - The fortress check is at a fixed position, decidable from the climate.
    - The bastion is a jigsaw at absolute y 33, so its check is fixed too. Its layout needs `Session::structure_layout`, which already works.
    - The nether fossil is always `Candidate` today. It needs a height provider and a downward scan of the base column, at most sea level.
    - The Nether's ruined portal: its `in_nether` y ranges are ported, and it needs `findSuitableY` on Nether columns.
    - The end city needs the rotated lowest corner, with heights below 60 rejected.
  - Strongholds and slime chunks are Overworld-only.
- **The library is one dimension per `World`.** `World::open` is the Overworld of `minecraft:normal`. `BiomeSource::load` and `StructureSets::load` already take a preset and a dimension. `spyglass-world` already reads player and spawn dimensions and finds every dimension's region folder. The save assessment, the CLI's `--from player` and the viewer's centring all refuse or ignore other dimensions.
- **Search is one `World`.** The query language has no dimension, and its anchors are `spawn` and points. The prefilter, `Session::check` and `evaluate` hold one session. Slime constraints aren't gated to the Overworld.
- **The oracle is ready but not run.** Its Java side resolves any dimension (`Game.stem`), but the golden set is Overworld-only:
  - `GOLDEN_DIMENSIONS`;
  - `structures`, `spawn` and `heights` pass `"minecraft:overworld"`;
  - every probe section uses the Overworld;
  - locate cases are filtered to the Overworld.

  `BiomeSampler` already skips tie checks for the End's source.

- **26.4 and 26.2 differ.**
  - 26.4-snapshot-5 keeps 26.3's formats but:
    - replaces `interpolated` with `upscale`;
    - drops `default_block` from noise settings;
    - adds an `ice_caves` biome;
    - changes the Nether's and End's `final_density`.
  - 26.2 still has the 1.21 formats: noise parameters, surface rules, the three cache types, `shifted_noise`, and aquifer fields in the router.
  - Both are m08-versions' work. This plan targets 26.3 only, but keeps version-specific values in data so that work stays small.

## Approach

### Layers

- **`spyglass-noise`**: simplex noise (`SimplexNoise`, as `EndIslandDensityFunction` uses it), and `legacy` normalization for `NormalNoise`.
- **`spyglass-density`**:
  - Binding with either random source, wired as the game's `RandomState` does for legacy: the Nether biome noises, the shift override, legacy blended noise.
  - Point and volume paths for `distance_to_point` and `end_outer_islands`.
  - `ClimateRouter` and `TerrainRouter` accept `legacy_random_source: true`.
- **`spyglass-biome`**:
  - A `BiomeSource` that is either `multi_noise` (the existing search tree) or `the_end` (`TheEndBiomeSource`: the main island within 64 sections, otherwise by the `erosion` router value at the section's sample point).
  - Biome names and palettes come from the source, not from a `ParameterList`.
  - Spawn origin for dimensions without spawn targets: chunk 0, 0, as today.
- **`spyglass-terrain`**: binding with legacy randomness. Nothing else is expected: no aquifers, the fluid picker as is, and columns clamped to the noise range.
- **`spyglass-structure`**:
  - Sets resolved per dimension, so the Nether's portal is tried in the Nether's copy of `ruined_portals`.
  - Terrain checks for the nether fossil, the Nether's ruined portal and the end city.
  - Bastion layouts through the existing jigsaw assembly.
  - End city layouts aren't jigsaw, so their bounds are out, as for mansions.
- **`spyglass`**:
  - `World::open_dimension(dir, dimension)` beside `World::open`, keeping `World` one dimension ([ADR 0014](../adr/0014-public-api-world-and-session.md)).
  - A small type for a whole bundle (working name `Game`) that opens dimensions lazily and resolves cross-dimension queries.
  - `SaveReport::open_world(dimension)`.
  - Search over a `Game`, one session per dimension per seed, bound only when a constraint needs it.
- **Front ends**: `--dimension overworld|nether|end` on `locate`, `heights` and `render` (default: the Overworld, or the player's with `--from player` / `--world`). `dimension` in JSON is already there. Cross-dimension constraints in `search`. In the viewer, a dimension switch, with the Nether at a default y of its own and terrain layers off there.

Rough API, names to change during the work:

```rust
pub enum Dimension { Overworld, Nether, End }
impl World {
    pub fn open_dimension(dir: impl Into<PathBuf>, dimension: Dimension) -> Result<World>;
    pub fn coordinate_scale(&self) -> f64;           // from the dimension type
}
pub struct Game { /* a bundle, its worlds opened on first use */ }
impl Game {
    pub fn open(dir: impl Into<PathBuf>) -> Result<Game>;
    pub fn world(&self, dimension: Dimension) -> Result<&World>;
    pub fn search_query(&self, text: &str) -> Result<SearchQuery>;
    pub fn search(&self, query: &SearchQuery, options: &SearchOptions, progress: impl FnMut(&SearchProgress) -> ControlFlow<()>) -> Result<SearchOutcome>;
}
```

### Legacy randomness

This is the riskiest part, so it goes first and is probed hardest. Port `RandomState`'s legacy branch from the 26.3 bytecode:

- the positional factory;
- every special case;
- `NormalNoise`'s legacy constructor and normalization;
- `BlendedNoise` seeding;
- `EndIslandDensityFunction`'s seeding and simplex noise.

The probe gains Nether and End sections: legacy noises at points, every named function in `nether/` and `end/` at points and over column volumes, and final-density chunk digests in both dimensions. Each is compared bit for bit, as phase 2 of m07 did for the Overworld.

### Biomes

The Nether is the existing `multi_noise` path once binding works. The End is a new source type.

- Golden biome files for both, from the existing `oracle biomes --dimension`.
- For the Nether, several y levels, since its biomes vary with height. Phase 1 picks the levels and regions; the Overworld's regions out to ±29,999,9xx apply in the Nether too.
- `check-save` against the test world, in all three dimensions, once the user has visited the Nether and End there.

### Terrain and structures

- Heights and block columns in both dimensions, through the height golden files.
- The Nether's `WORLD_SURFACE_WG` is kept exact even though it's the ceiling. What the game computes is what structures use, and the fossil's and portal's scans read base columns, not heightmaps.
- Each terrain-dependent type is ported from its `findGenerationPoint`, and the structure spec's table gains the Nether's and End's rows.
- With the structure golden files and confirmed `/locate` (`/execute in minecraft:the_nether run locate ...`), every stub must equal the game's, and confirmed locate must give the game's answer wherever no tie is involved.

### Cross-dimension search

The language gains a dimension per constraint and coordinate scaling between dimensions. Decided in [ADR 0024](../adr/0024-dimensions-in-the-search-language.md), extending [ADR 0020](../adr/0020-search-query-language.md):

- **A constraint's dimension is inferred from what it counts.** Fortresses, bastions and nether fossils are the Nether's; end cities the End's; biome ids are each dimension's own.
- **When the target can't decide, the dimension is explicit.** Examples are a tag spanning dimensions (the ruined portal ids are in one set) and `spawn`. The keyword is to be chosen; `in` is taken by boxes, so something like `nether: require minecraft:ruined_portal_nether within 200 of spawn`.
- **Anchors convert by coordinate scale.**
  - In a Nether constraint, `spawn` is the Overworld spawn's portal position: its block divided by 8, rounded down. Points are Nether coordinates.
  - Structure anchors in another dimension convert the same way, so "a fortress within 100 of a village" means the Nether's fortress against the village's position divided by 8.
  - The End has no scale and no spawn anchor (its spawn is a fixed platform); `spawn` there is an error.
- **Slime chunks and strongholds are Overworld-only.** In another dimension they're an error.
- **Evaluation keeps one session per dimension per seed,** bound only when needed.
- **The prefilter runs per dimension.** Nether and End placement depend only on the lower 48 bits too, so every dimension's required structure counts filter. The soundness and equality tests from m06 and m07 are extended to cross-dimension queries.

### Front ends

- **CLI:** `--dimension` where a single dimension is meant, and `--from player` taking the player's dimension. Search needs no flag, since constraints carry their dimension. `cli-json.md` gets the dimension values and any new fields.
- **Viewer:**
  - A dimension switch, with the view per dimension kept in settings.
  - Locate and markers follow the dimension shown.
  - A save opens in its player's dimension.
  - Default y: 64 in the Overworld and End; the Nether's chosen in phase 1 from its biome layout.
  - Relief and surface biomes are disabled in the Nether, where the heightmap is the ceiling.

### The oracle

- `GOLDEN_DIMENSIONS` gains the Nether and the End. `structures`, `spawn` and `heights` take the dimension, and their files go under `<kind>/<dimension>/`, as biome files already do.
- The probe gains the sections above.
- `oracle locate` gains Nether and End cases (fortress, bastion, nether fossil, the Nether's ruined portal, end city, and each dimension's biomes), run with `execute in`.
- `oracle-test` lists the new inputs.
- The seed set stays the ten golden seeds.

Specs and ADRs that govern this: [noise.md](../specs/noise.md), [rng.md](../specs/rng.md), [density-functions.md](../specs/density-functions.md), [biome-lookup.md](../specs/biome-lookup.md), [terrain.md](../specs/terrain.md), [structure-placement.md](../specs/structure-placement.md), [locate.md](../specs/locate.md), [search.md](../specs/search.md), [spawn.md](../specs/spawn.md), [render.md](../specs/render.md), [world-saves.md](../specs/world-saves.md), [cli-json.md](../specs/cli-json.md), [oracle.md](../specs/oracle.md), [ADR 0008](../adr/0008-game-derived-test-values-stay-local.md), [ADR 0014](../adr/0014-public-api-world-and-session.md), [ADR 0020](../adr/0020-search-query-language.md), [ADR 0021](../adr/0021-search-engine.md), [ADR 0022](../adr/0022-terrain-crate-and-volume-evaluation.md).

## Tasks

### Phase 1: the bytecode, the oracle and specs

- [x] From the 26.3 bytecode, note in the specs as drafts:
  - `RandomState`'s legacy branch;
  - `NormalNoise` legacy normalization;
  - `SimplexNoise`;
  - `EndIslandDensityFunction`;
  - `distance_to_point`;
  - `TheEndBiomeSource`;
  - the fortress, bastion, nether fossil, Nether ruined portal and end city generation points.
- [x] Oracle: `structures`, `spawn` and `heights` per dimension; Nether and End in `GOLDEN_DIMENSIONS`; Nether biome levels; probe sections; locate cases. Then run `cargo xtask oracle golden`. `oracle.md` updated. The probe sections move to phase 2, with the tests that read them, as in m07.
- [x] ADR: dimensions in the public API (`World::open_dimension`, the bundle-wide type, saves): [0023](../adr/0023-dimensions-in-the-public-api.md).
- [x] ADR: the query language's dimensions and coordinate scaling: [0024](../adr/0024-dimensions-in-the-search-language.md), extending ADR 0020 rather than superseding it, since its decisions stand. `search.md` drafted.

### Phase 2: legacy randomness and new noise

- [x] `legacy` normalization and the legacy `NormalNoise` constructor, with hand-worked tests.
- [x] Simplex noise, with hand-worked tests and probe points.
- [x] Binding with `legacy_random_source: true`: the positional factory and every special case.
- [x] `distance_to_point` and `end_outer_islands`, point and volume paths.
- [x] Ignored probe tests: every Nether and End function at points and columns, chunk digests, bit for bit.

### Phase 3: biomes

- [x] The Nether's source through `multi_noise`.
- [x] `TheEndBiomeSource`.
- [x] Biome names and palettes from the source.
- [x] `World::open_dimension`; spawn origins for both.
- [x] Golden biome tests in both dimensions.
- [ ] `check-save` against a real save's Nether and End: the test world has no Nether or End chunks yet.

### Phase 4: terrain

- [ ] Terrain binding with legacy randomness; heights and block columns in both dimensions.
- [ ] Golden height tests in both; `Session::height_at` per dimension.

### Phase 5: structures

- [ ] Per-dimension sets, the Nether's ruined portal in its own set, fortress and bastion checks, `Starts::with_terrain` for the nether fossil, the Nether's ruined portal and the end city.
- [ ] Golden structure tests in both dimensions: exact stubs, bastion layouts box for box.
- [ ] Confirmed `/locate` comparison in both dimensions.
- [ ] `structure-placement.md` and `locate.md` updated.

### Phase 6: the library and saves

- [ ] The bundle-wide type: worlds opened lazily, queries resolved across them.
- [ ] Save assessment per dimension (generator warnings, older chunks per dimension's regions); `open_world(dimension)`.
- [ ] Renders and features per dimension; slime chunks only in the Overworld.

### Phase 7: cross-dimension search

- [ ] Parser, canonical printer and resolution for dimensions and scaled anchors; round-trip and error tests.
- [ ] Evaluation and the fast path over sessions per dimension; the prefilter per dimension.
- [ ] Soundness and equality tests on fixtures, extended to cross-dimension queries; `search.md` final.
- [ ] Bench lines: a Nether fortress locate, a cross-dimension search, Nether and End binding.

### Phase 8: front ends and close-out

- [ ] CLI: `--dimension`, `--from player` across dimensions, cross-dimension search; `cli-json.md`; integration tests.
- [ ] Viewer: the dimension switch, per-dimension views, saves opening in the player's dimension, terrain layers off in the Nether.
- [ ] README, architecture.md, getting-started.
- [ ] `cargo xtask check` and `cargo xtask oracle-test`; move this plan to `completed/`. The milestone waits for m08-versions.

## Verification

- Ignored oracle comparisons in the Nether and the End:
  - probe values;
  - biome grids, ties included;
  - heights and block columns;
  - structure stubs, choices and bastion layouts;
  - confirmed `/locate` answers.

  All run by `cargo xtask oracle-test`. This is exit criterion 1.

- Hand-worked unit tests for simplex noise, legacy normalization, the End's biome rule and each new structure check, committed without game data.
- Search: soundness and equality of the fast path against `evaluate` over seed ranges, cross-dimension queries included; parser round trips.
- A manual check in the test world: a fortress, a bastion and an end city from `spyglass locate --dimension ...` where the game puts them, and a bastion's pieces against the save's stored starts, as m07 checked villages.

## Durable outputs

- [ ] `docs/specs/noise.md`: simplex noise, legacy normalization, legacy seeding
- [ ] `docs/specs/rng.md`, `docs/specs/density-functions.md`: legacy binding, `distance_to_point`, `end_outer_islands`
- [ ] `docs/specs/biome-lookup.md`: the Nether, `TheEndBiomeSource`
- [ ] `docs/specs/terrain.md`, `docs/specs/structure-placement.md`, `docs/specs/locate.md`: both dimensions
- [ ] `docs/specs/search.md`: dimensions and scaled anchors
- [ ] `docs/specs/world-saves.md`, `docs/specs/render.md`, `docs/specs/cli-json.md`, `docs/specs/oracle.md`
- [x] ADRs: dimensions in the API ([0023](../adr/0023-dimensions-in-the-public-api.md)); the query language's dimensions ([0024](../adr/0024-dimensions-in-the-search-language.md))
- [ ] `docs/architecture.md`, `README.md`, `docs/getting-started.md`

## Decisions

Agreed on 2026-10-26:

- **Two plans:** this one for the dimensions, and m08-versions, written once 26.4 nears release, for versions, the guide and CI.
- **Versions:** both 26.4 (on release, tracked through its snapshots) and 26.2 are to be supported. That's m08-versions' scope. 26.2 brings the older noise and density formats, so it's the larger part.
- **CI:** GitHub Actions, downloading the server jar from Mojang during each run, with nothing game-derived committed or kept (ADR 0008). An ADR in m08-versions.

Agreed on 2026-10-26, as recommended:

- `World` stays one dimension, with a bundle-wide type over it, rather than a `World` holding all three.
- A constraint's dimension is inferred from its target, explicit only when ambiguous; anchors convert by coordinate scale; `spawn` in the Nether is the Overworld spawn divided by 8.
- Terrain layers are off in the Nether rather than redefined (a "floor below y" heightmap would be new, game-less semantics).

## Notes

- 2026-10-26: Phase 3 done but for `check-save`, which needs a save with Nether and End chunks. Every golden biome cell matches: the Nether's 42,598,400 cells (8,064 tied, every tie set equal) and the End's 42,598,400, and the 30 spawn files. `BiomeSource` is multi-noise or the End's; biome names moved to `BiomeNames`, which structure loading takes instead of the parameter list. `World::open_dimension` reads each dimension's type from the preset. Terrain settings accept legacy randomness (refusing it only with aquifers, which no vanilla dimension combines), a phase 4 task done early because opening a world loads terrain.
- 2026-10-26: Phase 2 done. All 19,660 density probe cases match bit for bit: 14,660 in the Overworld, 2,240 in the Nether and 2,760 in the End, now including the router's inline entries. Every density function in the 26.3 bundle binds. Skipping 17,291 draws instead of 17,292 for the End's islands fails 548 cases. The binder takes a legacy flag (`Binder::with_legacy_random`) and the climate router reads it; terrain settings still refuse legacy until phase 4. Legacy normalization is ported but only hand-tested, since no 26.3 noise uses it.
- 2026-10-26: Phase 1 done. The golden set now covers all three dimensions (biomes, structures, spawn, heights), and `oracle locate` asks Nether and End cases with `execute in` (locate format 2, a dimension per case; the Rust comparison skips other dimensions until phase 5). Specs have drafts for everything below. From the 26.3 bytecode, beyond the plan's expectations:
  - Legacy randomness changes less than in 1.21: the positional factory, `old_blended_noise` from `LegacyRandomSource(seed)`, and nothing else. **There's no shift override any more.** The Nether biome noises have their own construction, keyed on their ids, not on the legacy flag. `end_outer_islands` is always legacy-seeded.
  - **Biomes don't depend on y** in the Nether (only temperature and vegetation, with `y_scale` 0) or the End (`end/islands` is y-independent). So every Nether and End biome check is decided from the climate. Fortresses, bastions and the Nether's ruined portals need no terrain to decide; nether fossils and end cities need terrain only for their y and terrain rejections.
  - `SimplexNoise` returns `f32` in 26.3. `end_outer_islands` lost its central-island term, which `end/islands` now adds in data with `distance_to_point`.
  - Every `ruined_portals` placement chunk in the Nether gets a Nether portal: the six Overworld portals fail their biome checks there.
  - The Nether's heights are 128 everywhere at the origin (the ceiling) and the End's 0 off the islands, as expected.
- 2026-10-26: Started, with the decisions agreed as recommended.
- 2026-10-26: Drafted from milestone 08, the code at 0ef3687, the 26.3 bundle's Nether and End data, and 26.2 and 26.4-snapshot-5 bundles extracted for comparison (in `bundles/`, gitignored).
