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

- **One code path per algorithm.** Versions differ at load time, not in copies of code.
- **Loaders read every supported version's data, dispatching on the data's own shape**: type ids and field names. Most format changes say which form they are (`argument1` or `left`, `cache_2d` or `cache`, `firstOctave` or `base_octave`). A form from no supported version is still an error that names the file and JSON path.
- **Behavior the data doesn't reveal is a named switch.** Switches live in one table, keyed by data version. An example: `SimplexNoise` returns `f32` from data version N.
  - Code reads switches, never data versions.
  - Each switch names its spec section.
  - A tool prints any bundle's switches.
- **A version's support is removable.** Dropping a version deletes the dispatch arms and switches only it used. The switch table shows which those are.
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
2. Run `cargo xtask version-diff`.
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
- **Which snapshots are supported:**
  - Releases from 26.2 on, plus the latest snapshot.
  - When a newer snapshot lands, the older one's switches stay but its bundle and goldens go.
  - On release, 26.4 replaces its snapshots.
  - The policy is in the ADR on supported versions.
- **Defaults prefer releases.** The CLI and the viewer pick the newest _release_ bundle unless a snapshot is asked for by name. That fixes the trap that pinned the CLI tests.

### CI

Decided on 2026-10-26: GitHub Actions, downloading the server jar from Mojang on each run, with nothing game-derived committed or kept. The ADR records the details:

- A matrix over the supported versions. Each job runs, in a fresh runner:
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

- [ ] Bytecode diffs, 26.2 to 26.3 to 26.4-snapshot-5, for every class spyglass ports:
  - `RandomState`, `NormalNoise`, `PerlinNoise`, `SimplexNoise`;
  - every density function, `NoiseRouter`, `NoiseChunk` and the aquifer;
  - `EndIslandDensityFunction`, `TheEndBiomeSource`, `Climate` and the spawn search;
  - each structure type's generation point, jigsaw assembly, `getBaseHeight` and `getBaseColumn`.

  Each difference is drafted as a _Versions_ note in its spec.

- [ ] Where 26.4's default block comes from, now that `default_block` is gone.
- [x] `cargo xtask version-diff`, with tests on fixture bundles. Its report on 26.2 → 26.3 and 26.3 → 26.4-snapshot-5 must name every change listed above.
- [ ] ADR: how versions coexist (shape dispatch and behavior switches).
- [ ] ADR: the oracle harness across versions (core and adapters).
- [ ] ADR: supported versions, snapshots, release-first defaults, and CI on GitHub Actions.
- [ ] Timing: goldens and locate for one version on 4 cores, as CI will have.

### Phase 2: version-aware infrastructure

- [ ] The behavior switch table, keyed by data version, printed by `version-diff` for any bundle.
- [ ] `spyglass-oracle` lists the versions with inputs. Every ignored comparison runs for each, and `oracle-test` reports per version.
- [ ] The harness split into core and a 26.3 adapter, with no change in output: 26.3's goldens stay byte-identical.
- [ ] Release-first defaults in `find_bundle`, the CLI and the viewer. Snapshots are chosen only by name. The CLI tests stop pinning 26.3.

### Phase 3: 26.4-snapshot-5, the first update

- [ ] A harness adapter for 26.4 (the nine compile errors); goldens for 26.4-snapshot-5.
- [ ] `upscale`, with per-axis cell sizes and modes. 26.3's `interpolated` loads as the same thing with linear modes, so there's one implementation.
- [ ] The default block from wherever 26.4 keeps it.
- [ ] `ice_caves` in the palette.
- [ ] `oracle-test` passes for 26.3 and 26.4-snapshot-5. Cost logged; a retro if it wasn't cheap.

### Phase 4: 26.2's formats and behavior

- [ ] Noises from `firstOctave` and `amplitudes`. The amplitude formula is checked against every `base_amplitude` 26.3 stores for the same noise.
- [ ] Density functions: `argument1`/`argument2`, `cache_2d`, `flat_cache`, `cache_once`, `shifted_noise`, `y_clamped_gradient`, `end_islands` and the older `interpolated`.
- [ ] 26.2's noise-settings keys, and its aquifer from the router's fields.
- [ ] The behavior switches phase 1 finds, each with a hand-worked test and its spec note.
- [ ] A 26.2 harness adapter. The 26.2 probe passes bit for bit in all three dimensions.

### Phase 5: 26.2 end to end

- [ ] Goldens for 26.2 in all three dimensions. Biome, height, structure, spawn and `/locate` comparisons pass, confirmed included.
- [ ] Anything left: structures, templates or tags that 26.2 loads differently. None is expected beyond data.
- [ ] Cost logged for 26.2, the first full version added by the guide.

### Phase 6: snapshots while the plan runs

- [ ] Each 26.4 snapshot released before the plan completes: the loop, a cost-log row, and a retro and follow-up task where it wasn't cheap. Snapshot 6 is likely on 2026-10-27.

### Phase 7: CI

- [ ] `cargo xtask ci --game-version <v>`, the steps a CI job runs, passing locally for each supported version.
- [ ] The GitHub Actions workflow: the version matrix and `cargo xtask check`.
- [ ] A first green run on GitHub. Blocked until the repository is published there, which is the user's decision. Exit criterion 2 waits for it.

### Phase 8: the guide and close-out

- [ ] `docs/adding-a-version.md`, followed for 26.2 and the snapshots, with the cost log. Exit criterion 3.
- [ ] Specs' status lines and _Versions_ notes for every supported version.
- [ ] `data-sources.md`, `oracle.md`, README (supported versions), architecture.md.
- [ ] Bench lines per version.
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

- [ ] ADRs: how versions coexist; the harness across versions; supported versions, snapshots and CI.
- [ ] `docs/adding-a-version.md`, with the cost log.
- [ ] _Versions_ notes in `noise.md`, `rng.md`, `density-functions.md`, `biome-lookup.md`, `terrain.md`, `structure-placement.md`, `spawn.md` and `locate.md`.
- [ ] `docs/specs/data-sources.md`, `docs/specs/oracle.md`, `docs/specs/bundle-format.md` (if the bundle changes)
- [ ] `README.md`, `docs/architecture.md`, `docs/getting-started.md`

## Decisions

Agreed on 2026-10-26:

- **Both versions now.** 26.2 and 26.4 are planned together, with 26.4 tracked through its snapshots instead of waiting for its release. Rework from snapshot changes should be cheap. When it isn't, that's evidence for improving how spyglass handles versions, and the plan acts on it.
- **CI:** GitHub Actions, downloading the server jar from Mojang during each run, with nothing game-derived committed or kept (agreed with m08-dimensions).

To be decided in phase 1's ADRs, with the recommendations above: how versions coexist, the harness's adapters, and supported versions with release-first defaults.

Open for the user: publishing the repository on GitHub, which exit criterion 2 needs.

## Notes

- 2026-10-26: Drafted from the 26.2, 26.3 and 26.4-snapshot-5 bundles, a diff of their formats, opening each in spyglass, and compiling the oracle harness against each jar.
- 2026-10-26: `cargo xtask version-diff` added. Its reports name every data change above, and more the survey missed: 26.2's `default_block` and `default_fluid` are block-state objects where 26.3 has strings, 26.2's `spawn_target` has another shape, every structure template changed between each pair of versions (1,212 and 1,511 files), and 26.3's heightmap tag `blocks_motion_in_heightmap` doesn't exist in 26.2. A first pass over the bytecode found the 26.3 → 26.4-snapshot-5 change isn't as small as the data suggests: 61 ported classes differ, and at least two changes alter results with no sign in the data (`Mth.sin` rounds its table index and is symmetric for negative arguments; `Climate.Parameter`'s distance adds 1).
