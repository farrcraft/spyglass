# m18. Underground map layers

- Status: Draft
- Milestone: [18. Surface rules, ore veins, caves and carvers](../roadmap/18-surface-veins-and-carvers.md)
- Started: YYYY-MM-DD
- Completed: YYYY-MM-DD

## Goal

Map layers that show what [m18-material-rule](m18-material-rule.md) and [m18-carvers](m18-carvers.md) compute: ore veins, noise caves, carver caves and ravines, at a height the user picks. They're in the library's renderer, so the viewer, `spyglass render` and exports draw the same pixels. It meets the milestone's second and third exit criteria.

## Approach

**What each layer shows**, at one y, for each pixel's block (the same corner sample as biomes, [render.md](../specs/render.md#map-areas)):

- **Ore veins:** copper or iron vein ore and raw-ore blocks, in two colours, and the vein's filler stone faintly. From the material rule's ore-vein rule (26.3+), or the column (26.2).
- **Noise caves:** air or fluid in the column (the density graph's caves, before carvers).
- **Carver caves and ravines:** positions the carvers carved, in two colours by carver type (cave or canyon), over the noise caves.

Each layer is a tint over the biome map, like slime chunks, so the biomes stay readable. With the biome layer off, the background is grey, as for relief.

**The height.** The layers use the area's `y`, like biomes. The viewer's Layers tab gains presets beside the height slider:

- **Surface:** the terrain height at the view's centre, minus 1.
- **Underground:** y 0.
- **Bottom:** the level's minimum y plus 8.

The slider stays the exact control. `spyglass render` takes `--y` as now.

**Cost and scale.** A pixel needs its chunk's blocks at one y. At scale 1, every chunk in view is computed whole. At larger scales, only one column in s² is sampled, but carvers need the chunk's mask whatever the scale. The plan measures the cost per 128 × 128 tile at scales 1, 2, 4, 8 and 16 with `cargo xtask bench`, then sets `MAX_UNDERGROUND_SCALE` where a tile takes at most about as long as relief at scale 4. Past that, the layers are off with a hint, as terrain layers are. The viewer keeps chunk results in a cache shared across y, so moving the slider doesn't recompute carvers.

**Tiling.** As every layer: a pixel depends only on its own block, so a view equals its tiles. The render spec's tiling test (`a_view_equals_its_tiles`) gains the new layers, on fixtures.

## Tasks

- [ ] **Library:** `Layers::veins`, `noise_caves`, `carvers`, and their colours and drawing order in render.md. `MAX_UNDERGROUND_SCALE` from the bench. Errors where a layer isn't drawn (scale, and the dimensions without veins).
- [ ] **Tiling tests** on fixtures, extending `a_view_equals_its_tiles`, and exact-pixel tests for each layer's colours.
- [ ] **CLI:** `spyglass render --veins --caves --carvers`, cli-json.md, and the export command's flags in the viewer.
- [ ] **Viewer:** the layer toggles, the height presets, a chunk cache across y, the hint past the largest scale, and the shared view text's `l` names (`veins`, `caves`, `carvers`). The text's format stays 1, since only names are added ([user-data.md](../specs/user-data.md#compatibility)).
- [ ] **Cost:** `cargo xtask bench` lines per layer at each scale; the chosen limit and its times here. Check the viewer by eye with `run-viewer`: it stays responsive while the layers fill (pan and zoom), and the search still yields to the map ([ADR 0046](../adr/0046-the-viewers-search-yields-to-the-map.md)).
- [ ] **Docs:** render.md, getting-started (the layers and presets), the changelog.
- [ ] `cargo xtask check` and `cargo xtask oracle-test`. Complete the plan, and the milestone once all four plans are complete.

## Verification

- A view equals its tiles with every new layer on (exit criterion 2).
- Recorded bench times, and the viewer checked by eye while the layers draw (exit criterion 3).
- On the real bundle, in ignored tests, a few pixels per layer agree with `block_at` at the same block.

## Durable outputs

- [ ] render.md: the underground layers, their colours, order and limits.
- [ ] user-data.md: the new layer names in the view text.
- [ ] cli-json.md: the new `render` layers.

## Notes
