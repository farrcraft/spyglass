# m25. What a save already knows

- Status: In progress
- Milestone: [25. What a save already knows](../roadmap/25-what-the-save-knows.md)
- Started: 2026-11-11
- Completed:

## Goal

Every exit criterion of milestone 25: for a world save, show which chunks are generated, answer structure locates from the save's own structure starts where they're generated, and say for each loot container in a generated chunk whether it's been opened and what it holds. All three are described in the user guide.

## Approach

Today a save is read twice, both times shallowly ([world-saves.md](../specs/world-saves.md#region-files)): `inspect_save` with `Scan::Chunks` reads every chunk for its root `DataVersion` alone (`nbt::read_root_int`) and keeps only counts, and the session that answers queries never sees the save again. This plan keeps a per-chunk index from that scan, and reads whole chunks on demand where a query needs them.

### The game's own behaviour, which this follows

- **Structure starts are read from disk.** `/locate`'s check of a candidate (`StructureCheck.checkStart`) asks storage first (`tryLoadFromStorage`, through `ChunkScanAccess.scanChunk`): a chunk on disk answers with its saved `structures.starts`, and only a chunk that isn't there is predicted from the seed. So for generated chunks the save's starts are the game's answer, including chunks an older version or a data pack generated. To confirm in the bytecode before relying on it: which chunk statuses count, and the data version below which a stored chunk is ignored and predicted instead.
- **A container keeps its loot table until something fills it.** A chest or barrel placed with a loot table stores `LootTable` and `LootTableSeed` in its block entity (`RandomizableContainer`), and drops both when it's filled: opened by a player, emptied by a hopper, broken. Chest minecarts store the same in `entities/` region files (since 1.17), not in the chunk.

### Crates

- **`spyglass-world`** (reading, no worldgen):
  - `region.rs`: the scan records, per chunk, its data version, `Status` and `InhabitedTime`, read in the same pass as today (`nbt::read_root_int` generalised to a few root fields). It keeps a `ChunkIndex` per dimension: for each region file, its 1,024 locations and the recorded fields, so a chunk can later be read again without scanning.
  - A `chunk.rs` reading one chunk's tree, capped as chunks are today ([Limits](../specs/world-saves.md#limits)), and returning its structure starts (structure id, start chunk, pieces' boxes, references) and its loot containers (block, id, position, `LootTable`, `LootTableSeed`, `Items`). `entities/` region files give chest minecarts the same way.
- **`spyglass`** (the public API):
  - `SaveReport` gains the index from `Scan::Chunks`; `Scan::Quick` leaves it empty, and the CLI's `load` scans chunks when a query will use them.
  - A `SaveChunks` for one dimension, opened from the report, attached to a session (`Session::with_save`). The final shape is the ADR's call; the rule is that a session without one answers exactly as today.
  - `Session::generated_chunks(min, max)`: each generated chunk in an area, with its status, whether an older version generated it, and its inhabited time.
  - `LocatedStructure` and `Session::structures_in`'s starts gain where the answer came from: predicted, or the save. A candidate start chunk that's generated is decided by the save: its start is certain if stored, and absent if not.
  - `FoundContainer` gains its state in the save: not generated yet, unopened (with whether the stored seed is the predicted one), filled (with the save's `Items`), or gone (the chunk is generated but the container isn't there).
- **`spyglass-viewer`**: a **Generated** layer toggle, and the save's answers in the locate panel and container contents.
- **`spyglass-cli`**: `spyglass render --generated` (an error without `--world`); `locate structure --world` and the loot commands print and emit in JSON where each answer came from ([cli-json.md](../specs/cli-json.md)).

### The map layer

Drawn after the underground layers and before slime chunks ([render.md](../specs/render.md#layers)), at scales up to 16 as slime chunks are: each generated chunk blended with one colour for a full chunk and another for a proto chunk (any status before `minecraft:full`), and hatched where an older version generated it. With inhabited time on, full chunks are shaded by it on a log scale instead. The colours and formulas go in render.md.

### Decisions for an ADR

Answers that come from a save rather than from prediction are a correctness policy, so an ADR records:

- that the save wins wherever the game would read it, and only there (the confirmed rules above);
- how a session gets a save, and that a session without one is unchanged;
- what "certain" means for a save's answer: certain about the save as it is, not about what a fresh world would generate.

## Tasks

- [x] Confirm `StructureCheck.tryLoadFromStorage`'s rules in the 26.3 bytecode: statuses read, the data-version cut-off, what an empty `starts` means; and when `RandomizableContainer` drops `LootTable`
- [x] Spec first: world-saves.md sections for the chunk index, structure starts and containers in saves
- [x] The region scan records version, status and inhabited time per chunk, at today's cost: 6 to 8 s on the 4.6 GB save before and after, byte-identical `world info`
- [x] `ChunkIndex` and the one-chunk reader in `spyglass-world` (`region::index`, `region::read_chunk`, `chunk::read`, `chunk::read_entities`), with unit tests on chunks built with `nbt::write`
- [x] The region fuzz target reads every chunk's starts and containers too: 2 minutes, 4,068 runs, nothing found
- [x] ADR: answers from a save ([0071](../adr/0071-answers-from-a-save.md))
- [x] `SaveChunks` (from `SaveReport::chunks`), `Session::with_save` and `SaveChunks::generated_in` (on the save, not the session: it needs no seed)
- [x] Structure answers from the save, in `locate_structure` and `structures_in`, with their source (`StructureStart::from_save`), through `spyglass_structure::StoredStarts` in `Starts::select`
- [x] Containers' save state in `containers_in`, `containers_holding` and `nearest_container`, chest minecarts included (`FoundContainer::saved`, `InSave`); `holds` and `contents_certain` follow the save
- [x] The map layer in `Session::render_with`, render.md, and `spyglass render --generated` (`Layers::generated`, `MapFeatures::generated`, in JSON too). Magenta, not the blue first tried, which vanished over oceans
- [x] The viewer's Generated toggle, and the save's answers in its panels (locate list, structure hover, container window). Not yet seen running: the screen was in use
- [x] CLI text and JSON for the answers' sources; cli-json.md: `saved` and `source` for structures, `saved` for containers, `generated` on maps
- [x] Inhabited-time shading: `Layers::inhabited`, `--inhabited`, the viewer's **By time played**; the scan already reads it
- [x] User guide: your-worlds.md for all three, and the viewer and CLI pages for the new toggle and flag
- [x] CHANGELOG

## Verification

- **Unit tests** in `spyglass-world` on chunks and region files built in the test: every status, starts present, absent and `INVALID`, block entities with and without `LootTable`, chest minecarts in `entities/`, malformed and oversized chunks counted as unreadable.
- **Ignored tests on the machine's own saves**, as world-saves.md's are: every save's index builds, and in every Default 26.3 save no older version touched, every structure start the save holds is one spyglass predicts (the same chunk and structure) and every unopened container's stored seed is the predicted one. That's an oracle-free check of the predictions too.
- **In game**: `cargo xtask oracle locate --world <save>` already runs `/locate` on a copy of a save ([oracle.md](../specs/oracle.md#locate-answers)). Run it on a save with chunks from an older version, where spyglass's prediction and the save differ, and check that spyglass's save-backed answers agree with the game's.
- **By eye**: the Generated layer on a real 26.3 save in the viewer (the `run-viewer` skill), against the chunks the game shows explored.
- `cargo xtask fuzz region` for a while after the reader changes; `cargo xtask check` and `cargo xtask oracle-test`.

## Durable outputs

- [ ] world-saves.md: the chunk index, structure starts and containers in saves, the limits for whole-chunk reads
- [ ] render.md: the Generated layer
- [ ] locate.md and loot.md: answers from a save
- [ ] cli-json.md: the source fields
- [ ] An ADR: answers from a save
- [ ] The user guide

## Notes

- 2026-11-11: the game's `/locate` on a copy of "Modrinth Latest upgraded" (26.3; 19,761 of 21,636 Overworld chunks from older versions): all 100 cases agree with spyglass answering from the save, and with it all 580 26.3 cases. Without the save's chunks, one of its cases fails: the game's answer isn't among spyglass's hits, which stop at the candidate cap. The comparison test now answers a save's cases from its chunks.
- 2026-11-11: containers in the same saves, 25 × 25 chunks around the spawn origin and the player: 455 unopened, every one with the predicted table and seed; 3 filled; 13 gone. Two of the gone ones were certain: a trial chambers corridor pot at (-526, -23, -320) in "Spyglass test", beside a saved pot at (-525, -23, -320) that spyglass also predicts, and a chamber dispenser at (1133, -25, -1332) in "Test", below a saved one at (1133, -20, -1332). Nobody went there. The saved blocks answer it: the pot's block is smooth basalt among calcite and amethyst (an amethyst geode), and the dispenser's is cobblestone over mossy cobblestone (a dungeon's wall). A neighbouring chunk's feature, decorated later, overwrote each, which is the case a certain container's `certain` already excludes ("for its chunk decorated before its neighbours"), and exactly what the save can settle. Also learned: 26.3 writes a block palette as a heterogeneous list, so a block without properties is `{"": "minecraft:deepslate"}`, not `{Name: ...}`; a block reader needs both. Matching is by position alone: block entity ids aren't block ids (copper chests share one).
- 2026-11-11: the saves on this machine agree with the predictions: in the four answerable 26.3 saves with no older chunks, all 200 structure starts saved within 48 chunks of the spawn origin are certain predicted starts of the same structure, and no certain prediction in a chunk they hold is missing (`saved_structure_starts_are_the_predicted_ones`, ignored). They're Fabric profiles, so they carry the modded and data-pack warnings, which the test allows.
- 2026-11-11: the bytecode (26.3) answered the open questions. `StructureCheck.tryLoadFromStorage` reads no status and has no data-version cut-off: any stored chunk is data-fixed and its `structures.starts` decides, an empty compound meaning none; only a chunk not on disk, or one with no `starts` compound, is predicted. `ChunkMap.save` never writes an `empty` chunk without a valid start. So the save's answer is the game's for every chunk on disk except pre-1.18 ones (`Level.Structures.Starts`), whose starts the game reads through data fixers that rename structures by biome (village to `village_plains` and so on); spyglass marks those `Legacy` and doesn't answer from them.
- 2026-11-11: the scan with the index costs what it did: `world info` on the 4.6 GB save took 5.7 to 7.4 s before and 5.9 to 6.3 s after, alternating, with byte-identical JSON. Status strings are kept per chunk (a few MB on that save); intern them if memory matters in the viewer.
- 2026-11-11: drafted. The milestone's idea came from MCToolKit's chunk map and pruner; that the game itself answers `/locate` from saved starts is what makes the save's answer the right one rather than an extra, and is the first thing to confirm.
