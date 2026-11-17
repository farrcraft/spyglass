# Data packs

Status: draft. Implemented in `spyglass-data` (`pack`, `Bundle::with_packs`), `spyglass-world` (`open_pack`) and `spyglass` (`World::open_with_packs`, `Game::open_with_packs`, saves). The rules below are the 26.3 server's, read from its bytecode on 2026-11-11; 26.2 and the 26.4 snapshots load packs through the same classes. Checked against the game with [the test pack](oracle.md#the-test-pack) and [the fixture pack](oracle.md#the-fixture-pack).

A world's data packs change its generation the way they change everything else: a pack's files replace vanilla's, or add to them, in the game's registries. spyglass keeps worldgen parameters in data ([architecture.md](../architecture.md)), so it applies a pack by reading the pack's files in place of the bundle's, as the game reads them in place of its vanilla pack's ([ADR 0072](../adr/0072-data-packs-as-layers.md)).

## The game's rules

### Which packs, in which order

A world's packs are `level.dat`'s `Data.DataPacks.Enabled`, in that order, lowest first (`MinecraftServer.configurePackRepository`, `PackRepository.rebuildSelected`, `openAllSelected`):

- `vanilla`, the game's own data.
- Built-in feature packs, by bare name: `trade_rebalance`, `minecart_improvements`, `redstone_experiments`. They're folders in the jar, `data/minecraft/datapacks/<name>/` (`ServerPacksSource`). In 26.3, none of them has worldgen files; `trade_rebalance` changes six chest loot tables and nine enchantment tags.
- `file/<name>`: a folder or zip in the save's `datapacks/` (`FolderRepositorySource`). A folder is a pack only if it has a `pack.mcmeta` file; a file is one only if its name ends in `.zip` (case-sensitive). The id keeps the `.zip`.
- Anything else is a mod's.

No server pack is required or fixed in place, so nothing is moved: the list is the load order. A duplicate counts once, where it first appears. A listed pack that isn't available is dropped with a "Missing data pack" warning. On start, the game also appends, at the top, every available pack that's in neither `Enabled` nor `Disabled` ("Found new data pack"): a pack put in `datapacks/` after the world was made loads from then on.

### Which file wins

For one file (a resource id with its extension, such as `minecraft:worldgen/noise/temperature.json`), the highest pack that has it wins and lower packs' copies aren't read (`FallbackResourceManager.getResource`). A registry is every file any pack has under its folder, each read from its highest pack (`listResources`), so a pack's new ids join the registry.

A pack's `pack.mcmeta` may have a `filter`: `{"block": [{"namespace": regex, "path": regex}, ...]}`, either pattern optional, matching everything when absent. A file of a **lower** pack is hidden if any entry's patterns both match its namespace and its path (`ResourceFilterSection`, `IdentifierPattern`). The patterns are Java regexes matched with `Pattern.asPredicate`: anywhere in the text, not the whole of it. The pack's own files aren't filtered, nor are higher packs'.

### Tags merge

Tags are the exception: every pack's file for a tag is read, lowest first (`TagLoader.load`, `getResourceStack`), each adding its `values` to the list, and a file with `"replace": true` clearing what lower packs gave. A filter hides lower packs' files as it does any file. A file that doesn't parse as a tag (`TagFile.CODEC`) is skipped ("Couldn't read tag list") and the rest kept: `values` not a list, an entry that isn't an id or `#tag` an `Identifier` can hold or an object with a string `id`, or a `replace` or `required` that isn't a boolean. DFU's `JsonOps` reads a number as a boolean too, true if its low byte isn't 0 (seen in 26.3: a pack's `is_ocean` of `[3]` was skipped, the world loaded). Building a tag (`TagLoader.build`) keeps each element once, in the order first named. An entry is an id, a `#tag`, or `{"id": ..., "required": bool}` (default `true`); an entry that doesn't resolve is skipped if it isn't required, and if it is, the whole tag is dropped with an error, and loading goes on.

### `pack.mcmeta`

```json
{
  "pack": { "description": "...", "min_format": 121, "max_format": [127, 0] },
  "overlays": {
    "entries": [
      { "directory": "new_format", "min_format": 125, "max_format": 127 }
    ]
  },
  "filter": {
    "block": [{ "namespace": "minecraft", "path": "worldgen/biome/" }]
  }
}
```

`pack` is required, with a `description` (any text component). Its formats (`PackFormat`, `PackMetadataSection`) are, from 1.21.9 on, `min_format` and `max_format`: a non-negative int `N` or a list of 1 to 256 (`[N]`, `N.0` as a minimum, `N.2147483647` as a maximum; `[N, M, ...]`, `N.M`). Every int here is a JSON number read as `Number.intValue()` (`Codec.INT`): `121.0` is 121, and `4294967417` keeps its low 32 bits, 121 (26.3 loaded a pack declaring both). Before, `pack_format` (an int) and `supported_formats` (an int, `[min, max]` or `{"min_inclusive", "max_inclusive"}`). Format 81 is the last without a minor part, and the validation (`IntermediaryFormat.validate`) follows from it:

| Fields                     | Range                                                                                                                                                                                                                                                 |
| -------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `min_format`, `max_format` | `[min, max]`, with `min <= max`. If `min` is above 81, `supported_formats` is an error, and a `pack_format` must be in the range. At 81 or below, `supported_formats` must be `[min.major, max.major]` (or end at 81), and `pack_format` is required. |
| only one of them           | an error                                                                                                                                                                                                                                              |
| `supported_formats`        | `[a.0, b.0]`, with `b <= 81` and a `pack_format` in it                                                                                                                                                                                                |
| `pack_format` alone        | `[f.0, f.0]`, with `f <= 81`                                                                                                                                                                                                                          |
| none                       | an error                                                                                                                                                                                                                                              |

A range of more than one format can't start below 15. A version's own format is `version.json`'s `pack_version.data_major` and `data_minor`: 107.1 for 26.2, 121.0 for 26.3, 127.0 for 26.4-snapshot-8. Formats compare by major, then minor.

What the game does with a pack (`Pack.readMetaAndCreate`, `readPackMetadata`):

- **Formats that don't cover the version, or don't validate:** the pack still loads when a world enables it. The selection screen marks it incompatible (`PackCompatibility`), and a `pack` section that fails validation leaves it of unknown compatibility. Nothing on the server's path checks compatibility.
- **No `pack.mcmeta`, no `pack` section, no description, or not JSON:** the pack isn't created ("Missing metadata in pack"), so it's dropped as missing.
- **Broken `overlays`:** the pack isn't created.

### Overlays

`overlays.entries` lists folders at the pack's root, each with `directory` (matching `[-_a-zA-Z0-9.]+`, the whole name) and its formats: `min_format` and `max_format` as above, or the older `formats` range (`OverlayMetadataSection`). An entry with no format field is skipped ("Unknown or broken overlay entry"). If any entry reaches format 81 or below, every entry must have `formats`. Any other bad entry fails the list, and the pack.

The overlays that apply are those whose range holds the version's format, in the order listed (`overlaysForVersion`). They and the root make one pack (`OverlayedPackResources`): a file is read from the **last** listed overlay that has it, then earlier ones, then the root. Within a pack, an overlay's tag file replaces the root's copy; it isn't merged, since the pack is one entry in the tag's stack.

A zip pack is read the same way, its overlays being prefixes of its entries (`FilePackResources`).

## What spyglass applies

### Packs as layers

A bundle is laid out as a data pack ([bundle-format.md](bundle-format.md#layout)), and every loader reads it through four `Bundle` methods. `Bundle::with_packs(packs)` returns the bundle with packs layered over it, lowest first, and those methods look through the layers as the game's resource manager does:

- `resource(registry, id)`: the highest layer's file, unless a filter above hides the lower ones; for a tag (`tags/...`), every layer's file merged as [above](#tags-merge), into one `{"values": [...]}`. A pack's unreadable tag file is skipped.
- `registry_ids(registry)`: the union of every layer's ids, less those hidden.
- `structure_template(id)`: as `resource`.
- `report(path)`: the bundle's own. Reports are the data generator's output, not a pack's.

Each pack is applied for the bundle's data pack format, recorded by the extractor ([bundle-format.md](bundle-format.md#bundlejson)), which picks its overlays. A bundle that doesn't record its format can't have packs applied; extracting it again records it. A layered bundle's `identity()` is the bundle's directory and each pack's name and content hash, for telling worlds apart.

Tag entries that don't resolve stay spyglass's own rule: a bundle has no registries to check ids against, so ids are kept and only a missing optional tag is skipped ([density-functions.md](density-functions.md)). A pack whose required entry names an id that doesn't exist makes the game drop the tag; spyglass keeps it.

### Reading packs

`spyglass_world::open_pack(path)` reads a folder or a zip (`spyglass-data` reads no archives, [ADR 0016](../adr/0016-extractor-crate-and-read-only-data.md); it parses and layers what `PackReader` is given): its `pack.mcmeta`, parsed and validated as [above](#packmcmeta), and every file under `data/` at its root or in a folder one level down (where overlays are). The rest (`assets/`, functions' siblings at the root) isn't read. Packs are untrusted input, as saves are ([world-saves.md](world-saves.md#limits)):

- at most 200,000 entries, 32 MiB a file after inflating, and 512 MiB a pack;
- a structure template's NBT at most 64 MiB after its own decompression, and its tree at most 64 MiB in memory (`nbt::MAX_TEMPLATE`). The game has no such limit, but a few kilobytes of gzip can inflate to gigabytes; the largest vanilla template is about 1 MiB. A template past it fails to load as malformed NBT does;
- a file is skipped unread, as the game skips it ("Invalid path in pack", seen in 26.3 for `worldgen/noise/Bad_Name.json`), if its namespace or path has a character an `Identifier` can't hold (`[a-z0-9_.-]`, and `/` in a path): uppercase, `\`, `:`, a space. spyglass also skips a path with an empty, `.` or `..` segment, which the game would take into the id; nothing reads a pack's files by filesystem path, so none can reach outside it;
- symbolic links in a folder aren't followed.

A `filter` pattern is read with `regex_lite`, whose syntax covers the patterns packs use; a pattern it can't read is an error, since the pack couldn't be applied as the game applies it. `\w`, `\d` and `\s` are ASCII in both, as in Java without `UNICODE_CHARACTER_CLASS`.

A pack's content hash is the SHA-1 of its `pack.mcmeta`'s and every file's SHA-1s, by path.

### Worlds

`World::open_with_packs(dir, dimension, packs)` and `Game::open_with_packs(dir, packs)` open the bundle with packs applied. A world with packs loads everything it otherwise loads on first use (template pools, block settings, features, loot) when it opens, so a pack that holds a type spyglass doesn't support, or a file it can't read, is an error naming the file then, not a failed query later. An error that the bundle without the packs has too (a bundle extracted before some part was added) is left for first use, as without packs.

A pack's new biomes get the map's hashed colour for ids it doesn't know ([ADR 0018](../adr/0018-biome-palette.md)).

### Saves

`SaveReport` applies a save's enabled packs, in order, and lists them in `packs`: `file/` packs read from `datapacks/`, and built-in feature packs from the bundle ([bundle-format.md](bundle-format.md#feature-packs)). `open_world` and `open_game` apply them. `SaveWarning::DataPack` is raised for a pack spyglass can't apply, or applies with a caveat ([world-saves.md](world-saves.md#warnings)):

| Concern        | When                                                                               | Answers |
| -------------- | ---------------------------------------------------------------------------------- | ------- |
| `missing`      | a `file/` pack whose folder or zip isn't in `datapacks/`                           | inexact |
| `mod`          | a pack that's neither `vanilla`, a feature pack the bundle has, nor `file/`        | inexact |
| `unreadable`   | over the limits, not a zip, or an I/O error                                        | inexact |
| `skipped`      | a pack the game doesn't load either: no `pack.mcmeta`, not `.zip`, broken metadata | a note  |
| `other_format` | applied, but its formats don't cover the version's                                 | a note  |

`SaveReport::pack_errors` opens each answerable dimension with the packs, for what can't be applied, and `spyglass world info` reports them as errors.

Packs found in `datapacks/` but in neither list aren't applied: the game adds them on its next start, but the chunks so far were made without them.

### What a pack changes

`Bundle::layer_changes()` says, for each applied pack, what it changes, from the layers' file lists, as [which file wins](#which-file-wins) decides; only tag files are read, for `replace`. Per registry (the folder under `data/<namespace>/`, such as `worldgen/biome` or `tags/worldgen/biome`), a pack's file is:

- **replaced** when a layer below, the bundle included, has a file at the same path that's still in view, and **added** when none has;
- for a tag, **merged** with what's below, or **replaced** when it says `"replace": true` (a file that doesn't read as a tag is skipped, as the game skips it);
- **shadowed** when a higher pack has the same path, or hides it with its filter: it never takes effect. A shadowed tag is still merged, unless a higher pack replaces or hides it.

Its `filter` **hides** the files of the packs below it that it matches, counted per registry the same way. Structure templates (`structure/`, `.nbt`) are counted as a registry of their own.

`spyglass::answers_fed_by(registry)` names the answers a registry feeds, so a pack's changes say which answers they touch:

| Answers    | Registries                                                                                                                                                                                                                                                              |
| ---------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| biomes     | `worldgen/biome`, `worldgen/multi_noise_biome_source_parameter_list`, `worldgen/density_function`, `worldgen/noise`, `worldgen/noise_settings`, `worldgen/world_preset`, `dimension`, `dimension_type`                                                                  |
| terrain    | the biomes' registries except `worldgen/multi_noise_biome_source_parameter_list`, and `worldgen/material_rule`, `worldgen/material_condition`, `worldgen/carver`, `worldgen/configured_carver`, `tags/worldgen/carver`, `tags/worldgen/configured_carver`, `tags/block` |
| structures | `worldgen/structure`, `worldgen/structure_set`, `worldgen/template_pool`, `worldgen/processor_list`, `structure` (templates), `tags/worldgen/biome`, `tags/worldgen/structure`                                                                                          |
| features   | `worldgen/biome`, `worldgen/configured_feature`, `worldgen/placed_feature`, `worldgen/feature`, `worldgen/block_state_provider`, `tags/worldgen/feature`, `tags/worldgen/configured_feature`, `tags/worldgen/placed_feature`, `tags/block`                              |
| loot       | `loot_table`, `enchantment`, `tags/item`, `tags/instrument`, `tags/enchantment`                                                                                                                                                                                         |
| flowers    | the features' registries, except `tags/worldgen/placed_feature` and `tags/block`                                                                                                                                                                                        |
| portals    | `worldgen/noise_settings`, `dimension`, `dimension_type`, `tags/block`                                                                                                                                                                                                  |

A test reads the library crates' sources for every registry name a loader uses and checks each feeds an answer.

Any other registry, such as `recipe`, `advancement` or `function`, is counted as not read by spyglass, so a pack of recipes only changes no answers.

## Testing

- **Unit tests** on packs built in the test: formats in every form, valid and not; packs the game skips; overlays chosen by format, the last on top; filters matching anywhere; zips and folders alike; oversized zip entries, and escaping or invalid paths skipped. On layered bundles: replacement, new ids, filters above and below, tag merging with and without `replace`, unreadable tag files, feature packs, and a bundle without a format.
- **Saves** built in the test: each concern, a pack that turns the fixture's temperature round and changes its biomes, through `open_world` and `open_game`, and a pack with an unsupported density function type failing on opening.
- **The oracle:** [the test pack](oracle.md#the-test-pack), loaded by the harness as the game loads a world's pack and applied by spyglass over the plain bundle; and [the fixture pack](oracle.md#the-fixture-pack), through every comparison.
- **Fuzzing:** `cargo xtask fuzz pack` on `open_pack` with arbitrary zips, `pack.mcmeta` included.

### A widely used pack

Terralith 2.6.5 for 26.3, the data pack edition (`Terralith_v2.6.5+26.3.zip`, 2,619 entries), downloaded by hand into the gitignored `packs/`, loaded by the harness with `--pack packs/Terralith_v2.6.5+26.3.zip --name terralith` and compared as `26.3+terralith` (2026-11-13). It replaces the Overworld's noise settings, parameter list and surface rules, overrides 40 vanilla biomes and adds 97, and brings 50 structures and 300 templates.

What agrees with the game, every value of the golden set: the noise probe, density functions, biomes (every golden cell), the aquifer, the noise fill, spawn, heights, and every block of the golden chunks without carvers and with them.

What spyglass doesn't support, so a world with Terralith isn't answered ([above](#worlds)), each the first such file of its kind:

- **Old templates.** 290 of its 300 structure templates were saved by older versions (data versions 2730 to 4325, 1.17 to 1.21.x), which the game brings up to date with its data fixers when it loads them. spyglass reads only the version's own (`terralith:mage/tower`, 3839).
- **Feature forms:** a `minecraft:weighted` block state provider as a `filling_provider` (`terralith:taiga/mega/boulders`), and doubtless more behind it.
- **Loot:** a `minecraft:tag` entry (`terralith:spire/rare`).

A world with packs loads every template pool, feature and loot table when it opens, not only its dimension's, so every dimension fails on opening, the Nether and the End too though Terralith doesn't change them: structures, features, containers, loot, locate, search and rendering. In the full comparison run (`cargo xtask oracle-test` with the variant present), every test that opens a world or loads the pack's structures, features or loot fails on one of these three errors, 33 of 61; the other 28 pass (2026-11-15).

It found faults that the vanilla data, the test pack and the fixture pack don't reach: whether a biome, tag or structure exists was looked up in the bundle and not the packs (`Bundle::has_resource`); surface rules and noise settings read only 26.2's block state form, not 26.3's `{"id", "properties"}`; carvers didn't keep [the game's spare gaussian](carvers.md) from one carver to the next. In the harness, the run without carvers replaced the pack's biomes with vanilla's, and its files were named after the pack's file rather than `--name`.
