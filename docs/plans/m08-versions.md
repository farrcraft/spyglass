# m08. More than one game version

- Status: In progress
- Milestone: [08. Nether, End and multiple versions](../roadmap/08-dimensions-and-versions.md)
- Started: 2026-10-26
- Completed: not yet

## Goal

Support 26.2, 26.3 and 26.4 side by side, all three dimensions each, matching the oracle for every one. 26.4 is tracked through its snapshots until it's released. This plan covers the milestone's other two exit criteria:

- **At least two versions pass the full oracle suite in CI.**
- **A guide documents how to add a version, and has been followed at least once.**

Following each 26.4 snapshot is also the test of how spyglass is built for versions. An update should be cheap. When one isn't, the plan records why and changes the structure so the next one is (see [Tracking 26.4](#tracking-264)).

## Facts established up front

These come from the code at 8bea3c9, the 26.2 (data version 4903), 26.3 (5023) and 26.4-snapshot-5 (5125) bundles, and compiling the oracle harness against each jar.

- **Versions now.**
  - The latest release is 26.3.
  - The latest snapshot is 26.4-snapshot-5, released 2026-10-20. Snapshots have come out weekly, on Tuesdays, so snapshot 6 is likely on 2026-10-27.
- **Only 26.3 opens today.** Each dimension's first failure:
  - 26.2: `shifted_noise` in the Overworld and the Nether, `cache_2d` in the End.
  - 26.4-snapshot-5: `default_block` is missing in all three.

  Loading stops at the first error, so these aren't the whole list. The rest below come from a diff of the bundles.

- **26.4-snapshot-5 against 26.3 is small.**
  - `interpolated` becomes `upscale`, with per-axis `x`, `y`, `z` objects of `cell_size` and `mode` (`interpolate_linear`). That's 13 uses in density functions and 8 inline in noise settings, including the Nether's and End's `final_density`.
  - `default_block` is gone from all seven noise settings. Where the default block comes from now is a bytecode question.
  - Biome `minecraft:ice_caves` is new: the Overworld parameter list changed, and three biome tags were added.
  - Unchanged: structures, structure sets, template pools and templates.
  - Dimension types gain only an attribute spyglass doesn't read.
  - The harness has **9 compile errors**: `BiomeResolver.getNoiseBiome`, `NoiseColumn`, `getBaseHeight`'s signature, `GenerationContext.biomeResolver`, and `Structure.generate`.
- **26.2 against 26.3 is large: it's the 1.21 format family.**
  - **Density functions:**
    - Binary operations take `argument1` and `argument2`.
    - `cache_2d`, `flat_cache` and `cache_once` stand where 26.3 has `cache` alone.
    - `shifted_noise` and `y_clamped_gradient` stand where 26.3 has `gradient`.
    - `end_islands` stands where 26.3 has `end_outer_islands` with `distance_to_point` and `slice`.
    - `interpolated` takes its older fields.
  - **Noises** are `firstOctave` and `amplitudes`. 26.3 stores `base_octave`, `octave_count`, `amplitude_modifiers` and a precomputed `base_amplitude`.
  - **Noise settings:**
    - `noise.size_horizontal` and `size_vertical`.
    - `aquifers_enabled` and `ore_veins_enabled`.
    - The router holds `barrier`, `fluid_level_floodedness`, `fluid_level_spread`, `lava`, `vein_*` and `preliminary_surface_level`. 26.3 has an `aquifers` object and `chunk_surface_level`.
    - `surface_rule`, which spyglass doesn't read.
  - **Structures:**
    - No abandoned camps: 18 structures, one set, 57 pools and 299 templates fewer.
    - Five structure tags exist only in 26.2, and 18 only in 26.3.
    - Every template differs, at least by its data version.
  - **Biomes:** no `dappled_forest`. Biome files carry spawn fields spyglass doesn't read. Features and carvers are `configured_*`, also unread.
  - **The harness** has **95 compile errors**. 26.3's density evaluation API (`DensityBuffer`, `DensityVolume`, `SamplerContext`, `DensitySamplerSet`), `NoiseSpawnFinder` and `synth.Noise` don't exist in 26.2, so the probe, biome, height and spawn code need a 26.2 path.
- **Likely algorithm changes the data doesn't show, all for 26.2.** To confirm in the bytecode:
  - `RandomState`'s legacy shift override and noise construction.
  - `SimplexNoise` returning `double`; it returns `f32` in 26.3.
  - `NormalNoise`'s value factor computed from the amplitude list.
  - The 1.21 aquifer, built from router fields.
  - `EndIslandDensityFunction` with its central-island term.
- **26.3 is hard-wired into about 67 places in code**, mostly test, bench and doc paths. Every ignored oracle test reads `bundles/26.3` and `oracle-out/26.3`.
- **The oracle per version:**
  - Biome goldens take about a minute, heights about 90 s, and locate several minutes (it runs a server).
  - `oracle-out/26.3` is 322 MB.
  - A CI job per version fits a hosted runner. It must regenerate everything each run (ADR 0008).
- **The front ends already handle versions.** Bundles are chosen with `--game-version` or the viewer's version list, and saves by data version.
  - However, "the newest bundle" is now a snapshot. The CLI tests had to pin 26.3 for this reason (m08-dimensions phase 8).
- **The repository isn't on GitHub**, so CI can't run there until it's published. That's the user's call.

## Approach

### How versions coexist

To be decided in an ADR in phase 1. The recommendation:

- **One code path per algorithm, with one exception.** Versions differ at load time, not in copies of code. The exception, from the phase 1 survey: 26.2's density evaluation is a different algorithm family (all `f64`, no volume arithmetic, caches and interpolation only in `NoiseChunk`), so it gets an evaluator of its own beside 26.3's, sharing the RNG, the noise primitives where they agree, the loaders and everything above density. Smaller differences inside a family stay switches.
- **Loaders read every supported version's data, dispatching on the data's own shape**: type ids and field names. Most format changes say which form they are (`argument1` or `left`, `cache_2d` or `cache`, `firstOctave` or `base_octave`). A form from no supported version is still an error that names the file and JSON path.
- **Behavior the data doesn't reveal is a named switch.** Switches live in one table, keyed by data version. An example: `SimplexNoise` returns `f32` from data version N.
  - Code reads switches, never data versions.
  - Each switch names its spec section.
  - A tool prints any bundle's switches.
- **Support is permanent, snapshots included.** Dispatch arms, switches, families and harness adapters accumulate and are never deleted, so each says which versions use it.
- **Specs describe each algorithm once**, with a _Versions_ note wherever it differs.

The alternatives the ADR weighs:

- Per-version modules or crates: copies that drift apart.
- Converting old data to the new format at extraction: lossy where the algorithms differ.
- Version checks scattered through the code: no single place to see or remove them.

### The oracle across versions

Also an ADR, extending [ADR 0004](../adr/0004-oracle-standalone-harness.md). The recommendation:

- A version-independent core.
- Small adapters for each range of game API the core calls. `xtask` picks the adapter by the jar's data version and compiles core plus adapter.
- 26.4's nine differences should fit one small adapter. 26.2's sampler API gets its own.

How big each adapter turns out is itself a measure of the harness's design.

Tests follow the same idea:

- `spyglass-oracle` lists every version that has both a bundle and oracle outputs.
- Every ignored comparison runs for each of them, instead of naming 26.3.
- `cargo xtask oracle-test` reports per version.

### Tracking 26.4

The loop for each snapshot:

1. Extract the snapshot.
2. Run `cargo xtask version-diff --code` (after `cargo xtask oracle build` for the new jar): data and bytecode.
3. Compile the harness.
4. Fix what changed.
5. Regenerate the goldens.
6. Run `oracle-test`.
7. Record the cost.

The pieces:

- **`cargo xtask version-diff <old> <new>`** is the durable form of this plan's survey. It reports what changed in the data spyglass reads: type ids and fields, checked against the forms the loaders accept, so a new form shows up before any loader fails. It also reports registry additions and removals. It answers "what must change?" in seconds.
- **The cost log** is a table in the guide, one row per version or snapshot. Each row records:
  - the date;
  - what changed;
  - the files touched, by area: loaders, switches, algorithms, harness adapters, specs, other;
  - the time taken;
  - the oracle result.
- **Cheap** means about a day or less, confined to loaders, switches, harness adapters and spec notes. Any other update gets a short retro in this plan's notes: what forced the work, and what change to the structure would have made it cheap. That change becomes a task here, or an ADR when it's a design change. The point is to learn, not to hit a number.
- **Which versions are supported: all of them, permanently.** Snapshots are versions of their own, which people play and keep worlds in, not a step to throw away when the release ships. Every version added stays supported: its bundle, its oracle comparisons, its CI job and its cost-log row. New versions are added as they come out, from 26.2 and 26.4-snapshot-5 on; older ones, 26.4's first four snapshots included, can be added later by the guide. The policy is in [ADR 0028](../adr/0028-supported-versions-and-ci.md).
- **Switches and adapters hold data-version ranges**, not a starting version, because a later snapshot can revert a change.
- **Defaults prefer releases.** The CLI and the viewer pick the newest _release_ bundle unless a snapshot is asked for by name, or a save's data version matches it. That fixes the trap that pinned the CLI tests. It decides only the default; snapshots are as supported as releases.

### CI

Decided on 2026-10-26: GitHub Actions, downloading the server jar from Mojang on each run, with nothing game-derived committed or kept. The ADR records the details:

- A matrix over every supported version, which only grows. Every version runs on every push and pull request until the matrix takes over an hour of wall-clock time; then an ADR picks a cheaper schedule. Each job runs, in a fresh runner:
  1. extract the bundle;
  2. `cargo xtask oracle golden` and `oracle locate`;
  3. `cargo xtask oracle-test`.
- One `cargo xtask check` job.
- `cargo xtask ci --game-version <v>` runs the same steps locally, so the workflow is tested before the repository is public.

### The guide

The guide is `docs/adding-a-version.md`. It covers:

- the loop above;
- what `version-diff` reports and where each kind of change is fixed;
- adding a switch, a dispatch arm or a harness adapter;
- the cost log.

It's followed for 26.2 and for each snapshot. That's exit criterion 3.

Specs and ADRs that govern this: [data-sources.md](../specs/data-sources.md), [bundle-format.md](../specs/bundle-format.md), [oracle.md](../specs/oracle.md), [noise.md](../specs/noise.md), [rng.md](../specs/rng.md), [density-functions.md](../specs/density-functions.md), [biome-lookup.md](../specs/biome-lookup.md), [terrain.md](../specs/terrain.md), [structure-placement.md](../specs/structure-placement.md), [spawn.md](../specs/spawn.md), [locate.md](../specs/locate.md), [ADR 0004](../adr/0004-oracle-standalone-harness.md), [ADR 0008](../adr/0008-game-derived-test-values-stay-local.md), [ADR 0010](../adr/0010-typed-loading-of-worldgen-json.md), [ADR 0016](../adr/0016-extractor-crate-and-read-only-data.md).

## Tasks

### Phase 1: survey, tools and decisions

- [x] Bytecode diffs, 26.2 to 26.3 to 26.4-snapshot-5, for every class spyglass ports:
  - `RandomState`, `NormalNoise`, `PerlinNoise`, `SimplexNoise`;
  - every density function, `NoiseRouter`, `NoiseChunk` and the aquifer;
  - `EndIslandDensityFunction`, `TheEndBiomeSource`, `Climate` and the spawn search;
  - each structure type's generation point, jigsaw assembly, `getBaseHeight` and `getBaseColumn`.

  Each difference is drafted as a _Versions_ note in its spec.

- [x] Where 26.4's default block comes from, now that `default_block` is gone: the material rule's last entry (see notes).
- [x] `cargo xtask version-diff`, with tests on fixture bundles. Its report on 26.2 → 26.3 and 26.3 → 26.4-snapshot-5 must name every change listed above.
- [x] ADR: how versions coexist (shape dispatch and behavior switches): [0026](../adr/0026-how-game-versions-coexist.md), accepted.
- [x] ADR: the oracle harness across versions (core and adapters): [0027](../adr/0027-oracle-harness-across-versions.md), accepted.
- [x] ADR: supported versions, snapshots, release-first defaults, and CI on GitHub Actions: [0028](../adr/0028-supported-versions-and-ci.md), accepted.
- [x] Timing: goldens and locate for one version on 4 cores, as CI will have.

### Phase 2: version-aware infrastructure

- [x] The behavior switch table, each switch holding the data-version ranges it applies to, printed by `version-diff` for any bundle. `spyglass_data::versions`: five switches so far (four for 26.4, one for 26.2), each named in its spec note.
- [x] A test that every supported bundle on the machine loads and binds in all three dimensions, so old dispatch arms keep working as new ones are added. Moved to phase 3: it would fail by design until 26.4 loads, and `oracle-test` now runs `each_dimension_opens` for every version, which covers most of it. Done by that: `each_dimension_opens` and `every_bundled_density_function_loads` run for every bundle on the machine under `oracle-test`, and for every supported version in CI under `cargo xtask ci`.
- [x] `spyglass-oracle` lists the versions with inputs. Every ignored comparison runs for each, and `oracle-test` reports per version. 36 tests for 26.3, all passing; 26.2 and 26.4-snapshot-5 fail to load, as expected until phases 3 to 5.
- [x] The harness split into core and a 26.3 adapter, with no change in output: 26.3's goldens stay byte-identical. `GameApi` in the core; `oracle/adapters/26.3/` (`Adapter`, `ProbeWorldgen`) chosen by its `data-versions`. Against the other jars every compile error is now in the adapter: 9 for 26.4-snapshot-5, about 90 for 26.2.
- [x] Release-first defaults in `find_bundle`, the CLI and the viewer. Snapshots are chosen only by name. The CLI tests stop pinning 26.3. `bundle.json` gains an optional `kind` (old bundles go by their id); a folder of only snapshots is `Error::NoRelease`, naming them; the viewer falls back to the newest snapshot, which its picker shows. `cli.rs` no longer pins; `world.rs` still does, since its saves are stamped with 26.3's data version and it reads 26.3's files. The synthetic fixture and test saves moved from data version 1 to 5023, so that every behavior switch is off for them.

### Phase 3: 26.4-snapshot-5, the first update

- [x] A harness adapter for 26.4 (the nine compile errors); goldens for 26.4-snapshot-5. `oracle/adapters/26.4/` extends the 26.3 adapter and replaces only its `Calls.java` (six calls; ADR 0029). The harness's own tie search needed the half-open ranges too.
- [x] `upscale`, with per-axis cell sizes and modes, on both paths. It isn't bit-identical to 26.3's `interpolated` (weights, axis order, per-block y), so both stay, chosen by type id.
- [x] Half-open climate parameters (`max` scores 1) for biome fitness, box distances and spawn, as a switch. Applied at load: over integers `[min, max)` is `[min, max - 1]`, and `(t - max) + 1` is `t - (max - 1)` in wrapping arithmetic, so fitness, the tree and spawn are unchanged code.
- [x] Heights from solid blocks only for every heightmap but the world-surface ones, as a switch; the ruined portal and mineshaft call sites follow. For spyglass's two heightmaps this means `OCEAN_FLOOR_WG` counts solid without the tag, and an empty column's height is its own bottom.
- [x] NaN spline coordinates return NaN, and `Math.clamp`'s signed zero, as switches if a test can tell them apart. Both baked in at load.
- [x] No default block in 26.4: solid cells take the material rule's block, and the overworld rules end in stone. `default_block` is optional by shape; a column's solid blocks stay `Block::Default`, unnamed for 26.4.
- [x] `ice_caves` in the palette.
- [x] `oracle-test` passes for 26.3 (36 tests) and 26.4-snapshot-5 (31; the other five are pinned to 26.3). Cost logged; a retro if it wasn't cheap: see the notes.

- [x] From the 26.4 retro: `cargo xtask version-diff --code <old> <new>`, the class-level bytecode diff (javap, normalized) of the classes spyglass ports, so an update's algorithm changes show up as fast as its data changes.

### Phase 4: 26.2's formats and behavior

- [x] Noises from `firstOctave` and `amplitudes`: `NormalNoise`'s value factor from the amplitude list, `f64` `SimplexNoise`, and the `f64` 1.21 `BlendedNoise`.
- [x] The 1.21-family evaluator: every 26.2 density type (`argument1`/`argument2`, `cache_2d`, `flat_cache`, `cache_once`, `shifted_noise`, `y_clamped_gradient`, `end_islands` with its central island, `old_blended_noise`, splines on narrowed coordinates), all in `f64`, with `NoiseChunk`'s caches and `lerp3` interpolation and its `flat_cache` window.
- [x] Climate sampling in `f64` narrowed to `f32`, the End source on it, and the spawn targets as parameter points. Every 26.2 golden biome cell (ties included) and spawn file matches. The golden biomes come through `NoiseChunk.cachedClimateSampler`, so chunk generation's climate path equals the point path spyglass uses.
- [x] 26.2's noise-settings keys (`default_block` and `default_fluid` as block states, `size_horizontal`/`size_vertical`, `aquifers_enabled`, `ore_veins_enabled`), its 15-field router, and its aquifer from the router's fields with the exclusion test in code. Settings load as `Family::V1_21`; `final_density()` and `bound()` return `None` for it, and columns go through a per-cell `NoiseChunk`. All 320 of 26.2's aquifer probe cases match.
- [x] Ore veins in block columns. `OCEAN_FLOOR_WG` counts vein blocks; every 26.2 height golden (6,135,300 values) matches, and 26.3's and 26.4-snapshot-5's still do.
- [x] A 26.2 harness adapter. The 26.2 probe passes bit for bit in all three dimensions. Adapter done: `oracle/adapters/26.2/`, standalone; 26.2's golden set (52,370 tied overworld cells, 8,064 in the Nether) and locate answers generate, and 26.3's stay byte-identical. The probe passes: density, climate and aquifer cases in every dimension.

### Phase 5: 26.2 end to end

- [x] Goldens for 26.2 in all three dimensions. Biome, height, structure, spawn and `/locate` comparisons pass, confirmed included. `oracle-test --game-version 26.2` passes. `/locate` for `#minecraft:abandoned_camp`, a 26.3 tag, is rejected by both the game and spyglass, and the comparison now checks that they reject the same targets.
- [x] Templates' `Name`/`Properties` palettes, feature pool elements' `minecraft:bottom` jigsaws, and 26.2's heightmap predicates without the `blocks_motion_in_heightmap` tag. Loaders and switches done (`feature_jigsaw_named_bottom`, `heightmap_blocks_motion`, which supports the three vanilla default blocks and refuses others); checked against the game with phase 5's goldens.
- [x] Cost logged for 26.2, the first full version added by the guide. In the guide's cost log, with 26.4-snapshot-5 and 26.4-snapshot-6.

### Phase 6: snapshots while the plan runs

- [ ] Each 26.4 snapshot, pre-release and release candidate released before the plan completes, added for good: the loop, its bundle and goldens kept beside the earlier snapshots', every earlier version's comparisons still passing, a cost-log row, and a retro and follow-up task where it wasn't cheap. Snapshot 6 is likely on 2026-10-27.
- [ ] 26.4-snapshot-6 (data version 5126, released 2026-10-27). Data: nothing spyglass reads changed (templates only in their `DataVersion`, biome feature order, block tags spyglass doesn't read). Bytecode: 20 ported classes changed, 11 added.
  - [x] Harness: `RandomState.create` split into `createForGenerator` (anchors from the level) and `createForTesting`. `GameApi.randomState` takes the level's height; the calls live in the 26.3 adapter's `RandomStates.java`, and `oracle/adapters/26.4-snapshot-6/` extends 26.4 with only that file.
  - [x] `planUpsample` tests a stepped volume's offset against the step, not the cell size: switch `upscale_offset_by_step`, with a hand-worked test (it only shows for steps that aren't powers of two).
  - [x] Goldens and `oracle-test` for 26.4-snapshot-6 (31 tests); every earlier version still passing.
  - [x] Forms the vanilla data doesn't use yet, implemented and checked by the oracle through a test data pack (decided 2026-10-27, [ADR 0030](../adr/0030-a-test-pack-for-forms-vanilla-doesnt-use.md)):
    - [x] `gradient` coordinates as vertical anchors, resolved against the level's heights when bound (`VerticalAnchor` moved down to `spyglass-data`; `Binder::with_anchors`, and the climate router, terrain, spawn targets and corner constraints bind with their dimension's heights).
    - [x] `domain_warp`: constant offsets, the flat-xz sampler and the general one, point and volume paths, with the game's `domainAxes` and its slicing of the warp's volume.
    - [x] `corner_constraint` on pool elements: density predicates at a piece's corners, rejecting starts (no stub) and children (after they take their space).
    - [x] `oracle/testpack/` and `cargo xtask oracle test-pack`: the harness's `--pack`, a merged bundle in `target/oracle/bundles/<version>+testpack/`, and `oracle-test` treating it as a version. 22 tests pass: 23,560 density cases, 1,544 constrained village starts assembled and 4,164 rejected.
  - [x] Cost and retro: see the notes. Logged in the guide's cost log when it exists (phase 8).
  - [x] From the retro: `version-diff` ignores a template change that's only its `DataVersion`, and `--code` lists codec types and fields a version registers that its data doesn't use, so a snapshot's unused forms show up in the report. Templates that differ only in their root `DataVersion` are counted apart ("1511 only in DataVersion"). `--code` finds each `register` and `fieldOf` call's string constant in the disassembly, compares against every type id and key in the new bundle's JSON, prints the forms new since the old version and writes all of them to `target/version-diff/<version>-unused.txt`. On snapshot 5 → 6 it names exactly `domain_warp`, its `offset_*` fields and `corner_constraint` with its predicate's fields; anchor coordinates show only as the added `GradientFunction$Coordinate$YAnchor` class, since they reuse `VerticalAnchor`'s fields.
- [ ] Older forms the vanilla data doesn't use, found by that list, that spyglass doesn't load yet: the `dimension_origin` placement type (26.3 on), `override_liquid_settings` on single pool elements, and the `checkerboard` biome source. Each with test-pack cases, as for snapshot 6's forms, or a recorded reason not to.

### Phase 7: CI

- [x] `cargo xtask ci --game-version <v>`, the steps a CI job runs, passing locally for each supported version. Extract (replacing the bundle), `oracle golden`, `oracle locate`, `oracle test-pack` where the pack loads (`oracle/testpack/data-versions`, `5126..`; `test-pack` now refuses other versions), then `oracle-test` for the version and its `+testpack`, where a comparison missing an input the job made is an error, not a skip. Passing on this machine (16 cores): 26.2 in 29.8 min, 26.3 in 8.9, 26.4-snapshot-5 in 8.9, 26.4-snapshot-6 in 12.1 with the test pack.
- [x] The GitHub Actions workflow: the version matrix and `cargo xtask check`. `.github/workflows/ci.yml`: a `check` job (the viewer's system libraries, the JDK `jdk-status` names, `scripts/setup.sh --oracle`, `cargo xtask check`) and an `oracle` job per version (the JDK, `cargo xtask ci`). Not yet run anywhere: GitHub needs the repository.
- [ ] A first green run on GitHub. Blocked until the repository is published there, which is the user's decision. Exit criterion 2 waits for it.

### Phase 8: the guide and close-out

- [x] `docs/adding-a-version.md`, followed for 26.2 and the snapshots, with the cost log. Exit criterion 3. Written from those three additions: the loop, reading `version-diff`, where each kind of change is fixed, switches, dispatch arms, adapters, the test pack, pinned tests, finishing, and the cost log with a row for each. The first version added from the guide as written will be the next snapshot (26.4-snapshot-7, likely 2026-11-03); its row should say what the guide got wrong.
- [x] Specs' status lines and _Versions_ notes for every supported version. Status lines in `rng.md`, `noise.md`, `biome-lookup.md`, `locate.md`, `spawn.md` and `terrain.md` now say 26.2 and the 26.4 snapshots are checked; `world-saves.md` still says its notes are unchecked, which is true (saves aren't compared per version).
- [x] `data-sources.md`, `oracle.md`, README (supported versions), architecture.md. Also the docs index and getting-started.
- [x] Bench lines per version. `cargo xtask bench <version>` now takes a version and defaults to the newest release, not `bundles/26.3`; lines in the notes.
- [ ] `cargo xtask check` and `cargo xtask oracle-test` for every version. Move this plan to `completed/`, and milestone 08 too once CI has run.

## Verification

- The ignored oracle comparisons for every supported version, run by `cargo xtask oracle-test`:
  - probe values;
  - biome grids, ties included;
  - heights and block columns;
  - structure stubs, layouts and confirmed `/locate`.

  In all three dimensions.

- 26.3's goldens byte-identical across the harness split. 26.3's comparisons unchanged throughout.
- Hand-worked tests for each behavior switch and each new dispatch arm, committed without game data.
- `version-diff` tested on fixture bundles. Its reports on the real bundles must name every change in the facts above.
- CI green on GitHub for at least two versions.
- The cost log, with a retro for every update that wasn't cheap.

## Durable outputs

- [x] ADRs: how versions coexist; the harness across versions; supported versions, snapshots and CI. 0026, 0027, 0028, and 0029, 0030 besides.
- [x] `docs/adding-a-version.md`, with the cost log.
- [x] _Versions_ notes in `noise.md`, `rng.md`, `density-functions.md`, `biome-lookup.md`, `terrain.md`, `structure-placement.md`, `spawn.md` and `locate.md`.
- [x] `docs/specs/data-sources.md`, `docs/specs/oracle.md`, `docs/specs/bundle-format.md` (if the bundle changes)
- [x] `README.md`, `docs/architecture.md`, `docs/getting-started.md`

## Decisions

Agreed on 2026-10-26:

- **Both versions now.** 26.2 and 26.4 are planned together, with 26.4 tracked through its snapshots instead of waiting for its release. Rework from snapshot changes should be cheap. When it isn't, that's evidence for improving how spyglass handles versions, and the plan acts on it.
- **26.2 stays in this plan** (2026-10-26, after the phase 1 survey showed it needs an `f64` evaluator family of its own). The ADR on how versions coexist records the second family.
- **Support is permanent, and every snapshot is a version** (2026-10-26). Once a version is supported it stays supported, snapshots included: people play specific snapshots and keep worlds in them. Nothing is dropped when a newer snapshot or the release lands.
- **CI:** GitHub Actions, downloading the server jar from Mojang during each run, with nothing game-derived committed or kept (agreed with m08-dimensions).

Accepted on 2026-10-26: [ADR 0026](../adr/0026-how-game-versions-coexist.md) (how versions coexist), [ADR 0027](../adr/0027-oracle-harness-across-versions.md) (the harness across versions) and [ADR 0028](../adr/0028-supported-versions-and-ci.md) (supported versions and CI).

Open for the user: publishing the repository on GitHub, which exit criterion 2 needs.

## Notes

- 2026-10-26: Drafted from the 26.2, 26.3 and 26.4-snapshot-5 bundles, a diff of their formats, opening each in spyglass, and compiling the oracle harness against each jar.
- 2026-10-26: `cargo xtask version-diff` added. Its reports name every data change above, and more the survey missed: 26.2's `default_block` and `default_fluid` are block-state objects where 26.3 has strings, 26.2's `spawn_target` has another shape, every structure template changed between each pair of versions (1,212 and 1,511 files), and 26.3's heightmap tag `blocks_motion_in_heightmap` doesn't exist in 26.2. A first pass over the bytecode found the 26.3 → 26.4-snapshot-5 change isn't as small as the data suggests: 61 ported classes differ, and at least two changes alter results with no sign in the data (`Mth.sin` rounds its table index and is symmetric for negative arguments; `Climate.Parameter`'s distance adds 1).
- 2026-10-26: Timing for 26.3 with `--threads 4` on this machine (16 cores, so other work still had cores): `oracle golden` 481 s, `oracle locate` 73 s, with the jar cached and the harness compiled. The regenerated `oracle-out/26.3` is byte-identical to the previous one. A hosted 4-vCPU runner is slower per core and adds the jar download, extraction and compilation, so plan on roughly 15 to 20 minutes per version job.
- 2026-10-26: Bytecode survey of every ported class, 26.2 → 26.3 → 26.4-snapshot-5, in four parts (math, noise, RNG and climate; density, router and aquifer; biomes, heights and spawn; structures). Working notes are in `target/version-survey/` (local, uncommitted); the _Versions_ notes in the specs come next. The plan's premise that 26.4 is small and 26.2 is mostly loaders doesn't hold:
  - **26.4-snapshot-5 changes results the data doesn't show.** `Climate.Parameter`'s distance becomes half-open (`max` itself scores 1), which changes biome fitness, the tree's box distances and spawn fitness. `upscale` with linear modes isn't bit-identical to `interpolated` on either path (weights `(1−t)·a + t·b`, point order z→x→y, per-block y on the volume path), so it's a new implementation, not a renamed one. `getBaseHeight` is gone, and `getFirstFreeHeight` counts only solid blocks for every heightmap but the world-surface ones, so `MOTION_BLOCKING` stops counting fluids. A NaN spline coordinate returns NaN. `Mth.sin`/`cos` change too, but spyglass doesn't port them.
  - **The default block** in 26.4 is the material rule's last entry: the overworld rules now end in stone. Solid cells are a marker until the rule runs; heights don't need a block.
  - **26.2 is a different evaluator, not a different format.** Its density functions, noises (`SimplexNoise` returns `f64`; `BlendedNoise` is the `f64` 1.21 code; `NormalNoise`'s factor from the amplitude list), climate sampling and End islands are all `f64`. It has no volume arithmetic; caches and interpolation live only in `NoiseChunk` (`lerp3` x then y then z; `flat_cache` quantized to the quart corner inside the chunk's window, which makes column and chunk answers differ). `y_clamped_gradient` and `end_islands` (central island on /8-quantized coordinates) aren't 26.3's formulas. The aquifer is the same algorithm with router inputs and an exclusion test in code. Columns gain ore veins.
  - **Unchanged in all three:** every RNG class, seeding and the positional factories, placement and the weighted structure pick, jigsaw assembly, the spawn search loop, the biome zoom. Every noise class is unchanged from 26.3 to 26.4.
  - **Loader-only for 26.2:** template palettes use `Name`/`Properties`; feature pool elements' jigsaws are named `minecraft:bottom` (same results with vanilla data); `spawn_target` is full parameter points.
- 2026-10-26: Phase 1 done but for accepting the three ADRs. The survey is drafted into the specs as _Versions_ notes (inline where a version differs at a point, a `26.2` section where it differs throughout: noise, density functions, terrain, spawn), each spec's status line saying they're unchecked against the oracle. ADRs 0026 to 0028 are proposed. Open points the notes flag as inferred: 26.2's chunk-path climate sampler against its point path, how often 26.2's quantized deep-dark test flips a column, and 26.4's per-block biomes needing every neighbour's quart biomes.
- 2026-10-26: Support is permanent, snapshots included: snapshots are versions people play, not steps to discard on release. ADR 0028 now says so, ADR 0026's switches and ADR 0027's adapters hold data-version ranges (a snapshot can revert a change), and nothing is ever removed. Phase 6 keeps every snapshot it adds.
- 2026-10-26: Phase 3 done. 26.4-snapshot-5 passes every oracle comparison, 26.3 is unchanged (its goldens regenerate byte-identical through both harness reworks). Ties nearly vanish in 26.4: 1,008 tied cells against 60,361, because half-open ranges no longer share their boundary values. Open: 26.4 changed `DensityVolume.maxBlock` to `min + (size - 1)·step`; the terrain crate's aquifer still uses 26.3's form, which no 26.4 comparison caught, so either it doesn't matter for the regions the aquifer sees or no case exercises it. Checked: every region the aquifer gets is a block volume (step 1, from `Volume::blocks`), where both forms are `min + size - 1`, so it can't differ.
- **Retro, 26.4-snapshot-5 (not cheap).** About a working day of agent and author time, well over the "loaders, switches, adapters, spec notes" budget: a new density type with two evaluation paths (`upscale`, ~1,500 lines with tests), a new harness mechanism (ADR 0029), and four switches. What forced it: the snapshot changed algorithms, not just data, in four places, and the harness was written against one version's API. What would have made it cheaper: the harness split (now done, so the next snapshot should cost a `Calls.java`), and `version-diff` reporting bytecode changes in ported classes as well as data, since the data diff alone said "small" for an update that wasn't. Follow-up task below.
- 2026-10-26: 26.2 terrain matches the oracle: heights, aquifers and ore veins. Two choices rest on the bytecode alone, since the oracle can't tell them apart: fluid spread in `f64` (26.3's `f32` gives the same answers on every case), and each column's aquifer from its own `NoiseChunk` rather than its chunk's. The deep-dark exclusion test is pinned by the data: without it 8 of 320 probe cases and one block column differ.
- 2026-10-26: Phases 4 and 5 done for 26.2 but for its cost row: `cargo xtask oracle-test --game-version 26.2` passes every comparison. The only failure on the first run was the locate test unwrapping a target 26.2 doesn't have (`#minecraft:abandoned_camp`); the game rejects it too, so the test now requires both to reject the same targets.
- 2026-10-27: 26.4-snapshot-6 (data version 5126) out today, added. The data diff said nothing spyglass reads changed: 1,511 templates differ only in their `DataVersion`, 57 biomes in feature order, 9 block tags spyglass doesn't read. `version-diff --code` found 20 changed and 11 new ported classes. The harness failed to compile at `RandomState.create`, split into `createForGenerator` (anchors from the level) and `createForTesting`: a 26.4-snapshot-6 adapter of one file (`RandomStates.java`, moved out of the 26.3 adapter so it can be replaced alone). One change can alter vanilla results: `planUpsample` tests a stepped volume's offset against the step (switch `upscale_offset_by_step`). With those, all 31 comparisons passed in about 40 minutes of work. The rest of the bytecode change is forms the vanilla data doesn't use: `domain_warp`, anchor gradient coordinates and `corner_constraint`; carvers and trunk placers, which spyglass doesn't port, changed too.
- **Retro, 26.4-snapshot-6 (cheap for vanilla, not for the unused forms).** Vanilla parity cost an adapter file and a switch, the "cheap" budget, which the harness split and `version-diff --code` bought. Supporting the three unused forms, as ADR 0026 asks, took most of a day: a new density type with three samplers and two paths, the game's `domainAxes`, anchor heights through every binder, a density sampler in jigsaw assembly and the structure check, and the test pack to check them (ADR 0030). What forced it: the game shipped new data forms ahead of using them, and only the bytecode showed them. What would have made it cheaper: seeing them in the report at once (`--code` listed the classes, but not "new codec types and fields"), and a data-pack-shaped way to check unused forms, which now exists. Follow-up task above. One trap worth recording: a concurrent `oracle-test` run recompiles whatever is in the tree, so edits during a regression run invalidate it.
- 2026-10-27: `cargo xtask ci` passes for all four versions. 26.2's job is three times the others' because of heights: the game writes 26.2's height goldens in 11 minutes against 26.3's 2.5 (its per-cell `NoiseChunk` path), and 26.2's comparisons take about 11 minutes against 3. Locate is 1 to 4 minutes per version. On a 4-vCPU runner expect 26.2 to be the longest job, likely past ADR 0028's 15 to 20 minute estimate but within its one-hour matrix budget, since jobs run in parallel.
- 2026-10-27: Bench per version (`cargo xtask bench <version>`, this machine). 26.3, 26.4-snapshot-5 and 26.4-snapshot-6 are within noise of each other. 26.2 is several times slower throughout: one-thread witch-hut search 403 seeds/s against 2,866 for 26.3; `locate_structure #village` 605 ms against 72; `height_at` on a bound session 3.1 ms against 0.39; `biome_at` over 256 × 256 quarts 694 ms against 321; a 1024² render 14.4 s against 3.5. That's its `f64` family with none of the newer evaluator's volume paths and caches, and a `NoiseChunk` per column. Correct, but a follow-up for milestone 09 (fast density): the 1.21 family needs the same treatment.
