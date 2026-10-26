# 05. Viewer MVP

- Status: In progress
- Depends on: [03](completed/03-structure-placement.md), [04](completed/04-world-saves.md)
- Plans: [m05-cleanup](../plans/completed/m05-cleanup.md) (completed), [m05-viewer](../plans/m05-viewer.md)

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
- [ ] Runs on Windows and Linux. macOS is best effort: built and tried when a Mac is available, not guaranteed.
- [ ] The viewer explains what to do when no bundle has been extracted yet.
