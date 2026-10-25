# 05. Viewer MVP

- Status: Planned
- Depends on: [03](03-structure-placement.md), [04](04-world-saves.md)
- Plans: none yet

## Goal

A native app to preview a seed or a world save on a map, rendering exactly the things you choose.

## Scope

- `spyglass-viewer` on `eframe` (egui + wgpu), built only on the public API.
- Open a seed (with version selection from locally extracted bundles) or a world save.
- Tiled, on-demand biome rendering with pan and zoom.
- **Layer selection:** pick which structure types to show, toggle slime chunks and spawn, and highlight chosen biomes while dimming the rest.
- Coordinates under the cursor, and a "go to" box.
- Locate from the map: click a point, find the nearest structure of a type.
- Image export of the current view, also available from the CLI (`spyglass render`) for scripts.

## Exit criteria

- [ ] Panning and zooming stay responsive while tiles render in the background.
- [ ] Overlays match the output of the public API exactly.
- [ ] `spyglass render` produces the same image as the viewer for the same view and layers.
- [ ] Runs on Windows, macOS and Linux.
- [ ] The viewer explains what to do when no bundle has been extracted yet.
