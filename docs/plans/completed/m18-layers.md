# m18. Underground map layers

- Status: Completed
- Milestone: [18. Surface rules, ore veins, caves and carvers](../../roadmap/completed/18-surface-veins-and-carvers.md)
- Started: 2026-10-30
- Completed: 2026-10-30

## Goal

Map layers that show what [m18-material-rule](m18-material-rule.md) and [m18-carvers](m18-carvers.md) compute: ore veins, noise caves, carver caves and ravines, at a height the user picks. They're in the library's renderer, so the viewer, `spyglass render` and exports draw the same pixels. It meets the milestone's second and third exit criteria.

## Approach

**What each layer shows**, at one y, for each pixel's block (the same corner sample as biomes, [render.md](../../specs/render.md#map-areas)):

- **Ore veins:** copper or iron vein ore and raw-ore blocks, in two colours, and the vein's filler stone faintly. From the material rule's ore-vein rule (26.3+), or the column (26.2).
- **Noise caves:** air or fluid in the column (the density graph's caves, before carvers).
- **Carver caves and ravines:** positions the carvers carved, in two colours by carver type (cave or canyon), over the noise caves.

Each layer is a tint over the biome map, like slime chunks, so the biomes stay readable. With the biome layer off, the background is grey, as for relief.

**The height.** The layers use the area's `y`, like biomes. The viewer's Layers tab gains presets beside the height slider:

- **Surface:** the terrain height at the view's centre, minus 1.
- **Underground:** y 0.
- **Bottom:** the level's minimum y plus 8.

The slider stays the exact control. `spyglass render` takes `--y` as now.

**Cost and scale.** A whole chunk takes about 42 ms, so a 128 × 128 tile of whole chunks would take seconds even at scale 1. Instead each pixel is sampled at its corner block alone (see the notes):

- **Noise caves and 26.2's veins:** `Terrain::probes_on`, the noise fill's block at one y in each column, scanned from the top down to y, batched by chunk as `base_heights_on` is. It also says whether a solid block is above.
- **Veins from 26.3:** the material rule followed at the position alone (`BoundMaterial::vein_at`). The rule stops at the first condition that needs the column. Vanilla's veins come right after the bedrock floor, so the answer is exact.
- **Carvers:** each chunk's carving mask, split by the kind of the first carver to carve each position (`ChunkGenerator::carvings`), with no material rule and no aquifer. A `CarvingMap` holds an area's carved runs at every height, so the viewer keeps one per tile and doesn't carve again when the height changes.

The plan measures the cost per 128 × 128 tile at scales 1, 2, 4, 8 and 16 with `cargo xtask bench`, then sets `MAX_UNDERGROUND_SCALE` where a tile takes at most about as long as relief at scale 4. Past that, the layers are off with a hint, as terrain layers are.

**Tiling.** As every layer: a pixel depends only on its own block, so a view equals its tiles. The render spec's tiling test (`a_view_equals_its_tiles`) gains the new layers, on fixtures.

## Tasks

- [x] **Library:** `Layers::veins`, `noise_caves`, `carvers`, and their colours and drawing order in render.md. `MAX_UNDERGROUND_SCALE` from the bench. Errors where a layer isn't drawn (scale, and the dimensions without veins).
- [x] **Tiling tests** on fixtures, extending `a_view_equals_its_tiles`, and exact-pixel tests for each layer's colours.
- [x] **CLI:** `spyglass render --veins --caves --carvers`, cli-json.md, and the export command's flags in the viewer.
- [x] **Viewer:** the layer toggles, the height presets, a chunk cache across y, the hint past the largest scale, and the shared view text's `l` names (`veins`, `caves`, `carvers`). The text's format stays 1, since only names are added ([user-data.md](../../specs/user-data.md#compatibility)).
- [x] **Cost:** `cargo xtask bench` lines per layer at each scale; the chosen limit and its times here. Check the viewer by eye with `run-viewer`: it stays responsive while the layers fill (pan and zoom), and the search still yields to the map ([ADR 0046](../../adr/0046-the-viewers-search-yields-to-the-map.md)).
- [x] **Docs:** render.md, getting-started (the layers and presets), the changelog.
- [x] `cargo xtask check` and `cargo xtask oracle-test`. Complete the plan, and the milestone once all four plans are complete.

## Verification

- A view equals its tiles with every new layer on (exit criterion 2).
- Recorded bench times, and the viewer checked by eye while the layers draw (exit criterion 3).
- On the real bundle, in ignored tests, a few pixels per layer agree with `block_at` at the same block.

## Durable outputs

- [x] render.md: the underground layers, their colours, order and limits.
- [x] user-data.md: the new layer names in the view text.
- [x] cli-json.md: the new `render` layers.

## Notes

- 2026-10-30: **Sampling, not chunks.** A chunk's blocks take 42 ms in 26.3, so whole chunks would put a 128 × 128 tile at about 2.7 s at scale 1, and far more beyond. Each layer is sampled at its pixel's block instead. The noise fill comes from a probe scanned down to y. Veins come from the rule at the position, which is exact because vanilla's vein rules come straight after the bedrock floor (26.3, 26.4-snapshot-5 and -6 alike). Carvers come from the carving mask by kind, without the aquifer or the rule. `spyglass-blocks/tests/layers.rs` checks all three against whole chunks in every version.
- 2026-10-30: **Cost** (`cargo xtask bench --only "128 x 128"`, 26.3, `dist`, one thread, y 0, run with the limit raised to 16 to see past it). Relief at 1:4 takes 1,247 ms.

  | Scale | Veins | Noise caves | Carvers | All three | Carvers, kept map |
  | ----- | ----- | ----------- | ------- | --------- | ----------------- |
  | 1:1   | 211   | 189         | 78      | 278       | 0.08              |
  | 1:2   | 1,169 | 1,161       | 469     | 1,465     | 0.10              |
  | 1:4   | 1,424 | 1,443       | 1,030   | 2,353     | 0.14              |
  | 1:8   | 1,554 | 1,535       | 3,472   | 4,888     | 0.15              |
  | 1:16  | 1,900 | 1,884       | 14,528  | 19,369    | 0.20              |

  `MAX_UNDERGROUND_SCALE` is 4: each layer alone costs about what relief does there, and it matches the terrain layers' limit, so the viewer's hints agree. All three together cost about twice relief at 1:4. Carving grows quickly past 1:4, where each pixel's chunk is carved for one column. From 1:2, the noise fill's probes cost most, since each column is sampled on its own.

- 2026-10-30: **By eye** (`run-viewer`, seed 12345, 26.3, every layer on): at 1:1, y 0, the view filled in under a second. Zooming out to 1:2 at y −40, the map moved at once while "Drawing, 24 left" counted down, and it filled in about 12 s. At 1:8 the layers are off with the hint. Surface gave y 62 (the sea's top at the centre), Bottom −56. Veins at alpha 96 filler and a pale iron barely showed, so filler is now alpha 128 and iron `#b8c4ff`. A seed search running beside them is in the next note.
- 2026-10-30: `cargo xtask check` passes, and `cargo xtask oracle-test` passes for 26.2, 26.3, 26.4-snapshot-5, 26.4-snapshot-6 and the test pack. Milestone 18's exit criteria are met.
- 2026-10-30: **Beside a search** (`run-viewer` with `SPYGLASS_VIEWER_TIMING=1`, seed 12345, 26.3, 1:4, y 0, every layer on). Filling the window at a new place took 26.5 s and 19.2 s alone. Beside the viewer's own seed search, two other places took 22.2 s and 22.9 s. The search went on at about 2,400 seeds a second while the map filled, and about 4,300 once it was idle. So the search yields to the map ([ADR 0046](../../adr/0046-the-viewers-search-yields-to-the-map.md)): the underground layers are computed in the tile pool's jobs, which hold the search to one thread.
