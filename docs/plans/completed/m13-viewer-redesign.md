# m13. Viewer redesign

- Status: Completed
- Milestone: [13. Viewer redesign](../../roadmap/completed/13-viewer-redesign.md)
- Started: 2026-10-29
- Completed: 2026-10-29

## Goal

The viewer from the design pass's direction A, the map icons and coarse-first loading. Covers every exit criterion of milestone 13.

The mockups are on the design canvas, [spyglass viewer redesign](https://claude.ai/artifact/2qkM3L7vUxPwK8JNXaFoZu): the "before" board with the eight problems, A exploring, A searching, and the map icons. B (map-first) is there for comparison and isn't being built.

## Approach

All of this is in `spyglass-viewer`, except where noted. The layout is egui's own parts: `Panel::top` for the toolbar, a tab row in the side panel, child UIs placed in the map's corners (`Ui::scope_builder`) for its controls, and a `Window` for settings.

- **Style** (`style.rs`, new): the palette (ground, panel, surface, border, text, muted text, an amber accent), spacing and corner radii, applied once to `egui::Style` and `Visuals`. Widgets and screens use the style's names, never their own colours. egui's own fonts stay (see Notes).
- **Toolbar** (`app/toolbar.rs`, new, taking over `world_section`):
  - The seed field is wide enough for any `i64`, with copy and random buttons, so the grey copy of the seed underneath goes.
  - The version picker, and the dimension as three toggle buttons.
  - "Open save…" as one menu (launcher saves, then a folder), replacing the drop-down and the separate button.
  - An Export menu (image, copy command) and a settings button.
- **Settings window:** the bundles folder, "Look again" and adding a game version (today's `extract_panel`).
- **Side panel tabs:**
  - **Layers:** Biomes (height, at the surface, tied cells, highlighted biomes as chips), Overlays (spawn, slime chunks, relief, each with its disabled reason shown under it rather than only on hover), Structures (a filter and a checkable row per family with its icon and how many are in view, opening to its variants), and "Confirm with terrain" set apart, with its effect in a line under it.
  - **Search:** today's `search_section`, laid out as on the board. The query is highlighted by token kind (keywords, ids, numbers). The options go in a two-column grid, with the seed range and start index folded away. Progress is a bar against the time limit. Results are a table of rank, seed and score.
  - **Locate:** today's `locate_section`, as a tab that shows a count when there are hits.
- **Map overlays** (`app/map.rs`): go to (focused with `G`), the zoom stack showing `1:N`, centre on spawn, a scale bar with a round number of blocks, and a legend of the structures and highlighted biomes marked. For an opened search hit, a card listing its constraints, met or not, and each `within` radius drawn as a dashed circle.
- **Status bar:** `x, y, z`, the chunk, the biome's swatch and name, and "Drawing N tiles" beside the spinner.
- **Icons** (`icons.rs`, new, ADRs [0044](../../adr/0044-the-viewer-draws-its-own-map-icons.md) and [0045](../../adr/0045-viewer-icons-are-svgs.md)): one SVG per structure family in `crates/spyglass-viewer/icons/`, drawn by `egui_extras`.
  - Families are keyed by structure id. Variants share a glyph (the five villages, seven ruined portals, both ocean ruins, both shipwrecks, both mineshafts, the abandoned camps); fortress and bastion get their own.
  - The badge is the set's `structure_colour`, the glyph dark or white for contrast. A candidate is the hollow badge with a lighter tint of the colour.
  - Three sizes by zoom: dots at 1:64 and out, small badges at 1:16 to 1:32, full badges at 1:8 and in.
  - Hover and selection get a white halo and a label. Locate hits become pins, search matches rings.
  - An unknown set gets a badge in its hashed colour with a generic glyph.
- **Coarse-first loading** (`app/map.rs`, `tiles.rs`):
  - Each frame queues, before the visible tiles, the ancestor four scales coarser of each one not yet cached. At 1:4 that's 1:16, which is past `MAX_TERRAIN_SCALE`, so it has 16 times fewer pixels and needs no heights.
  - The existing fallback already scales an ancestor up until the tile is ready. It reaches 8× today, so 4× is covered.
  - Where even the coarse tile isn't ready, a faint diagonal hatch stands in for the flat background, so the gap reads as unfinished. Not a checker: the ties layer already means "tied biomes" by one.
- **Names:** the status bar, tooltips and lists show `spyglass::display_name`, the id in words ("Cherry grove"), since bundles have no language files ([render.md](../../specs/render.md#colours)).
- **Library additions:** `World::structure_set`, the set whose colour a structure's markers are, and `World::structure_generates`, so the lists show only what can generate in the dimension shown.

Both decisions the icons needed are ADRs: the viewer draws them, and exports keep the spec's markers ([0044](../../adr/0044-the-viewer-draws-its-own-map-icons.md)); they're SVGs drawn by `egui_extras` ([0045](../../adr/0045-viewer-icons-are-svgs.md)).

## Tasks

- [x] ADR: whether exports get the icons, and the render.md change that follows.
- [x] ADR: how icons are drawn in the viewer.
- [x] Decide where biome display names come from; add them to the API.
- [x] `style.rs`: palette and spacing, applied at start.
- [x] Toolbar: seed with copy and random, version, dimension toggles, save menu, export menu, settings.
- [x] Settings window: bundles folder, look again, add a game version.
- [x] Side panel tabs; the Layers tab as laid out on the board.
- [x] Search tab: highlighted query, options grid, progress bar, results table.
- [x] Locate tab with a hit count.
- [x] Map overlays: go to, zoom stack, centre on spawn, scale bar, legend.
- [x] Opened search hit: constraint card and `within` radii.
- [x] Status bar: compact readout, biome swatch and name, tiles still drawing.
- [x] Icons: every family in each local bundle (26.2, 26.3, 26.4-snapshot-5 and -6), certain and candidate, three zoom sizes, unknown-set fallback.
- [x] Hover and selection halo and label; locate pins; search match rings.
- [x] Coarse-first loading and the hatch placeholder.
- [x] New screenshots of the before board's states, compared with it by the owner (the canvas's "After" board).
- [x] The End's void drawn wherever heights are, found in review.
- [x] `cargo xtask check`.
- [x] `cargo xtask oracle-test`: passed for 26.2, 26.3, 26.4-snapshot-5, 26.4-snapshot-6 and 26.4-snapshot-6+testpack.
- [x] Docs: architecture.md's viewer row, README screenshots, CHANGELOG.

## Verification

- Unit tests:
  - The scale bar picks a round length for every zoom level.
  - Every structure id in the oracle's bundle maps to a family (ignored, in `cargo xtask oracle-test`), and unknown ids fall back.
  - A view's coarse tiles contain each of its tiles exactly once.
  - Badges' glyphs contrast with them; markers shrink with zoom and pick by their own reach.
  - `display_name`, and `World::structure_set` and `structure_generates` on the real Nether (in `each_dimension_opens`).
- `cargo xtask check`.
- By hand: the before board's states, captured again (launch, a hovered marker, the map's menu, Search open), and a cold start at 1:4 with biomes at the surface.

## Durable outputs

- [x] The two ADRs.
- [x] [render.md](../../specs/render.md#layers): the viewer draws its own icons at the markers' pixels; `display_name` under Colours.
- [x] [architecture.md](../../architecture.md): the viewer's row.

## Notes

- 2026-10-29: completed. The owner checked the new screenshots against the before board.
- 2026-10-29: in review, the End looked wrong: a solid disc to 1,024 blocks with islands right after it. The biomes were right (the game gives `the_end` to every column that close) and so were the heights; nothing drew the void. [render.md](../../specs/render.md#layers) gained a Void step: wherever heights are drawn, a column with no blocks is `#0b0b12`, in exports too. Its test needs a fixture without the Overworld's lava below y −54, which would fill an empty column.
- 2026-10-29: the first `oracle-test` run failed only on the test-pack bundle: its `spyglass_test:village` got the generic icon, which is the intended fallback for another namespace. The family test now checks only `minecraft:` structures.
- 2026-10-29: with coarse-first loading, a cold start at 1:4 with biomes at the surface fills the window in about 5 seconds; the detailed tiles follow. While a search runs, it takes every core but one, so tiles and markers wait until it ends. That's milestone 14's to look at.
- 2026-10-29: egui's default fonts have no ★, ⏵ or ⌖, and draw ✔, 📋 and 🎲 from the colour emoji font, which ignores the label's colour. Controls use words ("Copy", "Random", "Settings", "Met"/"Maybe"/"No", "18 kinds") or shapes the viewer draws itself (the spawn button). Bundling Inter and JetBrains Mono was dropped: egui's fonts read well at these sizes, and it saves two font files and their licences.
- 2026-10-29: the structure list groups variants by family (one row for the 18 abandoned camps), with a checkbox that's partly ticked when only some variants are chosen. The legend lists families too.
- 2026-10-29: the structure sets are every dimension's, so the Nether listed Overworld structures, as it did before. `World::structure_generates` filters the lists, the legend and the locate menu.
- 2026-10-29: the "missing tile" strips seen while making the before board aren't a rendering bug. Temporary logging showed every undrawn tile had no cache entry while the pool was busy, and none was dropped once computed. The outermost tiles are queued last, and with biomes at the surface every tile at 1:4 needs a height map, so a full window took about 20 to 40 seconds on the dev machine, depending on the run. Until then those tiles are flat `#202020`, which reads as a hole, and the strips move as the panel's width changes which tiles are in view. Coarse-first loading and the hatch placeholder are the fix.
