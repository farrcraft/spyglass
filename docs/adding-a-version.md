# Adding a game version

How to add a Minecraft: Java Edition version to spyglass: a release, a snapshot, a pre-release or a release candidate, new or old. Every supported version stays supported for good ([ADR 0028](adr/0028-supported-versions-and-ci.md)), so adding one is also a promise to keep its comparisons passing.

The design this follows is in [ADR 0026](adr/0026-how-game-versions-coexist.md) (how versions coexist in the code), [ADR 0027](adr/0027-oracle-harness-across-versions.md) and [ADR 0029](adr/0029-oracle-adapters-extend-a-base.md) (the oracle harness across versions) and [ADR 0030](adr/0030-a-test-pack-for-forms-vanilla-doesnt-use.md) (the test pack). In short:

- **Loaders dispatch on the data's shape**, never on the version. One loader reads every supported version's form of a file.
- **Behavior the data doesn't show is a named switch** in `spyglass_data::versions`, on for ranges of data versions. Code reads the switch, never a data version.
- **The oracle harness is a core and adapters.** An adapter covers a range of data versions and can extend another, replacing only the files that differ.

You need the oracle set up: `scripts/setup.ps1 -Oracle` (Windows) or `scripts/setup.sh --oracle`, then `cargo xtask doctor`. See [getting-started.md](getting-started.md#6-the-oracle).

## The loop

Below, `<prev>` is the supported version closest to the new one, usually the one just before it, and `<id>` is the new version's id as Mojang names it (`26.4-snapshot-6`).

1. **Extract the bundle:** `cargo run -p spyglass-extract -- <id>`.
2. **Unpack the jar:** `cargo xtask oracle build --game-version <id>`. This unpacks the server jar before it compiles the harness, so step 3 can read the classes even when the compile fails. If no adapter covers the new data version it says so; that's expected, and step 5 deals with it.
3. **Diff:** `cargo xtask version-diff <prev> <id> --code`. This reports what changed in the data spyglass reads, the game classes spyglass ports and the codec forms the data doesn't use. [Reading the report](#reading-the-report) says what each part means and where it's fixed.
4. **Fix what changed** in the library: dispatch arms, switches, and, rarely, algorithms. Each change gets a hand-worked test, committed without game data, and a _Versions_ note in its spec.
5. **Make the harness compile** against the new jar, with a new adapter if the old one doesn't ([Harness adapters](#harness-adapters)).
6. **Run the version's CI job:** `cargo xtask ci --game-version <id>`. It re-extracts the bundle, regenerates the golden files, the `/locate` answers and the test pack's outputs, and runs every comparison, failing on any comparison that can't run ([oracle.md](specs/oracle.md#one-versions-ci-job)). Then run `cargo xtask oracle-test` with no version, so every earlier version is checked again too. A new dispatch arm or switch range can break an old version.
7. **Finish:** add the version to CI's matrix, update the specs, and log the cost ([Finishing](#finishing)).

Steps 3 to 6 repeat until the comparisons pass. When one fails, the oracle's files under `oracle-out/<id>/` and the probe's cases show where. The specs say what each comparison covers.

## Reading the report

`version-diff` prints three parts.

**The data.** For each registry it lists ids added and removed and counts files changed. For the JSON registries spyglass parses, it also lists **new shapes** and **gone shapes**: type ids by where they appear, fields with the kind of value each holds, and short enum-like strings. A new shape is a form some loader may not accept yet, so it shows up here before any load fails. Structure templates carry the data version that saved them, so every release changes them all. The report counts templates that changed only in that `DataVersion` separately, and those need nothing. Registries spyglass doesn't read are marked so.

**Switches.** The behavior switches whose state differs between the two versions. Those are the ones already in the table; a new behavior change needs a new switch, and the next part is how you find it.

**The code** (`--code`). The game classes spyglass ports, under the prefixes in `xtask/src/code_diff.rs`, disassembled and normalized so only real changes differ, listed as changed, added or removed with line counts. Read each changed class's normalized text side by side: `target/version-diff/<prev>/` against `target/version-diff/<id>/`. This is where algorithm changes the data doesn't show are found. 26.4-snapshot-5's data looked small, but its bytecode changed four results.

The code part ends with the **codec forms** the new version declares that its own data doesn't use: type ids its codecs register and fields they read, compared with every type id and key in the new bundle's JSON. Those new since `<prev>` are printed; all of them are in `target/version-diff/<id>-unused.txt`. A version can ship a form before any vanilla file uses it, and spyglass supports what a version accepts, not only what its data shows. The list is a lead, found by the string constant before each `register` and `fieldOf` call, so read the class before acting on it. A form that reuses existing field names, as 26.4-snapshot-6's anchor gradient coordinates reuse `VerticalAnchor`'s, shows only as an added class.

## Where each change is fixed

| What the report shows                                          | Where it's fixed                                                                                                                                                                                                                                                                                     |
| -------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| A new type id or field in a file spyglass reads                | A dispatch arm in the loader that owns it: density functions and noises in `spyglass-density` (`load.rs`, and `v1_21/load.rs` for 26.2's family), noise settings in `spyglass-terrain/src/settings.rs`, structures and pools in `spyglass-structure/src/load.rs`, biome sources in `spyglass-biome`. |
| A gone type id or field                                        | Nothing. The arm stays for the versions that still use it, forever. Its doc comment says which versions those are.                                                                                                                                                                                   |
| A changed class whose results change, with no sign in the data | A behavior switch ([Adding a switch](#adding-a-switch)).                                                                                                                                                                                                                                             |
| A whole algorithm family that differs throughout               | An evaluator of its own, beside the others ([ADR 0026](adr/0026-how-game-versions-coexist.md), item 3). 26.2's `f64` density evaluation is the only one so far. This is design work: write a plan first.                                                                                             |
| A new biome id                                                 | A color in the map palette, `crates/spyglass/src/render/palette.rs`.                                                                                                                                                                                                                                 |
| A codec form the data doesn't use                              | The same as a used one: a dispatch arm or a switch, then cases in [the test pack](#unused-forms-and-the-test-pack), since no golden file exercises it.                                                                                                                                               |
| A compile error in the harness                                 | A harness adapter ([Harness adapters](#harness-adapters)).                                                                                                                                                                                                                                           |
| A changed class spyglass doesn't port the behavior of          | Nothing, once you've read it. Carvers and tree placers change often, for example.                                                                                                                                                                                                                    |

### Adding a dispatch arm

A loader tells forms apart by what the JSON says: a type id (`upscale` or `interpolated`), a field's name (`argument1` or `left`) or a value's kind (a number or an anchor object). Never by the bundle's version. A form that no supported version uses stays an error that names the file and the JSON path ([ADR 0010](adr/0010-typed-loading-of-worldgen-json.md)). When two versions' forms of one type differ in meaning and not only in spelling, they are two types inside spyglass, like `upscale` and `interpolated`, chosen by the type id.

### Adding a switch

A switch is a field of `Behavior` in `crates/spyglass-data/src/versions.rs`, declared in the `switches!` table with:

- a name saying what the new behavior is (`climate_half_open`), not which version has it;
- its doc comment, describing the behavior;
- the spec section that specifies it, which a test checks exists;
- its data-version ranges: `from..` while it lasts, `from..until` if a later version reverts it, `until` exclusive.

Code reads `Behavior::of(data_version).<name>`. Crates below `spyglass-data` take the `bool` as a parameter. The spec section gets a _Versions_ note naming the switch. Ranges are stated for the supported versions: a range from 5125 means the first supported version with the change, not where the game changed it. So adding an older version means checking every switch for it.

Where possible, apply a switch at load time rather than in a hot loop. 26.4's half-open climate parameters are rewritten when the parameter list loads (over integers, `[min, max)` is `[min, max - 1]`), so the fitness code didn't change.

Each switch needs a hand-worked test that tells its two states apart. If no input can, as for 26.4-snapshot-6's `upscale_offset_by_step` with power-of-two steps, find one that does (a step that isn't a power of two), or record why not in the spec.

### Unused forms and the test pack

`oracle/testpack/` is a data pack of spyglass's own JSON. It uses forms the vanilla data doesn't, so the oracle can check them ([oracle.md](specs/oracle.md#the-test-pack)). Add cases to it for every new unused form: a named density function for a density type, or a structure set and pools for a structure form. They're sampled and compared like the vanilla ones.

The pack loads only in versions that accept every form in it. `oracle/testpack/data-versions` lists them, and `cargo xtask oracle test-pack` refuses others. If a new version adds a form that a version the pack already covers would reject, the pack can't hold both as it stands. It would need splitting into one pack per range, which `cargo xtask oracle` doesn't support yet.

## Harness adapters

The harness core (`oracle/src/`) talks to the game only through `GameApi`. Each adapter in `oracle/adapters/` implements it for a range of data versions, listed in its `data-versions` file in the same `from..`/`from..until` form as switches. `cargo xtask oracle` picks the one adapter whose ranges hold the jar's data version.

When the harness doesn't compile against the new jar:

1. Create `oracle/adapters/<id>/` with a `data-versions` file that starts at the new data version.
2. Close the previous adapter's range at that data version, so ranges don't overlap.
3. Add an `extends` file naming the previous adapter, and copy in only the files that must change ([ADR 0029](adr/0029-oracle-adapters-extend-a-base.md)). Version-sensitive calls live in small files of their own (`Calls.java`, `RandomStates.java`) so that a new adapter replaces one file. If the change you need is inside a larger file, move it into a small one in the base adapter first. Check that the base's versions still give byte-identical output.

26.4-snapshot-6's adapter is the model: an `extends` of `26.4`, and one file, `RandomStates.java`, for `RandomState.createForGenerator`.

A change in the harness core changes every version's output. After one, regenerate the goldens of every version and check that they're unchanged where they should be.

## Tests pinned to one version

A few comparisons expect one version's values exactly, for example the CLI's output for fixture saves made in 26.3. They're listed in `PINNED` in `xtask/src/oracle_test.rs` and run only for that version. Prefer a comparison that reads its expected values from the version's own oracle output. Pin a test only when its expectations come from somewhere else.

## Finishing

- **CI.** Add the version to the `oracle` job's matrix in `.github/workflows/ci.yml`, and to `CHECKED` in `crates/spyglass-data/src/versions.rs` with its data version, in the same commit; a test fails if they differ. The matrix only grows. Each version runs as two jobs, `heights` and `rest` ([ADR 0041](adr/0041-ci-runs-what-a-change-can-affect.md)). If the version's `heights` job runs close to its 60-minute timeout, as 26.2's did, split it by seed: `exclude` its `heights` entry and `include` one entry per shard (`shard: "1/2"`, `"2/2"`), as the matrix does for 26.2. Until the version is in `CHECKED`, its bundle loads with an `unchecked_version` warning ([bundle-format.md](specs/bundle-format.md#checked-versions)).
- **Specs.** Each change has a _Versions_ note in its spec. Each spec's status line lists the versions it has been checked against the oracle for.
- **The cost log** below gets a row.
- **A retro, if it wasn't cheap.** _Cheap_ means about a day or less, with changes confined to loaders, switches, harness adapters and spec notes. Any other update gets a short retro in the current plan's notes (see the [plans index](plans/README.md)): what forced the work, and what change to the structure would have made it cheap. That change becomes a task, or an ADR if it's a design change. The point is to learn, not to hit a number.

Front ends need nothing. The CLI and the viewer list every bundle. They default to the newest _release_ and open a save with the bundle whose data version matches it. A snapshot is used when it's named or when a save needs it.

## Cost log

One row per version added, oldest first. Files are counted per commit, by area. Time is as recorded at the time; it measures the work of adding the version, not the survey and tools all versions share.

| Version         | Data version | Added      | What changed                                                                                                                                                                                                                                                                                           | Files touched                                                                                                                                                                           | Time                                                                     | Oracle                               | Cheap                                                                                     |
| --------------- | ------------ | ---------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------ | ------------------------------------ | ----------------------------------------------------------------------------------------- |
| 26.3            | 5023         | 2026-10-25 | The baseline: spyglass was built on it, so every switch is off for it.                                                                                                                                                                                                                                 | —                                                                                                                                                                                       | —                                                                        | 36 comparisons pass                  | —                                                                                         |
| 26.4-snapshot-5 | 5125         | 2026-10-26 | `upscale` replacing `interpolated` (not bit-identical, so a new type on both paths); no `default_block`; `ice_caves`. Four switches: half-open climate parameters, solid-only heights, NaN splines, `Math.clamp`'s zero. Nine harness compile errors.                                                  | Algorithms and loaders: density 8, biome 3, terrain 3, API 1. Harness: a new `26.4` adapter (3), the `26.3` adapter 4, the core 3. Specs 8, ADRs 3 (0029, adapters that extend a base). | About a working day                                                      | 31 pass; 5 pinned to 26.3            | No: the bytecode changed four results the data didn't show. Led to `version-diff --code`. |
| 26.2            | 4903         | 2026-10-26 | The 1.21 family: `f64` noise and density, its own density evaluator with `NoiseChunk`'s caches and interpolation, the 15-field router and its aquifer, ore veins, older noise and settings keys, template palettes, feature jigsaw names, the heightmap rule. Two switches. 95 harness compile errors. | Algorithms and loaders: density 15, noise 11, terrain 8, biome 3, data 1, structure 1, API 1. Harness: a standalone `26.2` adapter (3). Specs 7.                                        | Commits over about 1.5 hours, after a survey shared with 26.4-snapshot-5 | 31 pass; 5 pinned to 26.3            | No: a whole evaluator family, by design (ADR 0026).                                       |
| 26.4-snapshot-6 | 5126         | 2026-10-27 | `RandomState.createForGenerator`; `upscale` plans stepped volumes by the step (one switch). Forms the vanilla data doesn't use: `domain_warp`, anchor gradient coordinates, `corner_constraint`.                                                                                                       | For vanilla parity: one adapter file and one switch. For the unused forms: density 9, structure 7, data 4, biome 2, the harness core 7, the test pack (17 files), ADR 0030.             | Vanilla parity about 40 minutes; the unused forms most of a day          | 31 pass; with the test pack, 22 more | Vanilla yes; the unused forms no. Led to the codec-form list in `version-diff --code`.    |
