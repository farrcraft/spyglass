# World saves

Status: stable. Implemented in `spyglass-world` (reading) and `spyglass` (assessment, `inspect_save`), for saves of any version; answers need a bundle with the save's data version, and the Default preset. 26.2 and 26.4-snapshot-5 differences are drafted in _Versions_ notes from the bytecode and not yet checked against the oracle.

spyglass reads a save to answer for it: its seed, version, generators, data packs, spawn and single player. It never writes to a save. Answers are only as exact as the world allows, so the assessment lists everything that makes them inexact or impossible ([warnings](#warnings)).

## Layouts

`Save::read(dir)` picks the layout by what exists. `dir` is a save if it has `level.dat`. Every `.dat` file is gzip-compressed NBT ([bundle-format.md](bundle-format.md#reading), `spyglass_data::nbt`).

| Layout      | When                                                   | Seed and generators                                                                                                 | Regions                                                     | Single player                                                                   |
| ----------- | ------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------- | ------------------------------------------------------------------------------- |
| `Current`   | `data/minecraft/world_gen_settings.dat` exists (26.x)  | that file's `data.seed`, `data.generate_structures`, `data.dimensions.<id>.generator`                               | `dimensions/<namespace>/<path>/region/`                     | `players/data/<uuid>.dat`, the UUID from `level.dat`'s `Data.singleplayer_uuid` |
| `Legacy`    | `level.dat` has `Data.WorldGenSettings` (1.16 to 1.21) | `Data.WorldGenSettings.seed`, `.generate_features`, `.dimensions.<id>.generator`                                    | `region/`, `DIM-1/region/`, `DIM1/region/`; others as above | `Data.Player`                                                                   |
| `Before116` | otherwise                                              | `Data.RandomSeed`, `Data.MapFeatures`, and `Data.generatorName` and `generatorOptions` as the overworld's generator | as `Legacy`                                                 | `Data.Player`                                                                   |

In 26.x, `WorldGenSettings` is saved data (`SavedDataType` `world_gen_settings`), which is why it moved to `data/minecraft/`. The game still reads the older layouts by datafixing them (`LevelStorageSource`).

From `level.dat` in every layout:

| Field                         | Meaning                                                                                      |
| ----------------------------- | -------------------------------------------------------------------------------------------- |
| `Data.DataVersion`            | The data version last saved in. Absent before 1.9.                                           |
| `Data.Version.Name`           | The version name, e.g. `26.3`.                                                               |
| `Data.LevelName`              | The name in the world list.                                                                  |
| `Data.DataPacks.Enabled`      | Enabled packs, in load order: `vanilla`, built-in feature packs, mods' packs, `file/<name>`. |
| `Data.WasModded`              | Whether a modded game has opened the world.                                                  |
| `Data.spawn.{pos, dimension}` | The world spawn (26.x); before, `Data.SpawnX`, `SpawnY` and `SpawnZ`, in the overworld.      |

A player's `Pos` is three doubles and `Dimension` an id (an int, 0, −1 or 1, before 1.16). The UUID file name is `UUIDUtil.uuidFromIntArray`'s four ints as hyphenated lower-case hex. Server saves have no single player.

`Data.version_history` (26.x) isn't used: in a world upgraded to 26.3 it lists only the current version.

**Generators** become JSON for comparing with bundle presets, the way `NbtOps` presents NBT to codecs: every numeric tag a number (booleans are bytes), lists and arrays as arrays, compounds as objects.

## Matching a bundle and a preset

- **Version**: the bundle whose `data_version` ([bundle-format.md](bundle-format.md)) equals the save's `Data.DataVersion`. There's no table of versions; each bundle states its own. A save with no matching bundle can't be answered for. spyglass is checked against the game only for the versions in the [oracle](oracle.md) golden set, so a bundle for another version gives unverified answers until milestone 08 ([roadmap](../roadmap/README.md)).
- **Preset**: no preset name is stored. Each dimension is checked on its own, since each has its own generator: for each of the Overworld, the Nether and the End that the bundle's `minecraft:normal` (Default) preset has, the save's generator for it is compared with the preset's, after normalizing both: ids without a namespace get `minecraft:`, booleans become 0 or 1, numbers compare by value. Only the Default preset is supported. A mismatch is reported with the first other preset (in `worldgen/world_preset/` order) whose generator for that dimension matches; one matching none (a customized superflat, for example) is described by its `type` and noise `settings`; a dimension the save has no generator for is described as `none`. Every vanilla preset has the same Nether and End, so a superflat or Amplified world still has answers in those two. Dimensions other than the three, from data packs, aren't checked: the `data_pack` warning covers them.

## Warnings

`inspect_save(dir, bundles, scan)` returns the save, its bundle, and `SaveWarning`s, most serious first, those about one dimension in the order Overworld, Nether, End. The first two block answers; the rest are shown with every answer. `SaveWarning::dimension` is the dimension a warning is about, or none for the whole save, and a blocking warning blocks only its dimension, or all three without one:

- `SaveReport::answerable_in(dimension)` is whether nothing blocks that dimension, and `answerable()` whether nothing blocks any.
- `SaveReport::open_world(dimension)` opens that dimension's `World`, or refuses with every reason that blocks it.
- Both apply the save's enabled packs, lowest first ([data-packs.md](data-packs.md#saves)); `SaveReport::pack_errors()` says which answerable dimensions they can't be applied in.
- `SaveReport::open_game()` opens the bundle as a `Game` ([ADR 0023](../adr/0023-dimensions-in-the-public-api.md)), whose `world(dimension)` refuses each blocked dimension the same way. Without a bundle, it refuses.

| Warning                 | Raised when                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| ----------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `no_bundle`             | No bundle has the save's data version, or the save is from before 1.9.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| `unsupported_generator` | A dimension's generator isn't the Default preset's.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| `structures_disabled`   | `generate_structures` (or `generate_features`, `MapFeatures`) is false: structure answers don't exist in the world.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| `data_pack`             | An enabled pack spyglass can't apply, or applies with a caveat, with its `concern` ([data-packs.md](data-packs.md#saves)): `missing` (a `file/` pack not in `datapacks/`, or whose name isn't a single file name, such as `file/../x`, which the game couldn't find either), `mod` (neither `vanilla`, a feature pack the bundle has, nor `file/`), `unreadable`, and as notes, which don't make answers inexact, `skipped` (a pack the game doesn't load either) and `other_format` (applied, though its formats don't cover the version's). Packs spyglass applies are listed in `SaveReport::packs`, not warned about. |
| `modded`                | `Data.WasModded` is set: mods can change generation without data packs.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| `older_chunks`          | With `Scan::Chunks`, per dimension: chunks in its region files whose own `DataVersion` is older than the save's (or that have none). Their terrain and structures follow the version that generated them.                                                                                                                                                                                                                                                                                                                                                                                                                 |
| `unreadable_regions`    | With `Scan::Chunks`, per dimension: region files that couldn't be opened or read, whose chunks weren't counted.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| `moving_border`         | A note, per dimension: its world border is still moving ([world borders](#world-borders)), so answers use the size it ends at.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| `pack_error`            | Only in the [report](#the-report), per dimension: an answerable dimension can't be opened with the applied packs, because a pack holds a type spyglass doesn't support or a file it can't read. Blocks answers in that dimension, as opening it fails.                                                                                                                                                                                                                                                                                                                                                                    |

A vanilla dedicated server opening a save that enables a pack it doesn't have logs `Missing data pack <name>` and carries on without it (26.3, checked 2026-10-25).

## The report

`SaveReport::details()` gives `SaveDetails`: everything spyglass took from the save, where it came from and what it decides, so a user can see why an answer for their world is what it is. It's an explanation of spyglass's own answers, not an NBT browser. `spyglass world info` prints it and the viewer's World panel shows it, from the same value. Building it opens the bundle with the save's packs once, as `pack_errors` did, which takes as long as a first query in each dimension. It has four sections, and every warning is beside the item it's about (`SaveWarning::subject`).

### Settings

One `SaveSetting` per value, each with its value, its source (the file relative to the save and the NBT path in it, `spyglass_world::Save::sources`) and what it decides, a fixed text. A value the file doesn't have is marked absent, with the default the game uses.

| Setting                     | `Current`                                                                | `Legacy`                                                      | `Before116`                                  | Decides                                    |
| --------------------------- | ------------------------------------------------------------------------ | ------------------------------------------------------------- | -------------------------------------------- | ------------------------------------------ |
| version                     | `level.dat` `Data.Version.Name`                                          | as `Current`                                                  | as `Current`                                 | what to extract when no bundle matches     |
| data version                | `level.dat` `Data.DataVersion`                                           | as `Current`                                                  | as `Current`                                 | which bundle answers                       |
| seed                        | `data/minecraft/world_gen_settings.dat` `data.seed`                      | `level.dat` `Data.WorldGenSettings.seed`                      | `level.dat` `Data.RandomSeed`                | every prediction                           |
| structures                  | `data/minecraft/world_gen_settings.dat` `data.generate_structures`       | `level.dat` `Data.WorldGenSettings.generate_features`         | `level.dat` `Data.MapFeatures`               | whether structure answers exist            |
| generator, per dimension    | `data/minecraft/world_gen_settings.dat` `data.dimensions.<id>.generator` | `level.dat` `Data.WorldGenSettings.dimensions.<id>.generator` | `level.dat` `Data.generatorName` (Overworld) | whether spyglass answers in that dimension |
| world border, per dimension | `dimensions/<namespace>/<path>/data/minecraft/world_border.dat` `data`   | `level.dat` `Data.BorderSize`                                 | as `Legacy`                                  | where portals can be found and built       |
| spawn                       | `level.dat` `Data.spawn`                                                 | `level.dat` `Data.SpawnX`                                     | as `Legacy`                                  | `--from spawn`, and the spawn origin       |
| single player               | `players/data/<uuid>.dat` `Pos`                                          | `level.dat` `Data.Player.Pos`                                 | as `Legacy`                                  | `--from player`                            |
| modded                      | `level.dat` `Data.WasModded`                                             | as `Current`                                                  | as `Current`                                 | nothing, but mods can change generation    |

Where the game reads either of two names (`generate_structures` or `generate_features`), the source is the one found, or the first if neither is. A generator is shown with the bundle's preset it matches, if any. A border is shown with its centre and size, whether it's moving, and whether its file was unreadable.

### Packs

Each enabled pack in load order, `vanilla` first, with what spyglass did with it: applied from `datapacks/`, a built-in feature pack applied from the bundle, the bundle itself (`vanilla`), or not applied, with the `data_pack` warning that says why. An applied pack lists what it changes that spyglass reads ([data-packs.md](data-packs.md#what-a-pack-changes)).

### Dimensions

One per dimension of the Default preset: its region folder, whether spyglass answers in it (and if not, the warnings that block it), and, after `Scan::Chunks`, its holdings:

- chunks: listed in region files, full (`Status` `full`), proto (any other status), unreadable, and unreadable region files;
- chunks by data version, newest first, each named by a bundle with that data version where one is in the bundles folder, or else by number; chunks with none are from before 1.9;
- the ticks players spent there (`InhabitedTime`, summed) and the bounds of the listed chunks;
- structure starts by structure: in chunks saved at data version 2970 or later, each key of `structures.starts` whose start has an `id` that isn't `INVALID` ([structure starts](#structure-starts-in-saves)). These are what answers take from the save instead of the seed. A start whose key or `id` the bundle with packs has no structure for is counted as dropped, as the game drops it. Starts in older chunks (`structures.starts` before 2970, `Level.Structures.Starts` before 1.18) are counted apart, by their keys: the game data-fixes them and spyglass predicts them;
- sealed loot containers by block entity `id`: entries of a chunk's `block_entities` with a `LootTable`, so not yet filled ([containers](#containers-in-saves)). Older chunks' `Level.TileEntities` and chest minecarts (in `entities/`) aren't counted.

The counts are made in the chunk scan's one pass, read without building any tree (`nbt::walk`).

**Opened containers** aren't counted for the whole save: a save can't tell an opened structure chest from one a player placed, and finding them would mean predicting every container in every generated chunk. Answers count them as they're made instead: `SaveChunks::usage()` tallies the chunks read, the starts answered from the save, the chunks whose save says no start is there, and the containers answered unopened, opened or gone. The viewer shows its session's tally; `spyglass world info` makes no queries, so shows none.

### Warnings and their subjects

| Warning                 | Subject (`SaveSubject`)                                                                                                                                                       |
| ----------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `no_bundle`             | the data version setting                                                                                                                                                      |
| `unsupported_generator` | that dimension's generator setting                                                                                                                                            |
| `structures_disabled`   | the structures setting                                                                                                                                                        |
| `data_pack`             | that pack                                                                                                                                                                     |
| `modded`                | the modded setting                                                                                                                                                            |
| `older_chunks`          | that dimension's holdings                                                                                                                                                     |
| `unreadable_regions`    | that dimension's holdings                                                                                                                                                     |
| `moving_border`         | that dimension's border setting                                                                                                                                               |
| `pack_error`            | the pack whose file the error names, by its `data/...` path or by an id one of the packs has a file for (most loaders' errors name the resource by id), or else the dimension |

`SaveWarning`, `PackConcern`, `SaveSubject` and `Setting` are `#[non_exhaustive]`, since spyglass may learn of more ([ADR 0077](../adr/0077-open-ended-public-enums-are-non-exhaustive.md)); `SaveSubject`'s `kind`, `setting`, `pack` and `dimension` read one without a `match`. `pack_error` comes from opening each answerable dimension with the packs, so only the report raises it; `SaveReport::warnings` doesn't have it, and `pack_errors()` gives the same errors without the report. The other warnings are the same in both.

## Region files

`region::chunk_versions(dir)` reads every `r.<x>.<z>.mca` (`RegionFile`) in a dimension's region folder, in parallel:

- The first 4 KiB is 1,024 big-endian locations, chunk `(x & 31) + (z & 31) · 32`: a 3-byte sector offset and a 1-byte sector count, sectors being 4 KiB; zero means no chunk.
- At the offset, a `u32` length (counting the next byte), a compression byte (1 gzip, 2 zlib, 3 none, 4 LZ4), then the chunk's NBT. With 128 added to the compression byte, the data is in `c.<x>.<z>.mcc` beside the region file, with the chunk's absolute coordinates.
- Each chunk's root `DataVersion` is counted. Chunks with none are from before 1.9, older than any versioned chunk. LZ4 (an opt-in server setting), unknown compression, missing `.mcc` files and malformed chunks are counted as unreadable, not errors. A region file that can't be opened is counted too, as an unreadable file, and the scan goes on.
- A chunk is read as `RegionFile.getChunkDataInputStream` reads it: its sectors, or what's left of the file, at most 255 × 4 KiB. It's unreadable if that's under 5 bytes, if its length is 0 (even for an external chunk), or, inside the region file, if the length is negative or the data (`length − 1` bytes after the compression byte) runs past what was read. An external chunk's length is otherwise ignored.
- A length longer than the location's sectors is malformed, as `RegionFile` treats it.

- Each chunk's `Status` (a string, e.g. `minecraft:full`; before 1.18, bare `full` under `Level`) and `InhabitedTime` (a long: ticks players spent nearby; under `Level` before 1.18) are kept with its data version, with its structure starts and sealed containers as the [report](#dimensions) counts them, in `region::index`'s `ChunkIndex`: per region file, each listed chunk by header index, readable or not. `region::read_chunk` reads one chunk again, by position, from the region folder, under the same rules; it needs no index, so queries that read a few chunks don't scan the save.

The scan reads every chunk in one pass, without building its tree (`nbt::walk`). The largest save checked (344,000 chunks, 4.6 GB) takes about 6 to 8 seconds, the same with the index as with counts alone (2026-11-11). Counting starts and sealed containers in the same pass took it from 6.3 to 7.1 s to 7.1 to 7.9 s in `spyglass world info --json`, three runs each, twice (2026-11-16): under a quarter more, so the counts are part of `Scan::Chunks` rather than a level of their own.

## Structure starts in saves

`/locate`, and every structure check (`StructureCheck.checkStart`), asks storage before predicting (`tryLoadFromStorage`, 26.3 bytecode, 2026-11-11):

- A chunk the dimension's region files don't have is predicted from the seed, as spyglass predicts it.
- A chunk on disk is read for `DataVersion` and `structures.starts` (`Level.Structures.Starts` before 1.18), then data-fixed to the current version. Until 1.18.2 (data version 2970, `StructuresBecomeConfiguredFix`), starts were keyed by structure feature (`minecraft:village`, `minecraft:ocean_ruin`), which the fix renames by the chunk's biomes; a missing `DataVersion` reads as -1, so every fix applies. No status is read, and no data version is too old: a chunk any version saved answers. A chunk that can't be read or fixed is loaded instead (`CHUNK_LOAD_NEEDED`), so the answer waits for the chunk.
- If the chunk has no `starts` compound, it's predicted. Otherwise each child is a start keyed by structure id, `{id, ChunkX, ChunkZ, references, Children}` (`SerializableChunkData.unpackStructureStart`, `StructureStart.loadStaticStart`, 26.3 bytecode): a start whose `id` is `INVALID` isn't one; a start is dropped unless both its key and its `id` name a structure the registry has (a removed data pack's doesn't), and a missing `id` reads as `""`, which names none; a missing `ChunkX`, `ChunkZ` or `references` reads as 0. The structure is present if the chunk has its start, and absent if not, even if the seed would place it. An empty `starts` means none.
- With `skipKnownStructures` (exploration maps), a start counts only while its `references` is 0.

So for chunks the save has, the save's starts are the game's answer, including chunks an older version or a data pack generated. spyglass answers from them likewise, in a session given the save (`Session::with_save`, [ADR 0071](../adr/0071-answers-from-a-save.md)): a chunk with current-format starts decides, and every start from it is certain and says so (`StructureStart::from_save`), even in a chunk a tied or platform-dependent stronghold ring position may be in, since the save records where the game put it. Chunks saved before 1.18.2 (or with no data version), whose starts need the data fixes spyglass doesn't port, and unreadable chunks are predicted. A start's pieces (`Children`, each with its `id` and its box `BB`, six ints) are what was placed. `ChunkMap.save` skips a chunk still at the `empty` status unless it has a valid start, and never saves a proto chunk over a full one, so a saved chunk has had its starts decided.

## Containers in saves

A container a structure or feature fills from a loot table (`RandomizableContainer`) stores the table as `LootTable` in its block entity, and `LootTableSeed` unless the seed is 0, which draws a seed when it's filled (26.3 bytecode, 2026-11-11). `unpackLootTable` clears the table and fills the container the first time anything reaches its contents: a player opening it, a hopper, breaking it. So a saved container with `LootTable` hasn't been filled yet, and its predicted contents stand if `LootTableSeed` is the predicted seed. One without it has been filled, and its `Items` are what it holds now. A container predicted certain can still be gone from a save no one has touched: a feature of a neighbouring chunk decorated later, such as an amethyst geode or a dungeon, overwrites it (seen in two 26.3 saves, 2026-11-11). Block entities are in a chunk's `block_entities` list, with `id` and `x`, `y`, `z`. A predicted container is the block entity at its position whose `id` is its block's block-entity type (the [block report](data-sources.md#block-report)'s `block_entity`); one of another type there, a furnace a player put where the chest was, means the container is gone. A player's chest in the same place can't be told from the structure's. With a bundle whose report predates `block_entity`, any block entity at the position is taken. Chunks saved before data version 3438 (by 1.19.4 with its 1.20 features on) name suspicious sand's block entity `minecraft:suspicious_sand`, which `DataFixers`' "Rename Suspicious Sand to Brushable Block" (`BlockEntityRenameFix`) renames to `minecraft:brushable_block`, and so does spyglass; it's the only block entity renamed since 1.18 (26.3's `DataFixers`, 2026-11-13). Chest minecarts are entities, in the dimension's `entities/` region files since 1.17, with the same fields. A proto chunk (generated partway) keeps its entities in its own `entities` list until it's full, when they move to `entities/` (`SerializableChunkData`, 26.3 bytecode), so a minecart in one is read from the chunk.

## Chunk biomes

spyglass doesn't read chunk biomes. A chunk's `sections` list (`SerializableChunkData`) has one compound per section, keyed by `Y`, and up to 26.3 its `biomes` is a paletted container of quart biomes, 4 × 4 × 4 per section: the biome source's answers, as spyglass's quart lookup gives them.

_Versions:_ 26.4-snapshot-5 stores a biome per block. A section's `biomes` is a 16 × 16 × 16 container (`PalettedContainerFactory.biomeContainerCodec`), and a section may also have `noise_biomes`, the 4 × 4 × 4 quart container (`noiseBiomeContainerCodec`), written only while the chunk still carries its quart biomes (`NoiseBiomeChunk`):

- Quart biomes are filled at the new `NOISE_BIOMES` chunk status, as 26.3 fills a chunk's biomes. The game drops them at `FEATURES`, so only proto chunks saved before that have `noise_biomes`.
- Block biomes are filled at the new `BIOMES` status by `CachedChunkBiomeResolver`: `BiomeManager`'s zoom (the fiddled nearest of eight quarts, seeded with `BiomeManager.obfuscateSeed(seed)`) over the quart biomes of the chunk and its neighbours. So each stored block biome is 26.3's `BiomeManager.getBiome(x, y, z)` for the same world. The zoom's arithmetic matches `BiomeManager`'s; that a neighbour's quart biomes are always still there when it runs is inferred, not confirmed in the bytecode.

For reading 26.4 saves, the data version tells the shapes apart: `biomes` holds quarts before 26.4 and blocks from it. A finished chunk's block biomes can't be turned back into quarts exactly, so comparing them with spyglass's quart answers needs `BiomeManager`'s zoom applied to those answers; where `noise_biomes` is present, it compares directly.

## Points of interest

Each dimension keeps its points of interest beside its chunks, in `poi/` (beside `region/`, under the layout's rules), in the [region format](#region-files). `spyglass_world::poi::read(dir, sections)` reads the Nether portal records the portal search uses ([portals.md](portals.md#points-of-interest)), as `SectionStorage.PackedChunk.parse` and the codecs of `PoiSection.Packed` and `PoiRecord.Packed` read them (26.3 bytecode, 2026-11-15):

- **A chunk** is a compound with `DataVersion` and `Sections`. For each section y in the dimension's range (the Overworld's −4 to 19), the game looks up `Sections` by the decimal text of y. Other keys (`20` in the Overworld, `+3`, `03`) are never read.
- **A section** is a compound with `Valid` and `Records`. `Valid` is lenient: a missing value or one that isn't a number is false, and a number is true when its `byteValue()` isn't 0. `Records` is required: without a list there, the section is read as missing.
- **A record** is `{pos, type, free_tickets}`. `pos` is three numbers, an int array or a list or array of numbers by each one's `intValue()` (longs narrowed, floats truncated). `type` is a resource id, `minecraft:` when it has no namespace. `free_tickets` is optional, but present it must be a number. A record the codec rejects is skipped, and the rest of its section is kept. Only `minecraft:nether_portal` records are kept, and whether another type is registered isn't checked.
- **Data fixes:** the game upgrades a chunk saved by an older version before reading it, taking a chunk without `DataVersion` as 1945. `ForcePoiRebuild` (schema 2218, 1.16) has every section of an older chunk rebuilt, so spyglass reads none of its sections as valid. The other fixes (`ReorganizePoi` at 1946, renaming `bee_hive` at 2209, removing unpopulated villager types at 3097) don't touch portal records.

A chunk that can't be read, or whose root isn't a compound, counts as unreadable, as does a region file that can't be read or whose name doesn't parse: their sections are missing to the game too, and it rebuilds them from the chunk's blocks.

## World borders

`Save::world_border(dimension)` gives a dimension's border as the game reads it on opening the save (26.3 bytecode, 2026-11-15):

- **Current layout:** the dimension's `minecraft:world_border` saved data, `dimensions/<namespace>/<path>/data/minecraft/world_border.dat`, whose `data` is `WorldBorder.Settings`: `center_x` and `center_z` (within ±29,999,984), `size`, `lerp_time`, `lerp_target`, `damage_per_block`, `safe_zone`, `warning_blocks` and `warning_time`. Every field is required and a number. A file that doesn't decode is replaced by the default, and spyglass reads it as unreadable, with the default. A dimension without the file has the default border: centre 0, 0, size 59,999,968.
- **Older layouts:** `level.dat`'s `Data.BorderCenterX`, `BorderCenterZ`, `BorderSize`, `BorderSizeLerpTime` and `BorderSizeLerpTarget`, each defaulted when missing (0, 0, 59,999,968, 0, 0). `LegacyWorldBorderFix` turns them into the settings above, and `LevelDatToSavedDataFileFix` writes them to the Overworld, the Nether and the End, the Nether's centre divided by 8. Other dimensions get none.
- **Moving borders:** a `lerp_time` above 0 is a border still moving to `lerp_target`. Its bounds then depend on time; spyglass answers with the size it ends at, and says so.

## Limits

Saves are untrusted input, so reads are capped well above anything the game writes:

- NBT files (`level.dat`, player and world-gen settings files): 64 MiB, both on disk and after decompression, and 64 MiB of memory for the tree they're read into. More is an error.
- Region files: no limit, since each chunk is read on its own, at most its 255 sectors.
- One chunk, or one `.mcc` file: 32 MiB, both stored and after decompression. Larger chunks are unreadable. That holds for `poi/` chunks too.
- A `poi/` chunk's tree: 256 MiB of memory. More makes the chunk unreadable.
- `world_border.dat`: as other NBT files. One past the limit is unreadable, and the border the default.
- A chunk read whole, for its starts and block entities (`chunk::read`, `chunk::read_entities`): 256 MiB of memory for its tree. More is an error, and the chunk is answered as one the save doesn't hold.
- Data pack folders are walked without following symbolic links. So `spyglass world info` scans by default, and `locate --world` doesn't.

## Finding saves

`launcher_saves()` lists the saves (folders with `level.dat`) in:

- the official launcher's `saves/`: `%APPDATA%\.minecraft` on Windows, `~/Library/Application Support/minecraft` on macOS, `~/.minecraft` on Linux;
- each Modrinth App profile's `saves/`: `ModrinthApp/profiles/<profile>/` in the platform's data folder (`%APPDATA%`, `~/Library/Application Support`, `~/.local/share`).

`find_save(text)` takes a path to a save, or a save folder's name if exactly one launcher save has it.

## Testing

- **Unit tests** build saves of each layout, the single player, errors, data packs (worldgen and other files, missing packs, names outside `datapacks/`, symlink cycles on Unix) and region files (every compression, `.mcc` chunks, unversioned and corrupt chunks, short files, lengths past their sectors and at the 4-byte boundary, zero-length external chunks, offsets past the end, decompression bombs, region files that can't be read) in the test, with `nbt::write` (ADR 0008).
- **Fixture tests per dimension**, on a fixture bundle with an Overworld and a Nether: a save whose Nether generator isn't the Default one, or is missing, blocks the Nether only, in `open_world` and `open_game` alike.
- **The CLI tests** run `world info` and `locate --world` on saves built in the test: Default, superflat, no bundle, packs, and `--from player`.
- **Ignored tests on the machine's own saves**: in every answerable save no older version touched, the structure starts saved within 48 chunks of the spawn origin are the certain predicted ones, and every certain prediction in a chunk the save holds is saved (200 starts in four 26.3 saves, 2026-11-11). Every save in the launchers reads (191 on the development machine, all three layouts, 1.9 to 26.3), and every answerable save that no older version generated part of has its Overworld spawn in the 11 × 11 chunks around spyglass's spawn origin. Every Default 26.3 save there stores all three dimensions' generators, each the Default preset's; a superflat one is blocked in the Overworld only. Every save there has a report (`details()`, 193 saves), and in every answerable 26.3 save no older version touched, the starts the chunk scan keeps are those the whole-chunk reader gives, chunk by chunk, within 48 chunks of the spawn origin (29,720 chunks and 228 starts in five saves, 2026-11-16). `world info --json` and `details()` agree field for field on a 26.3 save with chunks from an older version and on a copy of a 26.3 save with Terralith enabled.
- **In game**: `cargo xtask oracle locate --world <save>` runs the dedicated server on a copy of the save ([oracle.md](oracle.md#locate-answers)), and the `/locate` comparison opens the save through `inspect_save`. For the two Seed Check worlds, all 200 cases agree.
