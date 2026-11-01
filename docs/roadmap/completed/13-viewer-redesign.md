# 13. Viewer redesign

- Status: Completed
- Depends on: [12](12-first-run.md)
- Plans: [m13-viewer-redesign](../../plans/completed/m13-viewer-redesign.md)

## Goal

The viewer reads as one designed product rather than a stack of controls: the world you're looking at is always in view, the panel is split by task, the map carries its own controls, points of interest are recognisable at a glance, and a map that's still drawing looks like it's drawing.

## Scope

- **Layout:** a toolbar for the world (seed, version, dimension, save, export, settings), a side panel with Layers, Search and Locate tabs, and the bundles folder and adding versions moved behind settings.
- **The map's own controls:** go to, zoom with the scale shown as `1:N`, centre on spawn, a scale bar and a legend of what's marked.
- **Status bar:** position, chunk and biome in a compact readout, and drawing progress in words.
- **Search tab:** the query, its options, progress and the best seeds as a ranked table, with the opened hit's constraints shown over the map.
- **Points of interest:** an icon per structure family on a badge in its set's colour, certain and candidate told apart, sized by zoom; pins for locate hits, rings for search matches, a halo and label for the hovered or selected marker.
- **Loading:** every part of the map shows something soon after it comes into view, and empty areas look unfinished rather than broken.
- **A shared style:** colours, spacing, type and widget looks set in one place.

Not in scope:

- A light theme.
- The map-first layout with floating panels, which the design pass considered and set aside.
- New worldgen. If the viewer needs a new answer (a biome's display name, say), it comes from the API.

## Exit criteria

- [x] Each of the eight problems on the design pass's "before" board is fixed, checked against new screenshots of the same states.
- [x] Every structure set in each supported version's bundle has an icon. A set without one, from a newer version or a pack, still gets a badge in its hashed colour.
- [x] Opening a seed at 1:4 with biomes at the surface, no part of the map stays flat `#202020` once its coarse tile is drawn, and the status bar says how many tiles are still drawing.
- [x] `cargo xtask check` passes.
