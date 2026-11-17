# CLI JSON output

Status: stable, format 1. Covers `spyglass locate`, `spyglass loot`, `spyglass heights`, `spyglass blocks`, `spyglass flower`, `spyglass portal`, `spyglass render`, `spyglass search`, `spyglass find` and `spyglass world` with `--json`.

Scripts depend on this format, so it's versioned. Within a format version, fields are only ever added. Removing or changing the meaning of a field bumps `format`. The CLI writes the JSON from the library's results ([ADR 0014](../adr/0014-public-api-world-and-session.md)); the library types themselves aren't the format.

## Conventions

- One JSON object on stdout, pretty-printed. Key order isn't significant.
- Errors go to stderr as text, with a non-zero exit code and nothing on stdout.
- Coordinates are blocks unless the field says `chunk`. A chunk is `[x, z]`.
- Ids are full game ids, e.g. `minecraft:village_plains`.
- Paths are strings. Characters that aren't valid Unicode are replaced with U+FFFD.
- `seed` is the 64-bit seed after text seeds are hashed ([locate.md](locate.md#text-seeds)).

Every `locate`, `features`, `loot`, `heights`, `blocks`, `flower` and `render` object starts with:

| Field       | Type   | Meaning                                                                                                                                                                                           |
| ----------- | ------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `format`    | int    | This format's version: `1`.                                                                                                                                                                       |
| `command`   | string | `locate structure`, `locate biome`, `locate feature`, `locate slime`, `locate spawn`, `features`, `loot at`, `loot in`, `loot find`, `heights`, `blocks`, `flower`, `flower_heights` or `render`. |
| `version`   | string | The game version of the bundle used, e.g. `26.3`.                                                                                                                                                 |
| `dimension` | string | The dimension answered for: `minecraft:overworld`, `minecraft:the_nether` or `minecraft:the_end` (`--dimension`, or the player's when the position is the player's).                              |
| `seed`      | int    | The world seed.                                                                                                                                                                                   |

With `--world`, two more:

| Field      | Type   | Meaning                                                                                             |
| ---------- | ------ | --------------------------------------------------------------------------------------------------- |
| `world`    | object | `name` (the world list's name) and `dir` (the save's folder).                                       |
| `warnings` | array  | What makes answers inexact, as [below](#warnings). Never one that blocks answers: those are errors. |

Without `--world`, `warnings` is there only when the bundle's version is unchecked (`unchecked_version`, below).

## Warnings

Each warning is `{kind, message}`, and a `data_pack` warning also has `concern`: `missing`, `mod`, `unreadable`, `skipped` or `other_format` ([data-packs.md](data-packs.md#saves)). `message` is for people, `kind` one of `no_bundle`, `unsupported_generator`, `structures_disabled`, `data_pack`, `modded`, `older_chunks`, `unreadable_regions`, `moving_border` or `pack_error` ([world-saves.md](world-saves.md#warnings)), or `unchecked_version`: the bundle is for a game version this build wasn't checked against ([bundle-format.md](bundle-format.md#checked-versions)). In text mode, `unchecked_version` is a `warning:` line on stderr instead. A warning about one dimension (`unsupported_generator`, `older_chunks`, `unreadable_regions`, `moving_border` and `pack_error`) also has `dimension`, its id; one that blocks answers blocks them in that dimension only. A save's warning also has `about`, the item of [`world info`](#world-info)'s report it's beside ([world-saves.md](world-saves.md#warnings-and-their-subjects)): `{kind, setting?, dimension?, pack?}`, `kind` one of `setting` (with `setting` as in `settings`, and `dimension` for a generator or border), `pack` (with `pack`, its name), `dimension` or `holdings` (with `dimension`). New kinds may be added within format 1.

## `locate structure`

| Field            | Type        | Meaning                                                                                                                                             |
| ---------------- | ----------- | --------------------------------------------------------------------------------------------------------------------------------------------------- |
| `target`         | string      | The argument as given: a structure id or `#tag`.                                                                                                    |
| `mode`           | string      | `locate`, the game's `/locate` answer; or `nearest` with `--nearest` ([locate.md](locate.md#nearest)), which changes the meaning of the next three. |
| `from`           | `[x, y, z]` | Where the search ran from.                                                                                                                          |
| `radius`         | int         | Rings of placement cells searched (100 is `/locate`'s).                                                                                             |
| `confirmed`      | bool        | Whether terrain decided each candidate ([locate.md](locate.md#confirmed-mode)): true unless `--unconfirmed`.                                        |
| `hits`           | array       | Where the game's answer may be, in its search order ([locate.md](locate.md#structures)).                                                            |
| `complete`       | bool        | Whether the search ended where the game's does. If true, the game's answer is one of `hits` (or nothing, if every hit is a candidate it rejects).   |
| `certain_answer` | int or null | Index into `hits` of the game's answer if every candidate fails.                                                                                    |

With `mode` `nearest`, `hits` are nearest first, ending at the nearest certain start, which `certain_answer` points at. `complete` means the nearest start that generates is one of `hits`.

Each hit:

| Field         | Type           | Meaning                                                                                                                                                                                                                                                                      |
| ------------- | -------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `position`    | `{x, z}`       | What `/locate` reports (it prints `~` for y).                                                                                                                                                                                                                                |
| `chunk`       | `[x, z]`       | The structure's start chunk.                                                                                                                                                                                                                                                 |
| `set`         | string         | The structure set, e.g. `minecraft:villages`.                                                                                                                                                                                                                                |
| `structures`  | array          | The searched structures that may start here, in the order the game tries them.                                                                                                                                                                                               |
| `certain`     | bool           | Whether the game certainly starts one of them here. False: a candidate.                                                                                                                                                                                                      |
| `source`      | string         | `save` where the start is a `--world` save's own, read from a chunk it holds ([world-saves.md](world-saves.md#structure-starts-in-saves)); `seed` where it's predicted.                                                                                                      |
| `blocks_away` | int            | The distance `/locate` prints: horizontal, rounded down.                                                                                                                                                                                                                     |
| `bounds`      | object or null | With `--bounds`, for a certain hit with one structure that's a jigsaw or a mineshaft: `min` and `max` (`[x, y, z]`, corners included, as `Session::structure_layout` gives them, grown by 12 for structures that adapt the terrain) and `pieces` (how many). Otherwise null. |

## `locate biome`

| Field    | Type        | Meaning                                                                                             |
| -------- | ----------- | --------------------------------------------------------------------------------------------------- |
| `target` | string      | The biome id as given.                                                                              |
| `from`   | `[x, y, z]` | Where the search ran from. The search is 3D.                                                        |
| `radius` | int         | Blocks searched (6400 is `/locate`'s).                                                              |
| `hits`   | array       | Where the game's answer may be, in search order, ending at the first certain one. Empty: not found. |

Each hit: `position` (`{x, y, z}`), `biome` (the biome there), `tied` (every biome the game may give there if the cell is tied, else empty), `certain` (false for a tied cell whose biomes disagree) and `blocks_away` (3D, rounded down).

## `locate feature`

| Field      | Type        | Meaning                                                                                                   |
| ---------- | ----------- | --------------------------------------------------------------------------------------------------------- |
| `target`   | string      | The feature id as given.                                                                                  |
| `from`     | `[x, y, z]` | Where the search ran from. Distances are horizontal.                                                      |
| `radius`   | int         | Chunks searched around `from`'s.                                                                          |
| `complete` | bool        | Whether the hits end at a certain one.                                                                    |
| `hits`     | array       | Nearest first: every candidate nearer than the nearest certain feature, then that one. Empty: none found. |

Each hit ([locate.md](locate.md#features)): `feature` (the placed feature), `position` (`{x, y, z}`, where the game calls it), `certain`, `likely` (whether it placed on spyglass's blocks; a certain one always did), `reason` (why it's a candidate, or null), `neighbours_checked` (whether `--neighbours` asked), `neighbours` (why neighbours decorated first could change it; null where it holds whatever they do, or unchecked) and `blocks_away`. Unlikely candidates are left out unless `--all`.

## `features`

`target`, `min` and `max` (`[x, z]`, the region asked for) and `features`: each feature in the region, chunk by chunk, as `locate feature`'s hits without `blocks_away`.

## `loot`

Loot containers and their contents ([locate.md](locate.md#loot)). With `--world`, contents are as the seed generates them, not what a container holds now.

- **`loot at`:** `position` (`[x, y, z]`, the block asked about) and `containers`: every answer there, usually one, several where candidates differ in their seeds. Empty: spyglass places none there.
- **`loot in`:** `min` and `max` (`[x, z]`, the region asked for), `item` (the item asked for with `--item`, with its enchantment, or null) and `containers`: each container in the region holding it, or that might (a candidate without contents whose table could hold it), all of them without `--item`, chunk by chunk.
- **`loot find`:** `item` (as `loot in`'s), `from` (`[x, y, z]`), `radius` (chunks searched around `from`'s), `complete` (whether the hits end at one certainly holding the item) and `hits`: nearest first, every candidate nearer than the nearest certain one, then that one, each with `blocks_away` (horizontal, rounded down).

Each container:

| Field              | Type           | Meaning                                                                                                                                                                                                                                                                                                              |
| ------------------ | -------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `position`         | `{x, y, z}`    | The block, or a chest minecart's rail.                                                                                                                                                                                                                                                                               |
| `chunk`            | `[x, z]`       | The chunk whose decoration placed it; a dungeon's or ruined portal's chest may be beside it.                                                                                                                                                                                                                         |
| `holder`           | string         | The block or entity, e.g. `minecraft:chest`, `minecraft:chest_minecart`.                                                                                                                                                                                                                                             |
| `source`           | object         | What placed it: `kind` (`structure` or `feature`) and `id`, e.g. `minecraft:village_plains`, `minecraft:monster_room_deep`.                                                                                                                                                                                          |
| `table`            | string         | The loot table.                                                                                                                                                                                                                                                                                                      |
| `loot_seed`        | int or null    | The loot seed; null where it isn't known.                                                                                                                                                                                                                                                                            |
| `certain`          | bool           | Whether the container is certain on its own, table and seed included.                                                                                                                                                                                                                                                |
| `reason`           | string or null | Why it's a candidate.                                                                                                                                                                                                                                                                                                |
| `contents`         | array or null  | The filled slots in order; null without a seed.                                                                                                                                                                                                                                                                      |
| `contents_certain` | bool           | Whether the contents are certainly the game's.                                                                                                                                                                                                                                                                       |
| `contents_doubt`   | string or null | Why the contents may not be the game's, or aren't known: a tied biome, a map's target that isn't certain, an unknown seed.                                                                                                                                                                                           |
| `saved`            | object or null | With `--world`, how the save holds it, where it has placed the chunk's features ([world-saves.md](world-saves.md#containers-in-saves)): `state` `unopened` (with `as_predicted`, whether the saved table and seed are the predicted ones), `filled` (with `items`, slots as `contents`'s) or `gone`. Null otherwise. |

Each slot has `slot` (from 0), `item` and `count`, and the components its table set, each left out when unset: `damage`, `enchantments` and `stored_enchantments` (objects of levels by id), `potion`, `stew_effects` (`{effect, duration}` in ticks), `instrument`, `name` (the text component), `ominous_bottle_amplifier` and `map` (`target` as `{x, z}`, `decoration` and `zoom`).

## `locate slime`

`from` (`[x, y, z]`), `radius` (in chunks) and `chunks`: every slime chunk in the square of `radius` chunks around `from`'s chunk, nearest first.

## `locate spawn`

| Field                | Type             | Meaning                                                                                                             |
| -------------------- | ---------------- | ------------------------------------------------------------------------------------------------------------------- |
| `chunk`              | `[x, z]`         | The chunk the game's spawn search starts from ([spawn.md](spawn.md)). The spawn is in the 11 × 11 chunks around it. |
| `position`           | `{x, z}` or null | The climate search's best block; null if the dimension has no spawn targets.                                        |
| `platform_dependent` | bool             | Whether the game's answer may differ by platform.                                                                   |

## `heights`

The generator's heights, before structures, carvers, features and surface blocks ([terrain.md](terrain.md)), for one column (`--at`) or every column of a region (`--min` to `--max`, at most 65,536 columns).

| Field       | Type            | Meaning                                                                                                          |
| ----------- | --------------- | ---------------------------------------------------------------------------------------------------------------- |
| `heightmap` | string          | `WORLD_SURFACE_WG` (the default) or `OCEAN_FLOOR_WG`.                                                            |
| `min`       | `[x, z]`        | The first column; `--at`'s for one column.                                                                       |
| `max`       | `[x, z]`        | The last column, included.                                                                                       |
| `heights`   | array of arrays | One row per z from `min` to `max`, each with one height per x: one above the highest block the heightmap counts. |

## `blocks`

The blocks the game generates before structures and features ([material.md](material.md), [carvers.md](carvers.md)), for one block (`--at`), a column (`--column`) or a chunk (`--chunk`): after carvers, or before them with `--no-carvers`.

| Field     | Type | Meaning                                                           |
| --------- | ---- | ----------------------------------------------------------------- |
| `carvers` | bool | Whether carvers have cut the blocks: `false` with `--no-carvers`. |

With `--at`:

| Field      | Type        | Meaning                                                                                                                      |
| ---------- | ----------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `position` | `[x, y, z]` | The block.                                                                                                                   |
| `block`    | string      | Its state: the id, then non-default properties in brackets, e.g. `minecraft:grass_block[snowy=true]`. Air outside the level. |
| `certain`  | bool        | Whether it's the game's block whatever the lookup history; otherwise it depends on a [tied biome](material.md#ties).         |

With `--column` or `--chunk`:

| Field     | Type  | Meaning                                                                                          |
| --------- | ----- | ------------------------------------------------------------------------------------------------ |
| `min_y`   | int   | The level's lowest y, where each column starts.                                                  |
| `columns` | array | One for `--column`; 256 for `--chunk`, z outer and x inner. Each is `{x, z, blocks, uncertain}`. |

A column's `blocks` is its states from `min_y` to the level's top, as runs: `[state, count]` pairs, bottom up. `uncertain` lists the y of each block that depends on a tied biome.

## `flower at`

What bone meal grows on grass at a block, the block above the grass ([flowers.md](flowers.md)). `command` is `flower`.

| Field      | Type           | Meaning                                                                                                            |
| ---------- | -------------- | ------------------------------------------------------------------------------------------------------------------ |
| `position` | `[x, y, z]`    | The flower's block.                                                                                                |
| `certain`  | string or null | The flower that certainly grows there, whatever the biome lookup gives; null where it's down to chance.            |
| `biomes`   | array          | One for each biome the game may give at the block (each of a tie, spyglass's first): `{biome, features, flowers}`. |

A biome's `features` are its bone meal features in the game's order, repeats kept (bone meal picks one of them, each as likely): `{id, provider, by_position}`, with the provider's type and whether it picks by position. Its `flowers` are the states it can grow, most likely first: `{flower, chance}`, with `chance` as `{numerator, denominator, value}`, exact and as a float ([ADR 0075](../adr/0075-flower-answers-carry-exact-chances.md)). They're empty where the biome has no bone meal features.

## `flower heights`

One flower's cover of an area at each height ([render.md](render.md#flowers)). `command` is `flower_heights`.

| Field     | Type     | Meaning                                                                                                             |
| --------- | -------- | ------------------------------------------------------------------------------------------------------------------- |
| `flower`  | string   | The flower's block id.                                                                                              |
| `min`     | `[x, z]` | The area's corner, as given.                                                                                        |
| `max`     | `[x, z]` | Its other corner, included.                                                                                         |
| `scale`   | int      | Blocks between samples.                                                                                             |
| `heights` | array    | `{y, certain, possible, pixels}` for each height tried: the samples where the flower is certain, possible, and all. |

## `portal link`

Where a Nether portal leads ([portals.md](portals.md)). `command` is `portal link`. It needs no seed, so it starts with its own header:

| Field      | Type   | Meaning                                                                                                                                                                                                                                                                                                   |
| ---------- | ------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `format`   | int    | This format's version: `1`.                                                                                                                                                                                                                                                                               |
| `command`  | string | `portal link`.                                                                                                                                                                                                                                                                                            |
| `version`  | string | The game version of the bundle used.                                                                                                                                                                                                                                                                      |
| `world`    | object | With `--world`: `name` and `dir`, as above.                                                                                                                                                                                                                                                               |
| `warnings` | array  | With `--world`, what may make the save's portals or links differ from the game's: `{kind, message, dimension}`, `kind` one of `unreadable_poi`, `old_chunks`, `invalid_sections`, `moving_border` and `unreadable_border` ([portals.md](portals.md#a-saves-portals)). Also `unchecked_version`, as above. |

A **portal** is `{dimension, corner, axis, width, height}`: its lowest block `[x, y, z]`, `axis` `x` or `z`, and its size in blocks. A **save portal** is `{dimension, corner, bounds, blocks, rectangles}`: its lowest block, `[[x_lo, x_hi], [y_lo, y_hi], [z_lo, z_hi]]`, how many portal blocks it has, and those blocks as the portals the search takes.

**From a position** (`DIMENSION x,y,z`):

| Field      | Type   | Meaning                                                                                                                                                                   |
| ---------- | ------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `portal`   | object | The portal linked from, a portal.                                                                                                                                         |
| `to`       | string | The dimension it leads to.                                                                                                                                                |
| `exits`    | object | `{x, y, z}`, each `[lo, hi]`: the exit blocks a standing player aims at ([portals.md](portals.md#entering-a-portal)), all of them.                                        |
| `radius`   | int    | How far the search reaches from each exit in x and z.                                                                                                                     |
| `outcomes` | array  | `{exits, tied, portals}`: how many exit blocks land at `portals`, most first. No portals: the game builds one. `tied`: more than one, and the game's pick can't be known. |

Each of an outcome's `portals` is `{source, index}` and that portal's fields: `source` `given` with a portal's fields, `index` its place among `--portal`s from 0; or `save` with a save portal's, `index` its place in the save's portals, as `--world` alone lists them.

**From a save** (`--world` alone), `portals` lists every portal: `index`, a save portal's fields, `to`, `outcomes` as above but with `portals` as indices, and `flags`, each `{kind}`: `mislinked` (with `to`, the portal it leads to, and `back`, where that one leads instead), `split`, `unlinked`, `tied` or `too-low` ([portals.md](portals.md#a-saves-portals)). Save portals are the Overworld's first, then the Nether's and the End's, each by lowest block (y, then z, then x).

## `render`

The image is written to the file given; the JSON describes it ([render.md](render.md)).

| Field          | Type              | Meaning                                                                                                                                                                                                                                                                                                                                                                                |
| -------------- | ----------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `output`       | string            | The PNG written.                                                                                                                                                                                                                                                                                                                                                                       |
| `area`         | object            | `min` (`[x, z]`, the top-left pixel's corner block), `size` (`[width, height]` in pixels), `scale` (blocks per pixel) and `y`.                                                                                                                                                                                                                                                         |
| `layers`       | object            | What was drawn: `biomes`, `ties`, `slime_chunks`, `generated`, `inhabited`, `spawn`, `confirm` (markers decided with terrain), `relief`, `surface`, `veins`, `noise_caves`, `carvers` and `flowers` (bools), `flower` (the flower shown alone, as a block id, or null), `highlight`, `structures` and `features` (the ids and tags as given), and `loot` (the item as given, or null). |
| `structures`   | array             | The structure markers drawn, in drawing order. Each is a `locate structure` hit without `blocks_away`.                                                                                                                                                                                                                                                                                 |
| `features`     | array             | The feature markers drawn, in drawing order: certain and likely answers, each a `locate feature` hit without `blocks_away`.                                                                                                                                                                                                                                                            |
| `containers`   | array             | The loot rings drawn, in drawing order: each a [`loot`](#loot) container without `blocks_away`.                                                                                                                                                                                                                                                                                        |
| `slime_chunks` | array of `[x, z]` | The slime chunks shaded, by z, then x. Empty unless asked for.                                                                                                                                                                                                                                                                                                                         |
| `generated`    | array             | With `--generated`, the save's chunks shaded, by z, then x: `chunk` (`[x, z]`), `full` (generation finished), `older` (an older version saved it), `readable`, and `inhabited_time` (ticks players spent near it, or null). Empty otherwise.                                                                                                                                           |
| `spawn`        | object or null    | The spawn marker, as `locate spawn`'s fields; null if not asked for or not in view.                                                                                                                                                                                                                                                                                                    |
| `pins`         | array             | The `--pin` pins drawn, in order: each has `position` (`x`, `z`) and `colour` (`#rrggbb`, lowercase). Pins out of view aren't listed.                                                                                                                                                                                                                                                  |

## `search`

The best seeds for a query ([search.md](search.md)). A search is over many seeds, so its object has no `seed` or `world`, and has `warnings` only for `unchecked_version`: it starts with `format`, `command` (`search`), `version` and `dimension` (the first constraint's; each constraint has its own), then:

| Field             | Type        | Meaning                                                                                                  |
| ----------------- | ----------- | -------------------------------------------------------------------------------------------------------- |
| `query`           | string      | The query's canonical text, one constraint per line.                                                     |
| `source`          | object      | `kind`: `all`, `range` (with `first` and `last`, both included) or `list` (with `count`).                |
| `from`            | int         | The first index searched.                                                                                |
| `certain`         | bool        | Whether only seeds whose required constraints are certainly met were reported, ranked by the worst case. |
| `confirm`         | bool        | Whether structure starts were decided with each seed's terrain ([search.md](search.md#confirmed-mode)).  |
| `top`             | int         | How many seeds were kept.                                                                                |
| `next`            | int or null | The first index not searched; `--from` it to continue. Null when the source is finished.                 |
| `evaluated`       | int         | Indices searched: `next − from`, or to the end of the source.                                            |
| `prefiltered`     | int         | Seeds rejected by structure placement alone.                                                             |
| `checked`         | int         | Seeds checked further.                                                                                   |
| `reported`        | int         | Seeds that met the required constraints, whether or not they made the list.                              |
| `elapsed_seconds` | number      | How long the search took. The only field that differs between runs of the same search.                   |
| `hits`            | array       | The best seeds, best first: by `score`, then by `index`.                                                 |
| `partial`         | bool        | Whether a second Ctrl-C cut evaluating the best seeds short: then `hits` has only the best of them.      |

Each hit has `index`, `seed`, `score`, `verdict` (`met` or `possibly`) and `constraints`, one per line of the query, each with:

| Field        | Type   | Meaning                                                                                                         |
| ------------ | ------ | --------------------------------------------------------------------------------------------------------------- |
| `constraint` | string | The constraint's canonical line.                                                                                |
| `dimension`  | string | The dimension it counts in ([search.md](search.md#dimensions)); its features' positions are in its coordinates. |
| `required`   | bool   | Whether it's `require`.                                                                                         |
| `outcome`    | string | `met`, `possibly` or `failed`.                                                                                  |
| `best`       | int    | Its satisfaction in the best case, 0 to 1000.                                                                   |
| `worst`      | int    | Its satisfaction in the worst case.                                                                             |
| `features`   | array  | What it matched. Each has a `kind` and that kind's fields, as below. Empty for `covers`.                        |

Feature kinds; more may be added within format 1:

- `structure`: a `locate structure` hit without `blocks_away`.
- `feature`: a `locate feature` hit without `blocks_away`.
- `container`: a loot container, as [`loot`](#loot)'s, without `blocks_away`.
- `slime_chunk`: `chunk`.
- `biome`: `position` (`x`, `y`, `z`), `biome`, `tied` (as `locate biome`) and `certain`.
- `spawn`: `locate spawn`'s fields.
- `flower_field`: `position` (`x`, `y`, `z`: the field's first block, at its height), `flower`, `blocks` (its size) and `certain` (whether it holds in the worst case).
- `other`: a kind the library added that this build of the command can't describe yet. No other fields.

## `find`

Places in one seed or save that fit a `find` query ([search.md](search.md#searching-within-a-seed)). The header is the usual one (`format`, `command` (`find`), `version`, `dimension` (the target's), `seed`, and with `--world`, `world` and `warnings`), then:

| Field             | Type     | Meaning                                                                                              |
| ----------------- | -------- | ---------------------------------------------------------------------------------------------------- |
| `query`           | string   | The canonical text, its `find` line first.                                                           |
| `origin`          | `{x, z}` | What `within R` alone is around, in the target's dimension.                                          |
| `certain`         | bool     | Whether only certain matches, at certain targets, were kept.                                         |
| `confirm`         | bool     | Whether structure starts were decided with terrain.                                                  |
| `top`             | int      | The most matches asked for.                                                                          |
| `targets`         | int      | How many targets the place has.                                                                      |
| `judged`          | int      | How many were judged, nearest first.                                                                 |
| `complete`        | bool     | Whether every target was judged, or `top` matches found: false if it was stopped or ran out of time. |
| `elapsed_seconds` | float    | How long it took.                                                                                    |
| `matches`         | array    | Nearest first.                                                                                       |

Each match has `position` (`{x, z}`), `distance` (from the place's centre, rounded down), `target` (the structure start, feature, container or biome sample, as in `search`, or null for a point), `score`, `verdict`, and `constraints`, as a `search` hit's, without the `find` line.

## `world info`

`format`, `command` (`world info`), and the save:

| Field                   | Type           | Meaning                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| ----------------------- | -------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `name`                  | string         | The name in the world list.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| `dir`                   | string         | The save's folder.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| `data_version`          | int or null    | The data version it was last saved in; null before 1.9.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| `version_name`          | string or null | e.g. `26.3`.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| `seed`                  | int            | The world seed.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| `generate_structures`   | bool           | Whether structures generate.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| `data_packs`            | array          | Enabled data packs, in load order.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| `applied_packs`         | array          | Deprecated, and to be removed in the release after next: `packs` has every pack. The packs spyglass applies, in load order, each `{name, hash, worldgen_files}` ([data-packs.md](data-packs.md#saves)).                                                                                                                                                                                                                                                                                                                                                |
| `spawn`, `player`       | object or null | `dimension` and `position` (`[x, y, z]`, numbers); `player` only in single-player saves.                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| `bundle`                | string or null | The bundle with the save's data version.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| `answerable`            | bool           | Whether spyglass answers for the save: no warning blocks answers.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| `answerable_dimensions` | array          | The ids of the dimensions spyglass answers for: those no warning blocks, if there's a bundle.                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| `warnings`              | array          | [Warnings](#warnings), including the blocking kinds `no_bundle`, `unsupported_generator` and `pack_error`. `older_chunks` and `unreadable_regions` only without `--quick`.                                                                                                                                                                                                                                                                                                                                                                             |
| `pack_errors`           | array          | Only when there are some: `{dimension, message}` for each `pack_error` warning: an answerable dimension the applied packs can't be opened in, e.g. for a type spyglass doesn't support.                                                                                                                                                                                                                                                                                                                                                                |
| `settings`              | array          | Every value spyglass read from the save, in a fixed order ([world-saves.md](world-saves.md#settings)): `{setting, dimension?, value, file, field, present, decides}`. `setting` is `version`, `data_version`, `seed`, `structures`, `generator`, `border`, `spawn`, `player` or `modded`; `generator` and `border` have `dimension`. `value` and `decides` are for people. `file` is relative to the save and `field` an NBT path; `present` is false where the file doesn't have it and the game's default is used.                                   |
| `packs`                 | array          | Every enabled pack in load order, `vanilla` first: `{name, used, hash?, answers, changes?}`. `used` is `bundle`, `applied`, `feature` or `not_applied`; `hash` and `changes` are for applied packs. `changes` is by registry, each `{replaced, added, merged, hidden, shadowed, unreadable}`, arrays of ids ([data-packs.md](data-packs.md#what-a-pack-changes)); `answers` is which of `biomes`, `terrain`, `structures`, `features`, `loot`, `flowers` and `portals` they touch.                                                                     |
| `dimensions`            | array          | The Overworld, the Nether and the End: `{dimension, region, answerable, holdings}`. `holdings` is null with `--quick`, else `{chunks, full, proto, unreadable, unreadable_files, versions, inhabited_ticks, bounds, starts, dropped_starts, older_starts, sealed_containers}` ([world-saves.md](world-saves.md#dimensions)): `versions` newest first, each `{data_version, version, chunks}` (`version` the bundle's name for it, or null); `bounds` `[[min_x, min_z], [max_x, max_z]]` in chunks, or null; the counts each `{id, count}`, most first. |

## `world list`

`format`, `command` (`world list`) and `saves`: one object per save in the known launchers' folders, as `world info` without `format` and `command` (and without the chunk scan, so no `older_chunks` or `unreadable_regions`), plus `launcher` (e.g. `Modrinth (Fabric 26.3)`). A save that can't be read is `{launcher, dir, error}`.
