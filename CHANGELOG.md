# Changelog

What changed in each release of spyglass, for people who use it. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html) as [ADR 0037](docs/adr/0037-versions-and-releases.md) applies it. While the version starts with `0.`, a minor release may change the library's API, the command's options or its `--json` output; a patch release never does. A release that adds a game version is a minor release.

Changes go under **Unreleased** as they're made. `cargo xtask release` turns that section into the release's, and adds the game versions the release was checked against.

## [Unreleased]

## [0.5.1] - 2026-11-18

### Game versions

Checked against the game for 26.2, 26.3, 26.4-snapshot-5, 26.4-snapshot-6, 26.4-snapshot-7, 26.4-snapshot-8, 26.4-snapshot-9.

### Added

- **26.4-snapshot-9**, checked against the game. Nothing spyglass answers changed: its worldgen differs from snapshot-8's only in the names of the noise settings' debug functions.

## [0.5.0] - 2026-11-17

### Game versions

Checked against the game for 26.2, 26.3, 26.4-snapshot-5, 26.4-snapshot-6, 26.4-snapshot-7, 26.4-snapshot-8.

### Added

- **`spyglass world info` explains what spyglass read from your world:** every setting, the file and field it came from and what it decides; every enabled data pack, whether it was applied, what it changes and which answers that touches; and for each dimension, how many chunks it holds, which versions generated them, the structures answered from the save and the loot containers not yet opened. Each warning is printed under what it's about. In JSON, `settings`, `packs` and `dimensions`, and each warning's `about` ([cli-json.md](docs/specs/cli-json.md#world-info)). In the API, `SaveReport::details`, `spyglass_world::Save::sources`, `Bundle::layer_changes`, `answers_fed_by` and `SaveChunks::usage` ([world-saves.md](docs/specs/world-saves.md#the-report)).
- **A world border that's still moving** is a note (`moving_border`): spyglass answers with the size it ends at.
- **Maps from pixels already known:** `Known` holds the pixels of an area known from maps at other scales, and `biome_map_with`, `height_map_with`, `surface_biome_map_with`, `carving_map_with`, `underground_map_with` and `flower_map_with` compute only the rest, giving the same map as computed directly. With every pixel known, they compute nothing ([render.md](docs/specs/render.md#reuse-across-scales)).
- **`StructureQuery::with`** joins two structure queries into one, so a start either names is listed once, and is certain when one of their structures certainly starts there.
- **Map calls can be stopped:** `Session::stopping(&stop)` gives the session's map calls (`biome_map`, `height_map`, `surface_biome_map`, `carving_map`, `underground_map`, `flower_map`, `map_features`, `structures_in`, `features_in`, `containers_in`, `containers_holding`), which return the new `Error::Stopped` soon after `stop` is raised from another thread. `Stop` is the search's stop handle under a name for both ([ADR 0078](docs/adr/0078-map-calls-can-be-stopped.md), [render.md](docs/specs/render.md#stopping)).

### Changed

- **The viewer's side panel is in sections that fold,** on every tab, the World tab's included. A folded section says what's in it ("2 on", "on, paused", "3 warnings"), and folds are kept between runs. The selected tab stands out, and its counts are badges: the search's finds, locate's hits, your markers here, and the save's warnings in amber.
- **The viewer is clear about zoom limits, and quicker to zoom.** Every layer that's drawn only up close is greyed past its limit, with hover text saying how far to zoom in, and stays on, paused, until you zoom back in. Zooming out stops the work the new view doesn't need within a fraction of a second. Features, loot and structures are kept by area rather than by tile, so zooming from 1:2 to 1:1 with features on places nothing again, and a tile zoomed out from cached ones is copied from them. One step past their limit, the features and loot found closer in are still drawn, the area looked at outlined and the rest hatched ([render.md](docs/specs/render.md#reuse-across-scales)).

- **`SaveWarning` has two new variants,** `MovingBorder` and `PackError`, and from now on it, `PackConcern` and the new `SaveSubject`, `Setting` and `PackUse` are `#[non_exhaustive]`: a `match` on them outside the crate needs a `_` arm, and later variants won't break it ([ADR 0077](docs/adr/0077-open-ended-public-enums-are-non-exhaustive.md)). A dimension the packs can't be opened in is a `pack_error` warning in `world info`'s `warnings`, as well as in `pack_errors`.
- **`world info`'s `applied_packs` is deprecated,** for removal in the release after next: `packs` lists every pack, applied or not.
- **`SearchStop` is now `Stop`,** since map calls take it too. `SearchStop` is kept as a deprecated alias for one release.

### Fixed

- **The viewer's World tab mixed two saves** when a save it couldn't show was opened while another was on the map: the tab named the one on the map but listed the other's settings and warnings, and the notice counted the wrong save's warnings. Now the tab is about the save that couldn't be shown until its error is dismissed, and the notice about the one on the map.
- **Chunks saved before 1.18** are read for their status and inhabited time, which they keep under `Level`: `render --generated` and `--inhabited` showed them all as generated partway and never visited.

## [0.4.0] - 2026-11-14

### Game versions

Checked against the game for 26.2, 26.3, 26.4-snapshot-5, 26.4-snapshot-6, 26.4-snapshot-7, 26.4-snapshot-8.

### Added

- **Structures in your world come from the world itself** where it has generated the chunk, as the game's `/locate` does: `--world` answers mark them `saved` (`source` in JSON), and a chunk the world saved without a structure has none, whatever the seed says. In the API, `SaveReport::chunks` and `Session::with_save` ([world-saves.md](docs/specs/world-saves.md#structure-starts-in-saves)).
- **Loot in your world says what's become of each container:** unopened (holding what spyglass shows), opened (with what's in it now) or gone, and `loot find` goes by what's in it now.
- **`spyglass render --generated`** shades the chunks your world has generated, marking those an older version made; `--inhabited` shades them by the time players spent there. The viewer has both, under **Generated chunks**.
- **Data packs change spyglass's answers as they change the game's.** With `--world` and in the viewer, the world's enabled packs are applied, in order: those in its `datapacks/` folder, zips and folders, and the built-in ones such as `trade_rebalance`. `--pack <zip|folder>` applies packs for a seed, in every command that takes `--seed`. Overlays, filters and tag merging follow the game ([data-packs.md](docs/specs/data-packs.md)). In the API, `Bundle::with_packs`, `World::open_with_packs`, `Game::open_with_packs` and `SaveReport::packs`.
- **`spyglass world info` lists the packs it applied** (`applied_packs` in JSON), and reports a dimension the packs can't be applied in, with the file that holds what spyglass doesn't support (`pack_errors`).
- **Bundles record the version's data pack format and carry the built-in feature packs.** Extract your bundles again to apply packs; spyglass says so if you don't.

### Changed

- **Data pack warnings say only what spyglass can't apply:** a missing pack, a mod's pack or one it can't read. A pack the game doesn't load either, or one made for another format, is a note. In JSON, each `data_pack` warning has a `concern`; in the API, `PackConcern` has new variants in place of `Worldgen` and `Uninspectable`, and `PackContents` holds the read `Pack`.
- **Java's `Math.sin`, `cos`, `log` and `pow` are computed correctly rounded,** as the game computes them on x86-64 for all but 0.1% of arguments or fewer, the same on every system. Before, spyglass's Windows, Linux and macOS builds each used their system's math library, which differ from each other, and on Windows from the game in about 3% of `sin` and `cos` results. Stronghold ring positions, the spawn search, Gaussians and large dripstone heights can change where they hung on that last bit; stronghold rings and the spawn search still flag answers the game's own platforms could disagree on ([java-math.md](docs/specs/java-math.md)).

### Fixed

- **A stronghold whose ring position is uncertain** (a tied biome cell, or a sine the game's platforms may round differently) had its chests answered as certain, in spyglass's chunk only. They're now candidates, in every chunk the game may put the stronghold in, as `locate` already gave it.
- **A data pack's own biomes in a structure's biome tag** (Terralith's caves in `#minecraft:has_structure/ancient_city`) were refused as unsupported: the check looked in the bundle only, not the packs.
- **Block states written as 26.3 writes them** (`{"id": …, "properties": …}`) in a pack's surface rules and noise settings were refused; only features read that form.
- **Carvers whose shape draws a normal value** (a pack's `clamped_normal`, such as Terralith's canyons) carved from a fresh random's draws. The game keeps one random for a chunk's carvers, whose spare value carries from one carver to the next.
- **A chest placed over another** (an overgrown pillager outpost watchtower keeping its copy of the tower's chest) was answered with the first chest's loot seed. The later chest replaces it, as in the game.

## [0.3.1] - 2026-11-11

### Game versions

Checked against the game for 26.2, 26.3, 26.4-snapshot-5, 26.4-snapshot-6, 26.4-snapshot-7, 26.4-snapshot-8.

### Added

- **26.4-snapshot-8**, checked against the game. Its carving mask spans the noise's heights again, within the level's, so Nether caves stop at y 120 as in snapshot-5 ([carvers.md](docs/specs/carvers.md#264-the-carving-mask-with-the-rule)).

### Changed

- **Biomes at the surface and relief at every scale.** They were drawn only at 4 blocks per pixel or closer, so zooming out switched the viewer from surface biomes to biomes at one height, often a cave biome under mountains. They're slow everywhere, and slower zoomed out: a screen takes a few seconds at 1:4 and up to a minute or so from 1:32 out. `spyglass render --surface` and `--relief` take any scale, and the library's `MAX_TERRAIN_SCALE` is gone ([render.md](docs/specs/render.md#heights)). In the viewer, the quicker biomes at the chosen height show while they're worked out.

## [0.3.0] - 2026-11-09

### Game versions

Checked against the game for 26.2, 26.3, 26.4-snapshot-5, 26.4-snapshot-6, 26.4-snapshot-7.

### Added

- **A user guide** at <https://farrcraft.github.io/spyglass/>, published with each release: installing, game data, the viewer, the `spyglass` command, maps, seed searches with worked queries, searching within a seed, your own worlds, reading the answers, and troubleshooting. Release notes link to it.
- **26.4-snapshot-7**, checked against the game.
- **Pieces of strongholds, fortresses, end cities, woodland mansions and ocean ruins**, laid out as the game lays them out and checked against the game in every supported version. `Session::structure_layout` gives them, so their exact bounds show where jigsaw structures' and mineshafts' already did. Features near them, and near mineshafts, are decided by those pieces rather than by everything within 8 chunks, which settles most of the dungeons that were candidates for that reason ([structure-placement.md](docs/specs/structure-placement.md#legacy-pieces)).
- **Loot:** `spyglass loot at x,y,z` shows a container slot by slot, `spyglass loot in` lists the containers in a region (`--item` keeps those holding an item), and `spyglass loot find minecraft:enchanted_golden_apple` finds the nearest holding one, with `--enchantment` and `--level`, on a seed or a world save, with `--json` ([cli-json.md](docs/specs/cli-json.md#loot)). Maps ring the containers holding an item up close: `spyglass render --loot minecraft:diamond`, `Layers::loot`, and in the viewer a **Loot** section in the Layers tab that marks every container and rings those holding an item; clicking one opens its contents, and **Nearest** the item is in the map's menu ([render.md](docs/specs/render.md#layers)). In the library, `Session::containers_in` gives the loot containers structures and dungeons place in an area, each certain or a candidate with its table, seed and contents: every stack with its count, enchantments, damage, potion, name and an exploration map's destination. `Session::containers_holding` those holding an item, and `Session::nearest_container` the nearest holding an item or item tag, optionally with an enchantment (`World::item_query`). Checked against the game's containers in every supported version ([locate.md](docs/specs/locate.md#loot)).
- **Loot in searches:** `loot I` counts the containers holding an item or item tag in a place, with `with E [level]` for an enchantment, and `find loot I` searches one seed for them: `require loot minecraft:enchanted_golden_apple within 1000 of spawn` finds seeds with one near the spawn. Each seed's chunks are placed until one is found, so that search takes about a minute a seed; a loot place is at most 2,048 blocks on a side. Loot is the Overworld's unless a dimension is written. In `--json`, a search's matches have a `container` kind ([search.md](docs/specs/search.md#outcomes), [ADR 0065](docs/adr/0065-loot-in-the-search-language.md)).

### Changed

- `GETTING-STARTED.txt` in the archives is a short quick start: the first run, getting game data, and a few commands, with the user guide for the rest.

### Fixed

- `spyglass find --help` names every kind of target a query can find: structures, features, loot containers holding an item, biomes and grid points.

## [0.2.0] - 2026-11-01

### Game versions

Checked against the game for 26.2, 26.3, 26.4-snapshot-5, 26.4-snapshot-6.

### Added

- **Features:** dungeons, amethyst geodes, desert wells, fossils and lava lakes, placed as the game decorates a chunk and checked against the game in every supported version. `spyglass locate feature minecraft:monster_room` finds the nearest, `spyglass features` lists them in a region, with `--json`; `World::feature_query`, `Session::features_in` and `Session::nearest_feature` in the library. Maps mark them up close (1 or 2 blocks per pixel): `spyglass render --features minecraft:monster_room`, `Layers::features`, a **Features** section in the viewer's Layers tab, with a diamond per feature, filled when certain, and **Nearest feature** in the map's menu. Features can be marked done ([render.md](docs/specs/render.md#layers)). Each answer is certain or a candidate with its reason, and says whether neighbouring chunks decorated first could change it ([locate.md](docs/specs/locate.md#features), [ADR 0056](docs/adr/0056-two-certainties-on-its-own-and-whatever-the-neighbours.md)).
- **Features in searches:** `feature F` counts features in a place, `cluster N feature F within D of each other` asks for N of them close together (in three dimensions, as a spawner farm needs), and `find feature F` searches one seed for them: `require cluster 2 feature minecraft:monster_room within 16 of each other within 96 of spawn` finds seeds with two dungeons near the spawn. Each seed's features are placed chunk by chunk, so such a search runs at about a seed a second ([search.md](docs/specs/search.md#outcomes), [ADR 0057](docs/adr/0057-features-in-the-search-language.md)).
- **A redesigned viewer:** the seed, version, dimension and save are in a toolbar, the side panel has Layers, Search and Locate tabs, and where game data is kept moves to **Settings**. Zoom, go to, centre on spawn, a scale bar and a legend are on the map. Biomes and structures are shown by name, such as "Cherry grove".
- **Map icons:** each kind of structure has its own icon on a badge in its colour, filled when it certainly generates and hollow when it's a candidate. They shrink to dots when zoomed out. The marker under the pointer is enlarged and named, locate answers are pins, and an opened search hit rings what it matched and outlines each constraint's area. Exported images keep their square markers ([ADR 0044](docs/adr/0044-the-viewer-draws-its-own-map-icons.md)).
- **The viewer extracts game data itself:** with no data yet, it offers a place for it, a game version and an Extract button, with no terminal. **Add a game version** in Settings extracts more later.
- **Search within a seed:** `find` queries name a target (a structure, a biome, or each point of a grid) and constraints around it with `here`, judged at every target in one seed, nearest first. `spyglass find` runs them on a seed or a world save, with `--json`; the viewer's Search tab has a **This seed** mode with presets (double witch huts, a monument in deep ocean, a flat pillager outpost, rare biomes, a base site), and steps through matches with N and P. `Session::find` and `GameSession::find` in the library ([ADR 0047](docs/adr/0047-search-within-a-seed.md), [search.md](docs/specs/search.md#searching-within-a-seed)).
- **Biome tags** such as `#minecraft:is_ocean`, in `/locate biome`, the seed search and `find` queries, as the game takes them.
- **Terrain conditions** in queries: `height A to B` and `flat D` over the terrain's heights in a place.
- **The nearest structure, beside `/locate`'s answer.** The game's `/locate` takes the first structure in its search order, which is often not the nearest: for seed 12345, `/locate minecraft:ancient_city` gives one 407 blocks away while another is 249 blocks away. `Session::nearest_structure` in the library and `spyglass locate structure --nearest` find the nearest, with `mode` in the JSON. The viewer's Locate tab shows both answers when they differ ([locate.md](docs/specs/locate.md#nearest)).
- **Markers in the viewer:** right-click the map or press M to add a marker, named and coloured. Markers belong to a world (a seed, or a save) and a dimension, and are listed in the new **Markers** tab.
- **Done:** structures, locate pins, search matches and markers can be marked done. Done things are drawn dimmed, and the **Done things** layer hides them.
- **Saved worlds:** the seeds and saves you open are kept with their markers and where you left them, and named ones are saved. **Worlds** in the toolbar reopens them.
- **Sharing a view:** **Share** copies the seed, version, dimension, view and layers as a line of text, and opens a line someone sent you, as does pasting it onto the map. Texts are versioned, so old ones keep opening.
- **Your data in one file:** markers, done places and worlds are kept in `spyglass-data.json` beside the viewer's settings, in a documented format, with export and import in **Settings** ([user-data.md](docs/specs/user-data.md), [ADR 0048](docs/adr/0048-the-viewer-keeps-user-data-in-a-file-of-its-own.md)).
- **Pins in images:** `spyglass render --pin x,z,#rrggbb` and `Layers::pins` in the library draw pins, and the viewer's image export includes your markers as pins ([render.md](docs/specs/render.md#layers)).
- `World::query_structures` in the library: the structures a structure id or `#tag` names in a dimension.
- `display_name`, `World::structure_set` and `World::structure_generates` in the library: an id as a front end shows it, the set whose colour a structure's markers are, and whether it can generate in a dimension.
- A `bundles` folder beside the programs is found by all three, from any working directory, when it exists. The viewer puts its first bundle there unless you choose another place ([bundle-format.md](docs/specs/bundle-format.md#where-bundles-are)).
- **Blocks:** `spyglass blocks` gives the blocks the game generates for one block, a column or a chunk, before structures and features: the terrain with its surface (grass, sand, terracotta bands, eroded badlands pillars, icebergs), ore veins, and the caves and ravines carvers cut (`--no-carvers` leaves them out). Each block says whether a tied biome could change it. `Session::chunk_blocks`, `block_at` and `block_column` in the library. In every supported version, checked block for block against the game ([material.md](docs/specs/material.md), [carvers.md](docs/specs/carvers.md)).
- **Underground map layers:** ore veins, noise caves and the caves and ravines carvers cut, at a chosen height, up close (4 blocks per pixel or less). `spyglass render --veins --caves --carvers`; in the viewer, an **Underground** section in the Layers tab, with **Surface**, **Underground** and **Bottom** height presets. `Layers::veins`, `noise_caves` and `carvers`, `Session::underground_map` and `Session::carving_map` in the library, and `World::level` ([render.md](docs/specs/render.md#underground)).

### Changed

- **Mineshafts are decided exactly:** spyglass lays out a mineshaft's pieces as the game does, so its biome check is at the game's position and `locate`, searches and the map no longer leave mineshafts as candidates (without terrain, badlands mineshafts still can be). `--bounds`, `Session::structure_layout` and the viewer give mineshafts' bounds and pieces too ([structure-placement.md](docs/specs/structure-placement.md#mineshafts)).
- **Chunks generate 3 to 5 times faster in 26.3 and 26.4,** with the same blocks: a chunk's terrain is sampled as one volume, ore veins share their cells' corners, and carvers ask the aquifer only where they carve. Blocks, the underground layers and feature queries all gain.
- Searches whose checks are slow, such as for features, use every thread: the search's batches hold fewer seeds when each takes long.
- Feature queries leave out the unlikely candidates (those that don't place on spyglass's blocks) unless asked: `FeatureOptions::unlikely`, `--all` in the CLI. Without them, chunks where nothing could place are ruled out from the terrain alone ([features.md](docs/specs/features.md#ruling-out)).
- Bundles now include the data generator's block report, `reports/blocks.json`. Extract bundles again to ask about blocks with them ([bundle-format.md](docs/specs/bundle-format.md#layout)).

### Fixed

- The viewer no longer opens a console window beside its own on Windows, and no longer tells you to run `cargo`.
- The viewer fills the window in seconds: coarse tiles are drawn first while the detailed ones are computed, so the edges of the map no longer stay empty for half a minute with biomes at the surface. What isn't drawn yet is hatched.
- The viewer lists only the structures that can generate in the dimension shown.
- The search hit card's note on a "Maybe" constraint no longer has a run of spaces in the middle.
- A structure tag in the viewer's settings, such as `#minecraft:village`, now shows as its structures checked in the structure list, and unchecking them hides their markers. Before, the map marked them, but the list showed them unchecked and couldn't turn them off.
- **The End's void shows:** with relief or biomes at the surface, columns with no blocks are drawn near-black, so the main island and the outer islands stand out from the space between them. The biome map alone can't show it: the game gives the same biome to everything within 1,024 blocks of the origin. This applies to exported images with those layers too ([render.md](docs/specs/render.md#layers)).

## [0.1.0] - 2026-10-29

### Game versions

Checked against the game for 26.2, 26.3, 26.4-snapshot-5, 26.4-snapshot-6.

The first release.

### Added

- **`spyglass locate`:** where the game's `/locate` would find a structure, biome, slime chunk or the world spawn, for any seed. Structure answers are decided with the seed's terrain, so they match the game's, and give exact bounds for villages, bastions and other jigsaw structures with `--bounds`.
- **Your own worlds:** `--world` answers for a save in a known launcher's folder, from the player's position with `--from player`. `spyglass world list` and `spyglass world info` show the saves and anything that makes answers inexact, such as data packs or chunks generated by older versions.
- **All three dimensions:** the Overworld, the Nether and the End, with `--dimension`.
- **`spyglass render`:** a PNG map of a seed or a save, with biomes and the layers you choose: structures, slime chunks, the spawn, highlighted biomes and terrain relief.
- **`spyglass search`:** the seeds that best fit a query of required and preferred constraints on structures, biomes, slime chunks and the spawn, in any dimension, ranked. Searches run on every core, stop on Ctrl-C or after `--time`, and print the best so far.
- **`spyglass heights`:** the generator's terrain heights for one column or a region, before structures, carvers and features: the heights structures are placed by.
- **`--json`** on every command, in a versioned format for scripts ([cli-json.md](docs/specs/cli-json.md)).
- **The viewer** (`spyglass-viewer`): a pannable, zoomable map of a seed or a save, with the same layers, locate, search, a dimension switch and PNG export.
- **The extractor** (`spyglass-extract`): builds the game data spyglass needs from Mojang's server jar, downloaded and checked on your machine. Nothing of Mojang's is shipped with spyglass.
- **The `spyglass` library**, which the command and the viewer are built on.
- Bundles go in a per-user folder by default, or the folder `SPYGLASS_BUNDLES` names ([bundle-format.md](docs/specs/bundle-format.md#where-bundles-are)).
- A warning for a game version this build wasn't checked against. Its answers may differ from the game's ([bundle-format.md](docs/specs/bundle-format.md#checked-versions)).
- `--version` names the build and the game versions it was checked against.

[Unreleased]: https://github.com/farrcraft/spyglass/compare/v0.5.1...HEAD
[0.5.1]: https://github.com/farrcraft/spyglass/compare/v0.5.0...v0.5.1
[0.5.0]: https://github.com/farrcraft/spyglass/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/farrcraft/spyglass/compare/v0.3.1...v0.4.0
[0.3.1]: https://github.com/farrcraft/spyglass/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/farrcraft/spyglass/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/farrcraft/spyglass/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/farrcraft/spyglass/releases/tag/v0.1.0
