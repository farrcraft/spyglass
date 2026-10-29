# m13. Viewer redesign

- Status: Draft
- Milestone: [13. Viewer redesign](../roadmap/13-viewer-redesign.md)
- Started: 2026-10-29
- Completed:

## Goal

The viewer from the design pass's direction A, the map icons and coarse-first loading. Covers every exit criterion of milestone 13.

The mockups are on the design canvas, [spyglass viewer redesign](https://claude.ai/artifact/2qkM3L7vUxPwK8JNXaFoZu): the "before" board with the eight problems, A exploring, A searching, and the map icons. B (map-first) is there for comparison and isn't being built.

## Approach

All of this is in `spyglass-viewer`, except where noted. The layout is egui's own parts: `Panel::top` for the toolbar, a tab row of selectable labels in the side panel, `egui::Area`s anchored to the map's corners for its controls, and a `Window` for settings.

- **Style** (`style.rs`, new): the palette (ground, panel, surface, border, text, muted text, an amber accent), spacing and corner radii, applied once to `egui::Style` and `Visuals`. Inter for text and JetBrains Mono for numbers, both SIL OFL, bundled with `include_bytes!` and named in the third-party notices. Widgets and screens use the style's names, never their own colours.
- **Toolbar** (`app/toolbar.rs`, new, taking over `world_section`):
  - The seed field is wide enough for any `i64`, with copy and random buttons, so the grey copy of the seed underneath goes.
  - The version picker, and the dimension as three toggle buttons.
  - "Open save…" as one menu (launcher saves, then a folder), replacing the drop-down and the separate button.
  - An Export menu (image, copy command) and a settings button.
- **Settings window:** the bundles folder, "Look again" and adding a game version (today's `extract_panel`).
- **Side panel tabs:**
  - **Layers:** Biomes (height, at the surface, tied cells, highlighted biomes as chips), Overlays (spawn, slime chunks, relief, each with its disabled reason shown under it rather than only on hover), Structures (a filter and checkable rows with their icon and how many are in view), and "Confirm with terrain" set apart, with its effect in a line under it.
  - **Search:** today's `search_section`, laid out as on the board. The query is highlighted by token kind (keywords, ids, numbers). The options go in a two-column grid, with the seed range and start index folded away. Progress is a bar against the time limit. Results are a table of rank, seed and score.
  - **Locate:** today's `locate_section`, as a tab that shows a count when there are hits.
- **Map overlays** (`app/map.rs`): go to (focused with `G`), the zoom stack showing `1:N`, centre on spawn, a scale bar with a round number of blocks, and a legend of the structures and highlighted biomes marked. For an opened search hit, a card listing its constraints, met or not, and each `within` radius drawn as a dashed circle.
- **Status bar:** `x, y, z`, the chunk, the biome's swatch and name, and "Drawing N tiles" beside the spinner.
- **Icons** (`icons.rs`, new): one glyph per structure family, drawn as vector paths with `Painter` so nothing new is needed to render them.
  - Families are keyed by structure id. Variants share a glyph (the five villages, seven ruined portals, both ocean ruins, both shipwrecks, both mineshafts, the abandoned camps); fortress and bastion get their own.
  - The badge is the set's `structure_colour`, the glyph dark or white for contrast. A candidate is the hollow badge with a lighter tint of the colour.
  - Three sizes by zoom: dots at 1:64 and out, small badges at 1:16 to 1:32, full badges at 1:8 and in.
  - Hover and selection get a white halo and a label. Locate hits become pins, search matches rings.
  - An unknown set gets a badge in its hashed colour with a generic glyph.
- **Coarse-first loading** (`app/map.rs`, `tiles.rs`):
  - Each frame queues, before the visible tiles, the ancestor four scales coarser of each one not yet cached. At 1:4 that's 1:16, which is past `MAX_TERRAIN_SCALE`, so it has 16 times fewer pixels and needs no heights.
  - The existing fallback already scales an ancestor up until the tile is ready. It reaches 8× today, so 4× is covered.
  - Where even the coarse tile isn't ready, a faint diagonal hatch stands in for the flat background, so the gap reads as unfinished. Not a checker: the ties layer already means "tied biomes" by one.
- **Biome names:** the status bar and tooltips show a display name in place of the raw id. Whether that comes from the game's language files in the bundle, or a title-cased id, is decided in the first task, and belongs in the library either way.

Two decisions need an ADR before the icon work:

1. **Do exports get the icons?** [render.md](../specs/render.md#layers) has the viewer draw the same 9 × 9 markers as `Image::draw_features`. Either the library draws pixel-exact versions of the icons, so exports match and the spec defines each one, or the viewer adds its icons over the spec's markers, and the spec says the viewer may decorate. The first is more work. It keeps one picture of a map everywhere.
2. **How the icons are drawn:** hand-written `Painter` paths (nothing new, but each glyph is code) or SVGs through `egui_extras`' resvg loader (easy to edit, but a dependency, a `deny.toml` licence check and a bigger binary).

## Tasks

- [ ] ADR: whether exports get the icons, and the render.md change that follows.
- [ ] ADR: how icons are drawn in the viewer, and the bundled fonts.
- [ ] Decide where biome display names come from; add them to the API.
- [ ] `style.rs`: palette, spacing, fonts, applied at start.
- [ ] Toolbar: seed with copy and random, version, dimension toggles, save menu, export menu, settings.
- [ ] Settings window: bundles folder, look again, add a game version.
- [ ] Side panel tabs; the Layers tab as laid out on the board.
- [ ] Search tab: highlighted query, options grid, progress bar, results table.
- [ ] Locate tab with a hit count.
- [ ] Map overlays: go to, zoom stack, centre on spawn, scale bar, legend.
- [ ] Opened search hit: constraint card and `within` radii.
- [ ] Status bar: compact readout, biome swatch and name, tiles still drawing.
- [ ] Icons: every family in 26.3, certain and candidate, three zoom sizes, unknown-set fallback.
- [ ] Hover and selection halo and label; locate pins; search match rings.
- [ ] Coarse-first loading and the hatch placeholder.
- [ ] New screenshots of the before board's states, compared with it.
- [ ] Docs: architecture.md's viewer row, README screenshots, CHANGELOG.

## Verification

- Unit tests:
  - The scale bar picks a round length for every zoom level.
  - Every structure id in each bundle maps to a family, and unknown ids fall back.
  - The coarse ancestor of a tile contains it.
  - Coarse jobs are queued before fine ones.
- If exports get the icons, render.md's tests cover them: a view equals itself assembled from tiles with icons on.
- `cargo xtask check`.
- By hand: the before board's states, captured again (launch, a hovered marker, the map's menu, Search open), and a cold start at 1:4 with biomes at the surface.

## Durable outputs

- [ ] The two ADRs.
- [ ] [render.md](../specs/render.md#layers), if exports get the icons or the viewer's markers stop matching the library's.
- [ ] [architecture.md](../architecture.md): the viewer's row.

## Notes

- 2026-10-29: the "missing tile" strips seen while making the before board aren't a rendering bug. Temporary logging showed every undrawn tile had no cache entry while the pool was busy, and none was dropped once computed. The outermost tiles are queued last, and with biomes at the surface every tile at 1:4 needs a height map, so a full window took about 20 to 40 seconds on the dev machine, depending on the run. Until then those tiles are flat `#202020`, which reads as a hole, and the strips move as the panel's width changes which tiles are in view. Coarse-first loading and the hatch placeholder are the fix.
