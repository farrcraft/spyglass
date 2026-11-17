# 32. Viewer polish

- Status: Completed
- Depends on: [13](13-viewer-redesign.md), [14](14-viewer-performance.md), [31](31-save-inspector.md)
- Plans: [m32-viewer-polish](../../plans/completed/m32-viewer-polish.md)

## Goal

The viewer says plainly what the current zoom can show, stops work the view no longer needs, and doesn't compute anything twice when the user zooms. The side panel reads as clear sections that can be folded away. Today some layers that need a closer zoom are disabled (slime chunks, generated chunks) while others stay clickable and only add a hint line (underground layers, features, loot). Zooming out drops waiting jobs but lets running ones finish, and a features tile takes seconds. Features are kept per tile at the view's scale, so zooming from 1:2 to 1:1 places every chunk again.

## Scope

- **Layers that need a closer zoom.** One rule for every layer with a zoom limit (underground layers, features, loot, slime chunks, generated chunks, structure bounds): past its limit the control is greyed, its hover text says how far to zoom in ("Zoom to 1:2 or closer"), and its section header says so too. A layer that's already on stays on but paused, and draws again on zooming back in, so the user's choice isn't lost. The limits come from the library's constants (`MAX_FEATURE_SCALE`, `MAX_UNDERGROUND_SCALE`, `MAX_SLIME_SCALE`), not copies.
- **Stopping work on zoom.** Jobs for a layer the new zoom can't show stop, running ones included. The library's map calls take a stop signal, as searches do, and give up within a fraction of a second; the viewer's pool passes it to each running job and raises it when the job is no longer wanted. The same applies to panning far away and to switching layers off.
- **Reuse across zoom levels.**
  - _Points_ (features, loot containers, structure starts): kept per chunk region, independent of the view's scale, so zooming in draws from what's known and computes only what isn't.
  - _Pixels that depend on their block alone_ (biomes at a height, the underground layers): a coarser tile's pixels are a subset of finer tiles' pixels, since each shows its corner block ([render.md](../../specs/render.md#map-areas)). A coarser tile is assembled from finer ones already cached, and a finer tile can start from the coarser one's pixels it shares. Relief and surface biomes reuse too: heights are each pixel's own column, surface biomes the biome at that height, and relief is drawn from the heights and one row beyond the tile's west and north edges, which a neighbouring finer tile holds. The plan measures each layer's saving and keeps the finer-from-coarser start only where it pays. The render spec records which layers reuse and why it's exact.
  - _Further out than a layer's limit_: points already computed can be drawn past the limit, where they were computed, with the area that's covered shaded so an empty place isn't mistaken for one with nothing. Nothing new is computed there. The plan decides whether this is clear enough to ship, or confusing.
- **Memory.** The caches stay bounded. The plan measures what the reuse costs and sets limits from it.
- **The side panel.**
  - Clearer tabs: all five (Layers, Search, Locate, Markers, and the World tab [31](31-save-inspector.md) added while a save is shown), the selected tab unmistakable, and each tab's count (search hits, locate hits, markers, the save's warnings) where it's useful.
  - Clearer sections: top-level sections set apart from the controls inside them, with consistent spacing.
  - Folding: each section can be collapsed and expanded, the World tab's included, and the viewer remembers which ones. A collapsed section's header still says what in it is on ("2 on", "paused: zoom in"), or in the World tab its warnings, so a hidden layer or warning isn't a surprise.
- **Text boxes.** Every single-line text box is padded the same, in a menu or not. Today the boxes in menus (**Filter saves**, **Filter biomes**, the shared view) have their text against the box's top-left corner. Checkboxes in menus have a visible box, as they do in the panel.
- **Checked by eye and by tests.** Screenshots before and after for the guide. Tests for the zoom rule, stopping and reuse. [Viewer timing](14-viewer-performance.md) scenarios for zooming in and out show the reuse paying, and no scenario gets slower.

Not in scope:

- A cache on disk that outlives the viewer.
- New layers, or changes to what a layer shows.

## Exit criteria

- [x] Every layer with a zoom limit is greyed past it, with hover text saying how far to zoom in, and stays on, paused, when it was on.
- [x] Zooming out past a layer's limit stops its running jobs within half a second, measured, and a test checks the stop signal reaches the library's map calls.
- [x] Zooming in from 1:2 to 1:1 with features on places no chunk twice, and the viewer-timing scenarios for zooming are faster than before, with none slower. (Zooming 1:2 to 1:1 with every feature: 97 s before, 0.07 s after. The zooms out and to 1:1 underground compute what they did before but for what's cached; the cold starts are no slower. See the plan's notes.)
- [x] Tiles built from cached ones are identical to tiles computed directly (a test per layer that reuses).
- [x] The side panel's sections fold, remember their state across a restart, and say what's on when folded.
- [x] Every single-line text box is padded the same, in a menu or not, and checkboxes in menus have a visible box.
- [x] The user guide's [viewer](../../guide/src/viewer.md) page shows the new panel and describes zoom limits and folding.
