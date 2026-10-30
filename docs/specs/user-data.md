# User data

Status: stable. Implemented in `spyglass-viewer`, checked by its tests. Covers the viewer's user data file (markers, done places, recent and saved worlds) and the view text that opens a view in another viewer. Where the file lives, and why the viewer owns it, is [ADR 0048](../adr/0048-the-viewer-keeps-user-data-in-a-file-of-its-own.md).

These are spyglass's own formats, not the game's. Scripts may read and write the file by this spec. A field the spec doesn't name, at the top level or in a world, marker or done place, is kept as it is, so a script can add its own.

## The file

`spyglass-data.json`, UTF-8 JSON, in the viewer's settings folder:

- **Windows:** `%APPDATA%\spyglass\data\`.
- **Linux:** `$XDG_DATA_HOME/spyglass/`, or `~/.local/share/spyglass/`.
- **macOS:** `~/Library/Application Support/spyglass/`.
- **When `SPYGLASS_VIEWER_SETTINGS` names a settings file:** that file's folder.

### Top level

An object:

| Field    | Type    | Meaning                                                            |
| -------- | ------- | ------------------------------------------------------------------ |
| `format` | integer | The format's version. This spec is format 1. Required.             |
| `worlds` | array   | The [worlds](#worlds), in no particular order. Missing means none. |

### Worlds

A world is one seed, or one save, and what the user keeps for it. Each world is an object:

| Field       | Type           | Meaning                                                                                                                     |
| ----------- | -------------- | --------------------------------------------------------------------------------------------------------------------------- |
| `seed`      | string         | The seed, as a decimal 64-bit signed integer. **Required.** A string, because not every JSON reader holds an `i64` exactly. |
| `save`      | string or null | The save's level name (`LevelName` in `level.dat`) if the world came from a save. Missing or null for a bare seed.          |
| `save_dir`  | string or null | The folder the save was last opened from. A hint for reopening it, not part of the key.                                     |
| `name`      | string or null | The user's name for the world. A world with a name is **saved**; one without is only **recent**.                            |
| `modified`  | integer        | When `name` or `save_dir` last changed, in [milliseconds](#times).                                                          |
| `opened`    | integer        | When the world was last shown, in milliseconds.                                                                             |
| `version`   | string or null | The game version it was last shown in, as a bundle names it (`26.3`).                                                       |
| `dimension` | string         | The dimension last shown, by id (`minecraft:the_nether`). Default `minecraft:overworld`.                                    |
| `views`     | object         | The last [view](#views) of each dimension, by dimension id.                                                                 |
| `markers`   | array          | The user's [markers](#markers).                                                                                             |
| `done`      | array          | The [done places](#done-places).                                                                                            |

Missing times are 0. A world's **key** is its `seed` and `save` together. No two worlds in a file have the same key. The key holds no game version: markers are places, and a version switch keeps them. It holds no path either, since a save's path changes between machines.

### Views

| Field    | Type               | Meaning                                             |
| -------- | ------------------ | --------------------------------------------------- |
| `centre` | array of 2 numbers | The block x and z at the centre of the map. Finite. |
| `scale`  | integer            | Blocks per pixel: a power of two from 1 to 256.     |
| `y`      | integer            | The height biomes are drawn at.                     |

A view that's out of range is fixed as the viewer fixes its settings: the default scale (4), and a centre clamped to ±10⁹.

### Markers

| Field       | Type            | Meaning                                                                                                         |
| ----------- | --------------- | --------------------------------------------------------------------------------------------------------------- |
| `id`        | string          | **Required.** Unique within the world: 1 to 64 characters from `A–Z a–z 0–9 - _`. The viewer makes them random. |
| `dimension` | string          | **Required.** The dimension's id.                                                                               |
| `x`, `z`    | integer         | **Required.** The block. Within ±30,000,000.                                                                    |
| `y`         | integer or null | The block's height, if the user gave one. Within ±30,000,000.                                                   |
| `name`      | string          | May be empty, as it is by default.                                                                              |
| `colour`    | string          | `#rrggbb`, lowercase or uppercase hex. Default amber.                                                           |
| `done`      | boolean         | Whether it's marked done. Default false.                                                                        |
| `deleted`   | boolean         | A **tombstone**: the marker was deleted. Kept so that a [merge](#merging) can't bring it back. Default false.   |
| `modified`  | integer         | When it last changed, in milliseconds. Default 0.                                                               |

The viewer offers eight colours, but any `#rrggbb` is valid:

| Name   | Colour    |
| ------ | --------- |
| Amber  | `#f0b030` |
| Red    | `#e05050` |
| Green  | `#50c060` |
| Blue   | `#4c8ff0` |
| Purple | `#a070e0` |
| Pink   | `#f070b0` |
| White  | `#f0f0f0` |
| Black  | `#202020` |

### Done places

Things on the map that the user has marked done. They're keyed by place, not held by a marker, so a structure the viewer recomputes stays done:

| Field       | Type    | Meaning                                                                                                                 |
| ----------- | ------- | ----------------------------------------------------------------------------------------------------------------------- |
| `dimension` | string  | **Required.** The dimension's id.                                                                                       |
| `kind`      | string  | **Required.** `structure`, `biome` or `place`.                                                                          |
| `id`        | string  | For a structure, its structure set's id (`minecraft:villages`). For a biome, its id. Empty for a place, and by default. |
| `x`, `z`    | integer | **Required.** The block. Within ±30,000,000.                                                                            |
| `done`      | boolean | False once it's marked not done again, kept for [merging](#merging). Default true.                                      |
| `modified`  | integer | When it last changed, in milliseconds. Default 0.                                                                       |

- **A structure** is a structure start: an icon on the map, a structure answer of a locate, or a structure that a search hit or `find` match found. Its place is the start's position (`StructureStart::position`, x and z), the block locate and the map use. A start's set and position don't change with confirmation or the game version, so it stays done.
- **A biome** is a biome answer of a locate, or a biome that a search or `find` found, at the block reported.
- **A place** is a `find` match on a point target.

A place's key is its `dimension`, `kind`, `id`, `x` and `z`. No two done places in a world have the same key.

### Times

Times are integer milliseconds since the Unix epoch, UTC, from 0 to 2⁵³. They are compared only with each other, to decide which change is newer.

## Reading

A file is read leniently. Whatever can be read is kept, and each part that can't be read is reported with its path in the file (`worlds[3].markers[12]: colour "#zz0000" is not #rrggbb`):

- **No file:** no worlds, and no problem.
- **A file that exists but can't be opened** (locked, or without permission): no worlds, reported, and the file is read-only for the run.
- **A file larger than 16 MiB:** isn't read. It's reported, and kept as described under [writing](#writing).
- **A file that isn't JSON, or whose top level isn't an object:** the same.
- **A `format` that's missing or not an integer:** the same.
- **A `format` newer than 1:** read as format 1, field by field, but the file is **read-only**: the viewer never writes over it, and says so.
- **A world, marker or done place that's invalid** is skipped: a required field missing, of the wrong type, or out of range. A wrong optional field is ignored and reported, and its default used.
- **A view that isn't an object** with a `centre`, `scale` and `y` is ignored and reported. One that's out of range is fixed, as [above](#views).
- **A world with the same key as an earlier one** is merged into it, by the [merge rules](#merging).
- **A marker with the same id as an earlier one in its world, or a done place with the same key:** the one with the later `modified` is kept.
- **Limits.** Past these, entries are skipped and reported:
  - 1,000 worlds.
  - 10,000 markers and 10,000 done places per world, counting tombstones.
  - 200 characters for a name, and for an id, a version or `save`.
  - 4,096 characters for `save_dir`.

Fields the spec doesn't name are kept, at the top level and in worlds, markers and done places, and written back as they were. A view keeps only its own fields.

## Writing

- **When:** at most once a second after a change, and when the viewer closes.
- **How:** to a temporary file in the same folder, then renamed over `spyglass-data.json`. A crash leaves the old file or the new one, never half of one.
- **Backup:** the first write of each run first copies the file to `spyglass-data.json.bak`. A file that couldn't be read at all is renamed to `spyglass-data.json.bad` instead, so a new file can start without losing it.
- **Merging with other writers:** before each write, the viewer reads the file again. If it changed since the viewer last read or wrote it, it's [merged](#merging) into the viewer's data first, so markers another viewer wrote aren't lost.
- **Recent worlds:** unsaved worlds with no live markers and no done places, past the 20 most recently opened, are dropped.
- **Order:** worlds by `opened`, newest first, and markers and done places in the order they were read or made. Fields in alphabetical order, pretty-printed with two spaces.
- **A read-only file** (a newer format) is never written.

## Merging

Merging another file's worlds into the viewer's is how both import and writing work. Merging the same file twice changes nothing.

- **Worlds:** matched by key. A world only the other file has is added.
- **A world's `name` and `save_dir`:** from the one with the later `modified`.
- **Its `opened`, `version`, `dimension` and `views`:** from the one with the later `opened`.
- **Markers:** matched by id, and the later `modified` wins, tombstones included.
- **Done places:** matched by key, and the later `modified` wins.
- **Ties:** on equal times, the viewer's own copy wins.

A forgotten world is removed from the file, not left as a tombstone. So a second viewer that still has it open writes it back.

## Exporting

The viewer exports the whole file, or one world as a file with only that world. Either is a format 1 file, and importing either merges it.

## The view text

A line of text that opens a view: the seed, version, dimension, view and layers. Markers aren't in it. For example:

```text
spyglass:1;s=-5;v=26.3;d=nether;c=100,-200;sc=8;y=64;st=village_plains,#minecraft:village;hl=river;l=spawn,confirm
```

### Grammar

```text
text   = "spyglass:" format { ";" field }
format = digits
field  = key "=" value
```

- **Keys** are lowercase ASCII letters, and values have no `;` or whitespace. A text is at most 4,096 bytes.
- **Pasted text:** the text starts at the first `spyglass:` in it and ends at the first whitespace after that. So a text survives being pasted from a chat message with words around it.
- **A format newer than 1** isn't opened. The viewer says it was made by a newer spyglass.
- **Unknown keys** are ignored.
- **A key given twice,** a field without `=`, or a known key with a bad value is an error naming the key.

### Fields of format 1

| Key  | Value                                                                                                                                                                                              | When missing       |
| ---- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------ |
| `s`  | The seed: a decimal 64-bit signed integer. Required.                                                                                                                                               | An error.          |
| `v`  | The game version, as a bundle names it.                                                                                                                                                            | The version shown. |
| `d`  | The dimension: `overworld`, `nether` or `end`.                                                                                                                                                     | `overworld`.       |
| `c`  | The centre block: `x,z`, integers within ±10⁹.                                                                                                                                                     | `0,0`.             |
| `sc` | The scale: a power of two from 1 to 256.                                                                                                                                                           | 4.                 |
| `y`  | The height biomes and the underground layers are drawn at, an integer.                                                                                                                             | 64.                |
| `st` | Structures shown: ids and `#`tags, separated by commas.                                                                                                                                            | None.              |
| `hl` | Highlighted biomes: ids, separated by commas.                                                                                                                                                      | None.              |
| `l`  | The layers on, separated by commas, from `ties`, `slime`, `spawn`, `confirm`, `relief`, `surface`, `veins`, `caves` (noise caves) and `carvers`. Any not listed is off. Unknown names are ignored. | `spawn,confirm`.   |

- **Namespaces:** an id in `st` or `hl` without a namespace is `minecraft:`'s, and the viewer writes `minecraft:` ids without it. Tags keep their `#`.
- **What the viewer writes:** the fields in the order `s`, `v`, `d`, `c`, `sc`, `y`, `st`, `hl`, `l`, leaving out `st` and `hl` when they're empty.
- **The centre** is written as the block at the view's centre, rounded down, and opened as that block's centre.
- **A save's view** is shared as its seed and version, since its path means nothing on another machine.
- **A missing bundle:** if the text's version has no bundle, the viewer opens the view in the version shown, and says so.

### Compatibility

Each format number fixes what its texts mean, and every format the viewer has written stays readable. A change that only adds keys, which older viewers can ignore, keeps the number. A change to what an existing key means needs a new number. The viewer's tests keep a fixed set of format 1 texts and check that each still opens to the same view.
