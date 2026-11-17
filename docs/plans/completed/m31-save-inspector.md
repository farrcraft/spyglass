# m31. What spyglass read from a save

- Status: Completed
- Milestone: [31. What spyglass read from a save](../../roadmap/completed/31-save-inspector.md)
- Started: 2026-11-16
- Completed: 2026-11-16

## Goal

Turn `SaveReport` from a list of warnings into an explanation: every value spyglass took from a save, where it came from, what it decides, and how the answers used it. One library value carries it, and `spyglass world info` and a viewer panel both show it.

This plan covers all three of the milestone's exit criteria:

- `world info` and the viewer's panel give the same report for a real 26.3 save with data packs, and for one with chunks from an older version;
- every warning a save can raise appears in the report beside what it's about, with a test per warning;
- the user guide's [your own worlds](../../guide/src/your-worlds.md) page describes the report.

It reads nothing new about worldgen and makes no new predictions, so it needs no oracle comparison. What it reports is checked against the saves themselves.

## What spyglass reads today

From [world-saves.md](../../specs/world-saves.md) and `crates/spyglass/src/save.rs`:

- **`spyglass_world::Save`** has the name, layout, data version, version name, seed, `generate_structures`, each dimension's generator, the enabled and disabled packs, `WasModded`, the spawn and the single player. It doesn't record which file and NBT path each value came from. The layout decides that ([the table](../../specs/world-saves.md#layouts)).
- **`Save::world_border(dimension)`** reads a dimension's border, or reports it unreadable. Only the portal search uses it, and no report shows it.
- **`SaveReport`** has the `SaveInfo` summary, the matched bundle, `warnings`, the applied `packs`, and per dimension the folders and, after `Scan::Chunks`, the `ChunkIndex` (each chunk's data version, status and `InhabitedTime`).
- **Data packs** show as names, hashes and `worldgen_files` counts. What a pack replaces, adds or hides isn't worked out anywhere. `Bundle::with_packs` layers them and answers through `resource` and `registry_ids`, without saying which layer a file came from.
- **Saved starts and containers** are read on demand, chunk by chunk, by `SaveChunks` for the queries that need them ([ADR 0071](../../adr/0071-answers-from-a-save.md)). Nothing counts them for the whole save.
- **Front ends:** `world info` prints the summary and then the warnings one after another. The viewer prints the warnings in the toolbar's notice row and has no panel for the save.

## Approach

Four parts: the report value, the counts the scan adds, the packs' changes, and the front ends with the guide.

### The report

`SaveReport::details() -> SaveDetails` (`spyglass::save`, a new `details.rs` beside it) builds the full report from what `inspect_save` already read. It opens the bundle with the packs once, as `pack_errors` does today, and folds `pack_errors` in. `SaveDetails` has four sections, and each item carries the warnings about it:

- **`settings`:** one `SaveSetting` per value, each with:
  - its value;
  - its `source`: the file relative to the save, and the NBT path in it, e.g. `data/minecraft/world_gen_settings.dat` `data.seed`, or `level.dat` `Data.WorldGenSettings.seed` in the Legacy layout;
  - `decides`: a short fixed text from the library, e.g. "every prediction" for the seed, "which bundle answers" for the data version, "`--from spawn` and the spawn origin" for the spawn, "where portals can be found and built" for the border.

  The values are the seed, the version and data version, `generate_structures`, the generator per dimension with the preset it matches, the spawn, the single player, the world border per dimension (moving or not, and unreadable or not), and `WasModded`.

  `spyglass_world::Save` gains the sources as `Save::sources: SaveSources`, filled by each layout's reader where it reads the value. The fields themselves don't change.

- **`packs`:** each enabled pack in load order, `vanilla` included, with what spyglass did with it (applied, a feature pack from the bundle, or not applied and why), and for an applied pack its changes ([below](#what-a-pack-changes)).
- **`dimensions`:** one per dimension: the folder, whether spyglass answers in it and why not, and after a chunk scan what it holds ([below](#what-the-save-holds)).
- **`warnings`:** every `SaveWarning`, as today, each with `about`, a `SaveSubject` naming the item it's beside:
  - `NoBundle`: the version setting.
  - `UnsupportedGenerator`: that dimension's generator setting.
  - `StructuresDisabled`: `generate_structures`.
  - `DataPack`: that pack.
  - `Modded`: `WasModded`.
  - `OlderChunks`, `UnreadableRegions`: that dimension's holdings.

  A pack error (`pack_errors`) becomes a new warning, `SaveWarning::PackError { dimension, pack, file, message }`. It's about the pack whose layer provides the file the error names, and blocks answers in its dimension as the error does today. A moving world border becomes a note, `SaveWarning::MovingBorder { dimension }`, beside that border, since answers use the size it ends at. Both join the [warnings table](../../specs/world-saves.md#warnings). So every warning has one subject, and a front end draws each one beside its subject, not in a list of its own.

`SaveWarning` is a public enum without `#[non_exhaustive]`. Adding variants breaks matches outside the crate. The CLI and viewer are the only callers, and the crate is pre-1.0, so the variants are added and the changelog says so.

### What the save holds

The chunk scan (`region::index`, `nbt::read_root_fields`) already decompresses every chunk and skips through its tree. It gains two counts in the same pass, read by a skipping visitor that builds no tree, under the same limits:

- **Structure starts:** the keys of `structures.starts` whose child's `id` isn't `INVALID`, by structure id, as [world-saves.md](../../specs/world-saves.md#structure-starts-in-saves) reads them. A chunk saved before data version 2970, or with no data version, is counted apart, as "starts the game data-fixes and spyglass predicts". Whether a key names a structure the registry has is checked when the report is built, against the bundle with packs. A start a removed pack placed is counted as dropped, as the game drops it.
- **Sealed loot containers:** block entities in `block_entities` that have a `LootTable`, by block entity `id`. These are containers no one has filled yet, so predictions about them stand if the seed matches. Chest minecarts in `entities/` aren't counted. Counting them would mean scanning the entity region files as well, and the report says so.

`ChunkFields` gets `starts: Vec<(String, StartCount)>` and `sealed_containers: u32`. The scan's time on the largest save on the development machine (344,000 chunks, 4.6 GB, about 6 to 8 s) is measured before and after. If the counts add more than a quarter to it, they move behind their own `Scan::Holdings` level and `world info --quick` stays as fast as it is.

Per dimension, `SaveDetails::dimensions[d].holdings` then gives:

- chunks: total, full, proto (generated partway), unreadable, and unreadable region files;
- chunks by data version, newest first, each named by the bundle with that data version where there is one (`26.3`), or else by number. No table of old versions is added: a bundle says its own data version ([world-saves.md](../../specs/world-saves.md#matching-a-bundle-and-a-preset));
- the time players spent there (`InhabitedTime` summed), and the bounds of the generated chunks;
- structure starts from the save, by structure: certain, and answered from the save instead of the seed. Starts in pre-2970 chunks are counted apart;
- sealed loot containers, by type.

**Opened containers.** Today a container is called opened only when a query predicts a container at that spot and the save has no `LootTable` there ([world-saves.md](../../specs/world-saves.md#containers-in-saves)). A save can't tell an opened structure chest from a chest a player placed. Counting opened containers for the whole save would mean predicting every container in every generated chunk, which costs a decoration per chunk. So the report counts the sealed ones from the save, and the answers count the rest as they're made:

- `SaveChunks` gets a usage tally, `SaveChunks::usage() -> SaveUsage`: chunks read, starts answered from the save, chunks whose save said "no start here", and containers answered unopened, opened or gone.
- The viewer's panel shows the tally for its session ("so far, this map used …").
- `world info` shows only the save's own counts, since it makes no queries. The milestone's scope is reworded to say this.

### What a pack changes

`Bundle::with_packs` gains `Bundle::layer_changes() -> Vec<LayerChanges>` (`spyglass-data`), one per pack, worked out from the layers' file lists without parsing them:

- per registry (`worldgen/biome`, `worldgen/noise_settings`, `worldgen/structure`, `loot_table`, `tags/worldgen/biome`, …): ids **replaced** (the layers below have them), ids **added**, and tags **merged** or **replaced** (`"replace": true`, read from the tag file);
- what its `filter` **hides** of the packs below it;
- files a **higher** pack replaces or hides in turn, so they never take effect;
- structure templates, counted the same way.

The library maps each registry to the answers it feeds, with a fixed table in `spyglass` (`answers_fed_by(registry)`): biomes, terrain, structures, features, loot, flowers, portals. A pack's line can then say "changes biomes, terrain and structures". Registries spyglass doesn't read (`recipe`, `advancement`, functions) are counted as "not read by spyglass", so a pack that only adds recipes says it changes no answers.

### Front ends and the guide

- **CLI:** `spyglass world info` prints the four sections in order. Each warning is printed under the line it's about, marked `error`, `warning` or `note` as today. A save with nothing to warn about ends with today's "exact where they say so" line. `--quick` leaves out the holdings, and says so.

  `--json` keeps every field it has today and adds `settings`, `packs` (replacing `applied_packs`, which is kept one release and marked deprecated in [cli-json.md](../../specs/cli-json.md)) and `dimensions`. Each warning gains `about`, `{kind, dimension?, pack?, setting?}`. `world list` is unchanged.

- **Viewer:** a **World** panel in the side panel, present while a save is shown. It has collapsible sections for settings, packs, dimensions and the session's usage. A warning sits under its item in the warning colour, and a note in the muted colour. Clicking the spawn, the player or a dimension's generated bounds moves the view there. The toolbar's notice row shrinks to one line, "Showing the save X: 2 warnings", which opens the panel. The panel reads the same `SaveDetails`. It's built on the inspect worker after `inspect_save`, so opening a save doesn't stall the UI.
- **Guide:** [your-worlds.md](../../guide/src/your-worlds.md) gets a section on reading the report, with a `world info` example from a save with a pack and older chunks, and the warnings list rewritten as "where each warning appears". [viewer.md](../../guide/src/viewer.md) gets the World panel, and [data-packs.md](../../guide/src/data-packs.md) gets the pack's changes.

## Tasks

- [x] Spec first: [world-saves.md](../../specs/world-saves.md) gets a _Report_ section (the four sections, the source of every setting in each layout, what each decides, the holdings and how they're counted, the subject of each warning). It also gets the two new warnings. [data-packs.md](../../specs/data-packs.md) gets _What a pack changes_.
- [x] `spyglass_world::Save::sources`: the file and NBT path of every value, for the three layouts, with a test per layout.
- [x] The scan's counts: a skipping visitor in `spyglass_data::nbt` for `structures.starts` and `block_entities`, `ChunkFields`' new fields, and unit tests on chunks built in the test (`INVALID`, missing `id`, pre-2970 chunks, empty `starts`, containers with and without `LootTable`, proto chunks). Fuzz it through the existing `region` target.
- [x] Time the scan on the largest local save, before and after. Decide between keeping the counts in `Scan::Chunks` and adding `Scan::Holdings`, and record the times in the spec.
- [x] `Bundle::layer_changes`, with tests on layered bundles: a replacement, an addition, a filter, a tag merged and replaced, a file shadowed by a higher pack, a pack of recipes only.
- [x] `answers_fed_by`, the registry-to-answers table, with a test that every registry a loader reads has an entry.
- [x] `SaveWarning::PackError` and `SaveWarning::MovingBorder`, and `SaveWarning::subject`. `pack_errors` is kept as a thin wrapper over the new warning until the front ends move.
- [x] `SaveReport::details() -> SaveDetails`, with a test per warning kind (all nine) that it appears once, beside its subject, on saves built in the test.
- [x] `SaveChunks::usage`, counted where `StoredStarts` and the container lookup already answer, with a test that a locate and a loot query move the right counts.
- [x] CLI: `world info`'s text and `--json`. [cli-json.md](../../specs/cli-json.md) gets `settings`, `packs`, `dimensions` and `about`, and the CLI tests cover each section and `--quick`.
- [x] Viewer: the World panel, the shorter notice, moving the view from the panel, and the session's usage. Checked by eye with the `run-viewer` skill.
- [x] Ignored tests on the machine's saves: every launcher save's `details()` builds. In every answerable 26.3 save no older version touched, the start counts match what `SaveChunks` reads for the chunks within 48 of the spawn origin.
- [x] The exit criterion's two real saves: one 26.3 save with a data pack (Terralith or the fixture pack) and one with chunks from an older version. `world info --json` and the panel's `SaveDetails` are compared field for field in an ignored test.
- [x] Guide: [your-worlds.md](../../guide/src/your-worlds.md), [viewer.md](../../guide/src/viewer.md), [data-packs.md](../../guide/src/data-packs.md). The changelog notes the new `SaveWarning` variants and the deprecated `applied_packs`.
- [x] Update the milestone: reword the scope's opened-containers item ([above](#what-the-save-holds)) and tick the exit criteria.

## Verification

- **Warnings:** a test per warning kind, on a save built in the test, that `details()` places it beside the right subject, and that `world info`'s text prints it under that line.
- **Counts:** unit tests on built chunks for every rule of the start and container counts. The ignored test on local saves checks that the scan's start counts agree with `SaveChunks`' on-demand reads.
- **Packs:** `layer_changes` on built packs, and on Terralith 2.6.5 (from the gitignored `packs/`), whose known changes are in [data-packs.md](../../specs/data-packs.md#a-widely-used-pack): 39 vanilla biomes replaced, 95 added, 29 structures, 183 templates (the spec's earlier 40, 97, 50 and 300 were wrong).
- **Same report:** the CLI's JSON and the viewer's `SaveDetails` for the two real saves are compared in an ignored test, and by eye in the panel.
- **Cost:** the scan's time on the largest local save, before and after, in the spec.
- `cargo xtask check` passes.

## Durable outputs

- [x] [world-saves.md](../../specs/world-saves.md): the report, the settings' sources, the holdings and their counting, the warnings' subjects, `PackError` and `MovingBorder`.
- [x] [data-packs.md](../../specs/data-packs.md): what a pack changes, and the registry-to-answers table.
- [x] [cli-json.md](../../specs/cli-json.md): `world info`'s new fields, and `applied_packs` deprecated.
- [x] An ADR if the counts need their own scan level, or if `SaveWarning` is made `#[non_exhaustive]` to stop the next variant from breaking callers.
- [x] The user guide's [your-worlds.md](../../guide/src/your-worlds.md), [viewer.md](../../guide/src/viewer.md) and [data-packs.md](../../guide/src/data-packs.md).

## Notes

Newest first.

- 2026-11-16: Completed. `cargo xtask check` passes; the plan touched no worldgen, so no `oracle-test` run. Everything durable is in world-saves.md, data-packs.md, cli-json.md, ADR 0077 and the guide.
- 2026-11-16: [ADR 0077](../../adr/0077-open-ended-public-enums-are-non-exhaustive.md): open-ended public enums are `#[non_exhaustive]`. Applied to `SaveWarning`, `PackConcern`, `SaveSubject`, `Setting` and `PackUse`; `Answer` stays exhaustive. The scan counts needed no level of their own (14% slower), so no ADR for that. `SaveSubject` gained `kind`, `setting`, `pack` and `dimension`, which the CLI's JSON uses instead of a `match`.
- 2026-11-16: The exit criterion's pack save: "Terralith Test", generated in 26.3 with Terralith, in the Modrinth App's Fabric profile, so it's also modded and enables `fabric-convention-tags-v2`. `local_report.rs` now compares on it instead of the copy, expecting those two warnings besides Terralith's three `pack_error`s, and Terralith's own saved starts (`terralith:underground/mining_outpost`) count as answered from the save, not dropped. In the viewer, a save blocked in every dimension wasn't shown at all, so its World tab never appeared; now the report is kept (`unshown_save`), the error has **What spyglass read**, and the tab shows it without **Go** buttons. Checked by eye on this save. The panel now marks registries spyglass doesn't read, as the CLI does.
- 2026-11-16: The World tab checked by eye with the `run-viewer` skill on "Modrinth Latest upgraded": the notice row reads "Showing the save Modrinth Latest, made in Minecraft 26.3: 3 warnings." with **What spyglass read**, which opens the World (3) tab. Settings show their sources; the three warnings sit under modded, the mod's pack and the Overworld's holdings in the warning colour; the spawn's **Go** moved the map to 0, 0. With mineshafts, ocean ruins and ruined portals drawn, the tally read 113 chunks read, 80 starts from the save and 33 chunks with none. The panel's font draws `--from` with an en dash.
- 2026-11-16: The two real saves. "Modrinth Latest upgraded" (26.3, 19,761 of 21,636 Overworld chunks from data version 4790) and a copy of "Portal Link Test" with Terralith put in `datapacks/` and enabled in `level.dat` give the same report from `world info --json` and `details()`, field for field (`spyglass-cli/tests/local_report.rs`). The copy stands in for a world generated with a pack: none of the machine's saves has one. Terralith showed `layer_changes` counting its 18 `.json.disabled` structure files, which the game doesn't list; a file now counts only with its registry's extension. It also showed the spec's Terralith counts were wrong (39, 95, 29 and 183, not 40, 97, 50 and 300; 173 old templates, not 290). Its three `pack_error`s land under the pack, naming `data/terralith/structure/mage/tower.nbt`. The local-saves test: 193 reports build, and the scan's starts match the whole-chunk reader's in 29,720 chunks of five saves.
- 2026-11-16: The World tab is built and compiles clean with clippy, but isn't checked by eye yet: the viewer opened behind other windows on the desktop, so the run-viewer skill couldn't see or click it. It's a side-panel tab, not a separate panel, shown only while a save is, and the notice row is one line with a **What spyglass read** button. The report is built on the inspect worker after `inspect_save`.
- 2026-11-16: A pack error rarely names a file: the density function loader's names the resource by id (`minecraft:zero: type: unsupported density function type`). So the report finds the pack by a `data/` path in the message, else by an id one of the packs has a file for, else by a namespace only packs have. The registry test found two registries the draft table missed, `worldgen/carver` and `tags/instrument`. `MovingBorder` is raised by `inspect_save` (the borders are three small files), `PackError` only by `details()`.
- 2026-11-16: The counts are read by a new tree-free walker, `nbt::walk`, which `read_root_fields` now uses too. On hermitcraftvanilla-s5 (4.5 GB) `world info --json` went from 6.3 to 7.1 s to 7.1 to 7.9 s, three runs each, twice: about 14%, so the counts stay in `Scan::Chunks` and no ADR is needed. Reading `Level` for pre-1.18 chunks also gives them their `Status` and `InhabitedTime`, which the scan missed before. 120 s of `cargo xtask fuzz region` found nothing.
- 2026-11-16: Drafted. Counting opened containers for the whole save would mean predicting every container in every generated chunk, so the report counts sealed containers from the save, and answers count the opened ones as they're made (`SaveChunks::usage`). The milestone's scope needs rewording to match.
