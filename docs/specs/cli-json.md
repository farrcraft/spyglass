# CLI JSON output

Status: stable, format 1. Covers `spyglass locate`, `spyglass render`, `spyglass search` and `spyglass world` with `--json`.

Scripts depend on this format, so it's versioned. Within a format version, fields are only ever added. Removing or changing the meaning of a field bumps `format`. The CLI writes the JSON from the library's results ([ADR 0014](../adr/0014-public-api-world-and-session.md)); the library types themselves aren't the format.

## Conventions

- One JSON object on stdout, pretty-printed. Key order isn't significant.
- Errors go to stderr as text, with a non-zero exit code and nothing on stdout.
- Coordinates are blocks unless the field says `chunk`. A chunk is `[x, z]`.
- Ids are full game ids, e.g. `minecraft:village_plains`.
- Paths are strings. Characters that aren't valid Unicode are replaced with U+FFFD.
- `seed` is the 64-bit seed after text seeds are hashed ([locate.md](locate.md#text-seeds)).

Every `locate` and `render` object starts with:

| Field       | Type   | Meaning                                                                         |
| ----------- | ------ | ------------------------------------------------------------------------------- |
| `format`    | int    | This format's version: `1`.                                                     |
| `command`   | string | `locate structure`, `locate biome`, `locate slime`, `locate spawn` or `render`. |
| `version`   | string | The game version of the bundle used, e.g. `26.3`.                               |
| `dimension` | string | `minecraft:overworld`.                                                          |
| `seed`      | int    | The world seed.                                                                 |

With `--world`, two more:

| Field      | Type   | Meaning                                                                                             |
| ---------- | ------ | --------------------------------------------------------------------------------------------------- |
| `world`    | object | `name` (the world list's name) and `dir` (the save's folder).                                       |
| `warnings` | array  | What makes answers inexact, as [below](#warnings). Never one that blocks answers: those are errors. |

## Warnings

Each warning is `{kind, message}`: `message` is for people, `kind` one of `no_bundle`, `unsupported_generator`, `structures_disabled`, `data_pack`, `modded` or `older_chunks` ([world-saves.md](world-saves.md#warnings)). New kinds may be added within format 1.

## `locate structure`

| Field            | Type        | Meaning                                                                                                                                           |
| ---------------- | ----------- | ------------------------------------------------------------------------------------------------------------------------------------------------- |
| `target`         | string      | The argument as given: a structure id or `#tag`.                                                                                                  |
| `from`           | `[x, y, z]` | Where the search ran from.                                                                                                                        |
| `radius`         | int         | Rings of placement cells searched (100 is `/locate`'s).                                                                                           |
| `hits`           | array       | Where the game's answer may be, in its search order ([locate.md](locate.md#structures)).                                                          |
| `complete`       | bool        | Whether the search ended where the game's does. If true, the game's answer is one of `hits` (or nothing, if every hit is a candidate it rejects). |
| `certain_answer` | int or null | Index into `hits` of the game's answer if every candidate fails.                                                                                  |

Each hit:

| Field         | Type     | Meaning                                                                        |
| ------------- | -------- | ------------------------------------------------------------------------------ |
| `position`    | `{x, z}` | What `/locate` reports (it prints `~` for y).                                  |
| `chunk`       | `[x, z]` | The structure's start chunk.                                                   |
| `set`         | string   | The structure set, e.g. `minecraft:villages`.                                  |
| `structures`  | array    | The searched structures that may start here, in the order the game tries them. |
| `certain`     | bool     | Whether the game certainly starts one of them here. False: a candidate.        |
| `blocks_away` | int      | The distance `/locate` prints: horizontal, rounded down.                       |

## `locate biome`

| Field    | Type        | Meaning                                                                                             |
| -------- | ----------- | --------------------------------------------------------------------------------------------------- |
| `target` | string      | The biome id as given.                                                                              |
| `from`   | `[x, y, z]` | Where the search ran from. The search is 3D.                                                        |
| `radius` | int         | Blocks searched (6400 is `/locate`'s).                                                              |
| `hits`   | array       | Where the game's answer may be, in search order, ending at the first certain one. Empty: not found. |

Each hit: `position` (`{x, y, z}`), `biome` (the biome there), `tied` (every biome the game may give there if the cell is tied, else empty), `certain` (false for a tied cell whose biomes disagree) and `blocks_away` (3D, rounded down).

## `locate slime`

`from` (`[x, y, z]`), `radius` (in chunks) and `chunks`: every slime chunk in the square of `radius` chunks around `from`'s chunk, nearest first.

## `locate spawn`

| Field                | Type             | Meaning                                                                                                             |
| -------------------- | ---------------- | ------------------------------------------------------------------------------------------------------------------- |
| `chunk`              | `[x, z]`         | The chunk the game's spawn search starts from ([spawn.md](spawn.md)). The spawn is in the 11 × 11 chunks around it. |
| `position`           | `{x, z}` or null | The climate search's best block; null if the dimension has no spawn targets.                                        |
| `platform_dependent` | bool             | Whether the game's answer may differ by platform.                                                                   |

## `render`

The image is written to the file given; the JSON describes it ([render.md](render.md)).

| Field          | Type              | Meaning                                                                                                                         |
| -------------- | ----------------- | ------------------------------------------------------------------------------------------------------------------------------- |
| `output`       | string            | The PNG written.                                                                                                                |
| `area`         | object            | `min` (`[x, z]`, the top-left pixel's corner block), `size` (`[width, height]` in pixels), `scale` (blocks per pixel) and `y`.  |
| `layers`       | object            | What was drawn: `biomes`, `ties`, `slime_chunks` and `spawn` (bools), `highlight` and `structures` (the ids and tags as given). |
| `structures`   | array             | The structure markers drawn, in drawing order. Each is a `locate structure` hit without `blocks_away`.                          |
| `slime_chunks` | array of `[x, z]` | The slime chunks shaded, by z, then x. Empty unless asked for.                                                                  |
| `spawn`        | object or null    | The spawn marker, as `locate spawn`'s fields; null if not asked for or not in view.                                             |

## `search`

The best seeds for a query ([search.md](search.md)). A search is over many seeds, so its object has no `seed`, `world` or `warnings`: it starts with `format`, `command` (`search`), `version` and `dimension`, then:

| Field             | Type        | Meaning                                                                                                  |
| ----------------- | ----------- | -------------------------------------------------------------------------------------------------------- |
| `query`           | string      | The query's canonical text, one constraint per line.                                                     |
| `source`          | object      | `kind`: `all`, `range` (with `first` and `last`, both included) or `list` (with `count`).                |
| `from`            | int         | The first index searched.                                                                                |
| `certain`         | bool        | Whether only seeds whose required constraints are certainly met were reported, ranked by the worst case. |
| `top`             | int         | How many seeds were kept.                                                                                |
| `next`            | int or null | The first index not searched; `--from` it to continue. Null when the source is finished.                 |
| `evaluated`       | int         | Indices searched: `next − from`, or to the end of the source.                                            |
| `prefiltered`     | int         | Seeds rejected by structure placement alone.                                                             |
| `checked`         | int         | Seeds checked further.                                                                                   |
| `reported`        | int         | Seeds that met the required constraints, whether or not they made the list.                              |
| `elapsed_seconds` | number      | How long the search took. The only field that differs between runs of the same search.                   |
| `hits`            | array       | The best seeds, best first: by `score`, then by `index`.                                                 |

Each hit has `index`, `seed`, `score`, `verdict` (`met` or `possibly`) and `constraints`, one per line of the query, each with:

| Field        | Type   | Meaning                                                                                  |
| ------------ | ------ | ---------------------------------------------------------------------------------------- |
| `constraint` | string | The constraint's canonical line.                                                         |
| `required`   | bool   | Whether it's `require`.                                                                  |
| `outcome`    | string | `met`, `possibly` or `failed`.                                                           |
| `best`       | int    | Its satisfaction in the best case, 0 to 1000.                                            |
| `worst`      | int    | Its satisfaction in the worst case.                                                      |
| `features`   | array  | What it matched. Each has a `kind` and that kind's fields, as below. Empty for `covers`. |

Feature kinds; more may be added within format 1:

- `structure`: a `locate structure` hit without `blocks_away`.
- `slime_chunk`: `chunk`.
- `biome`: `position` (`x`, `y`, `z`), `biome`, `tied` (as `locate biome`) and `certain`.
- `spawn`: `locate spawn`'s fields.

## `world info`

`format`, `command` (`world info`), and the save:

| Field                 | Type           | Meaning                                                                                                                             |
| --------------------- | -------------- | ----------------------------------------------------------------------------------------------------------------------------------- |
| `name`                | string         | The name in the world list.                                                                                                         |
| `dir`                 | string         | The save's folder.                                                                                                                  |
| `data_version`        | int or null    | The data version it was last saved in; null before 1.9.                                                                             |
| `version_name`        | string or null | e.g. `26.3`.                                                                                                                        |
| `seed`                | int            | The world seed.                                                                                                                     |
| `generate_structures` | bool           | Whether structures generate.                                                                                                        |
| `data_packs`          | array          | Enabled data packs, in load order.                                                                                                  |
| `spawn`, `player`     | object or null | `dimension` and `position` (`[x, y, z]`, numbers); `player` only in single-player saves.                                            |
| `bundle`              | string or null | The bundle with the save's data version.                                                                                            |
| `answerable`          | bool           | Whether spyglass answers for the save: no warning blocks answers.                                                                   |
| `warnings`            | array          | [Warnings](#warnings), including the blocking kinds `no_bundle` and `unsupported_generator`. `older_chunks` only without `--quick`. |

## `world list`

`format`, `command` (`world list`) and `saves`: one object per save in the known launchers' folders, as `world info` without `format` and `command` (and without the chunk scan, so no `older_chunks`), plus `launcher` (e.g. `Modrinth (Fabric 26.3)`). A save that can't be read is `{launcher, dir, error}`.
