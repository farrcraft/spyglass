# Changelog

What changed in each release of spyglass, for people who use it. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html) as [ADR 0037](docs/adr/0037-versions-and-releases.md) applies it. While the version starts with `0.`, a minor release may change the library's API, the command's options or its `--json` output; a patch release never does. A release that adds a game version is a minor release.

Changes go under **Unreleased** as they're made. `cargo xtask release` turns that section into the release's, and adds the game versions the release was checked against.

## [Unreleased]

### Added

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
- **Blocks:** `spyglass blocks` gives the blocks the game generates for one block, a column or a chunk, before structures and features: the terrain with its surface (grass, sand, terracotta bands, eroded badlands pillars, icebergs), ore veins, and the caves and ravines carvers cut (`--no-carvers` leaves them out). Each block says whether a tied biome could change it. `Session::chunk_blocks`, `block_at` and `block_column` in the library. For 26.2 and 26.3, checked block for block against the game ([material.md](docs/specs/material.md), [carvers.md](docs/specs/carvers.md)).

### Changed

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

[Unreleased]: https://github.com/farrcraft/spyglass/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/farrcraft/spyglass/releases/tag/v0.1.0
