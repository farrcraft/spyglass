# World saves

Status: stable. Implemented in `spyglass-world` (reading) and `spyglass` (assessment, `inspect_save`), for saves of any version; answers need a bundle with the save's data version, and the Default preset.

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
- **Preset**: no preset name is stored. The overworld's generator is compared with each `worldgen/world_preset/*.json` in the bundle, after normalizing both: ids without a namespace get `minecraft:`, booleans become 0 or 1, numbers compare by value. Only `minecraft:normal` (Default) is supported. A generator matching another preset is reported with that preset's id; one matching none (a customized superflat, for example) is described by its `type` and noise `settings`.

## Warnings

`inspect_save(dir, bundles, scan)` returns the save, its bundle, and `SaveWarning`s, most serious first. The first two block answers (`SaveReport::open_world` refuses); the rest are shown with every answer.

| Warning                 | Raised when                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| ----------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `no_bundle`             | No bundle has the save's data version, or the save is from before 1.9.                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| `unsupported_generator` | The overworld's generator isn't the Default preset's.                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| `structures_disabled`   | `generate_structures` (or `generate_features`, `MapFeatures`) is false: structure answers don't exist in the world.                                                                                                                                                                                                                                                                                                                                                                                             |
| `data_pack`             | An enabled pack other than `vanilla` may change worldgen: a `file/` pack with any file under `data/<ns>/worldgen/` or `data/<ns>/tags/worldgen/` (at its root or in an overlay folder; zips and folders both read); a `file/` pack that's missing, or whose name isn't a single file name (such as `file/../x`; it isn't read, and the game couldn't find it either); or any other pack (a built-in feature pack or a mod's), whose files aren't in the save. `file/` packs without worldgen files are ignored. |
| `modded`                | `Data.WasModded` is set: mods can change generation without data packs.                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| `older_chunks`          | With `Scan::Chunks`: overworld chunks whose own `DataVersion` is older than the save's (or that have none). Their terrain and structures follow the version that generated them.                                                                                                                                                                                                                                                                                                                                |

A vanilla dedicated server opening a save that enables a pack it doesn't have logs `Missing data pack <name>` and carries on without it (26.3, checked 2026-10-25).

## Region files

`region::chunk_versions(dir)` reads every `r.<x>.<z>.mca` (`RegionFile`) in a dimension's region folder, in parallel:

- The first 4 KiB is 1,024 big-endian locations, chunk `(x & 31) + (z & 31) · 32`: a 3-byte sector offset and a 1-byte sector count, sectors being 4 KiB; zero means no chunk.
- At the offset, a `u32` length (counting the next byte), a compression byte (1 gzip, 2 zlib, 3 none, 4 LZ4), then the chunk's NBT. With 128 added to the compression byte, the data is in `c.<x>.<z>.mcc` beside the region file, with the chunk's absolute coordinates.
- Each chunk's root `DataVersion` is counted. Chunks with none are from before 1.9, older than any versioned chunk. LZ4 (an opt-in server setting), unknown compression, missing `.mcc` files and malformed chunks are counted as unreadable, not errors.
- A length longer than the location's sectors is malformed, as `RegionFile` treats it.

The scan reads every chunk, without building its tree (`nbt::read_root_int`). The largest save checked (344,000 chunks, 4.7 GB) takes about 8 seconds.

## Limits

Saves are untrusted input, so reads are capped well above anything the game writes:

- NBT files (`level.dat`, player and world-gen settings files): 64 MiB, both on disk and after decompression. Larger is an error.
- Region files: the format's own maximum, 2 + 1,024 × 255 sectors of 4 KiB. Every chunk of a larger file is unreadable.
- One chunk, or one `.mcc` file: 32 MiB, both stored and after decompression. Larger chunks are unreadable.
- Data pack folders are walked without following symbolic links. So `spyglass world info` scans by default, and `locate --world` doesn't.

## Finding saves

`launcher_saves()` lists the saves (folders with `level.dat`) in:

- the official launcher's `saves/`: `%APPDATA%\.minecraft` on Windows, `~/Library/Application Support/minecraft` on macOS, `~/.minecraft` on Linux;
- each Modrinth App profile's `saves/`: `ModrinthApp/profiles/<profile>/` in the platform's data folder (`%APPDATA%`, `~/Library/Application Support`, `~/.local/share`).

`find_save(text)` takes a path to a save, or a save folder's name if exactly one launcher save has it.

## Testing

- **Unit tests** build saves of each layout, the single player, errors, data packs (worldgen and other files, missing packs, names outside `datapacks/`, symlink cycles on Unix) and region files (every compression, `.mcc` chunks, unversioned and corrupt chunks, short files, lengths past their sectors, offsets past the end, decompression bombs) in the test, with `nbt::write` (ADR 0008).
- **The CLI tests** run `world info` and `locate --world` on saves built in the test: Default, superflat, no bundle, packs, and `--from player`.
- **Ignored tests on the machine's own saves**: every save in the launchers reads (191 on the development machine, all three layouts, 1.9 to 26.3), and every answerable save that no older version generated part of has its spawn in the 11 × 11 chunks around spyglass's spawn origin.
- **In game**: `cargo xtask oracle locate --world <save>` runs the dedicated server on a copy of the save ([oracle.md](oracle.md#locate-answers)), and the `/locate` comparison opens the save through `inspect_save`. For the two Seed Check worlds, all 200 cases agree.
