# 04. Viewer MVP

- Status: Planned
- Depends on: [03](03-structure-placement.md)
- Plans: none yet

## Goal

A native app that shows a seed's biome map with structures on top.

## Scope

- `spyglass-viewer` on `eframe` (egui + wgpu).
- Seed input and version selection from locally extracted bundles.
- Tiled, on-demand biome rendering with pan and zoom.
- Structure, slime chunk and spawn point overlays that can be toggled.
- Coordinates under the cursor.

## Exit criteria

- [ ] Panning and zooming stay responsive while tiles render in the background.
- [ ] Overlays match the output of `spyglass-structure` exactly.
- [ ] Runs on Windows, macOS and Linux.
- [ ] The viewer explains what to do when no bundle has been extracted yet.
