# m05. Map rendering and the viewer

- Status: In progress
- Milestone: [05. Viewer MVP](../roadmap/05-viewer-mvp.md)
- Started: 2026-10-25
- Completed: —

## Goal

A native app that shows a seed or a world save on a pannable, zoomable map, with the layers the user picks, plus `spyglass render` to make the same image from scripts. This is the milestone's second plan, after [m05-cleanup](completed/m05-cleanup.md). It covers every exit criterion:

- Panning and zooming stay responsive while tiles render in the background: phases 3 and 4.
- Overlays match the output of the public API exactly: phases 1 and 4.
- `spyglass render` produces the same image as the viewer for the same view and layers: phases 1, 2 and 6.
- Runs on Windows and Linux, macOS best effort: phase 6.
- The viewer explains what to do when no bundle has been extracted yet: phase 3.

## Approach

### Rendering belongs to the library

Library first ([architecture.md](../architecture.md#front-ends)): an image is a worldgen result, so the pixels come from the `spyglass` crate, not from a front end. Both the viewer's export and `spyglass render` call one function, so the third exit criterion holds by construction, and the tests only need to show that the viewer and the CLI build the same request. The viewer draws its on-screen map from the same library tiles; only the on-screen overlay markers are drawn by egui, from the same API data and the same style table.

A new `render` module in `spyglass`, roughly:

```rust
/// A rectangle of the map: which blocks, at what scale and height.
pub struct MapArea {
    pub min: [i32; 2],   // block x, z of the top-left pixel; a multiple of `scale`
    pub size: [u32; 2],  // pixels
    pub scale: u32,      // blocks per pixel: a power of two, 1 to 256
    pub y: i32,          // the block height biomes are sampled at
}

/// What to draw on top of (or instead of) biomes.
pub struct Layers {
    pub biomes: bool,
    pub highlight: Vec<BiomeQuery>,     // empty: no dimming
    pub structures: Vec<StructureQuery>,
    pub slime_chunks: bool,
    pub spawn: bool,
    pub ties: bool,                     // mark tied biome cells
}

impl Session<'_> {
    pub fn biome_map(&self, area: &MapArea) -> BiomeMap;          // one biome per pixel, with ties
    pub fn map_features(&self, area: &MapArea, layers: &Layers) -> Result<MapFeatures>; // starts, slime chunks, spawn
    pub fn render(&self, area: &MapArea, layers: &Layers) -> Result<Image>;              // RGBA8
}
impl BiomeMap { pub fn colorize(&self, style: &Style, highlight: &[BiomeQuery]) -> Image; }
impl Image { pub fn write_png(&self, w: impl Write) -> Result<()>; }
```

The names will change during the work. What must hold:

- **A fixed sampling grid.** Pixel `(i, j)` samples the quart containing block `(min.x + i·scale, y, min.z + j·scale)`, through `Biomes::biomes_in` so each column's climate is computed once. The grid is aligned to the world, not the view, so a view rendered whole equals the same view assembled from tiles, and the viewer can cache tiles across pans. Scales below 4 repeat each quart over several pixels. Zoomed out, pixels skip quarts: each pixel is still the game's exact biome at its sample point, and the spec says so.
- **Biome indices, then colour.** `BiomeMap` keeps biome indices (into `World::biomes`) and tie flags, not colours, so changing the highlight or palette recolours cached tiles without sampling again.
- **Overlays come from the existing queries.** Structure starts from `Session::structures_in` (certain and candidate starts drawn differently), slime chunks from `Session::slime_chunks`, the spawn from `Session::spawn_origin`. `map_features` adds no worldgen logic of its own. Markers are simple shapes from a style table in the library (a colour and shape per structure set, filled for certain starts, hollow for candidates), so the CLI rasterizes them and the viewer paints them with the same table.
- **Deterministic output.** Same session, area and layers give the same bytes on every platform: no floating-point blending that could differ, integer alpha only.

The biome palette is spyglass's own presentation, not a game parameter, so it isn't bundle data. Recommended: a built-in table keyed by biome id, written for spyglass (not copied from Amidst, which is GPL), with a fallback for ids it doesn't know (a later version's new biomes) derived from the bundle's biome JSON colours. An ADR records this.

PNG encoding uses the `png` crate in `spyglass`, so both front ends write identical files. Its licences are already allowed.

### `spyglass render`

A new CLI command over `Session::render`:

```sh
spyglass render --seed spyglass --centre 0,0 --size 1024x768 --scale 16 --y 64 \
    --structures '#minecraft:village,minecraft:mansion' --slime --spawn \
    --highlight minecraft:cherry_grove -o map.png
```

It reuses `Common` (`--seed` or `--world`, `--game-version`, `--bundle`, `--bundles`). `--centre` or `--min` place the view; `--centre` snaps to the scale's grid exactly as the viewer does, with the snapping rule in the library (`MapArea::centred_on`). `--json` prints the area actually rendered and the features drawn ([specs/cli-json.md](../specs/cli-json.md)).

### The viewer

`spyglass-viewer` on `eframe` with the `wgpu` backend, plus `rfd` for native file dialogs. It depends only on `spyglass`.

- **Threads.** `Session<'w>` borrows its `World`, so the viewer opens a world on a host thread that spawns scoped worker threads (`std::thread::scope`) borrowing it. They live until another bundle or save is opened. Changing the seed makes a new `Session` (about 0.1 ms) shared by the workers. This needs no API change; if it proves awkward, an owned session (`Arc<World>`) goes into the API, with an ADR 0014 follow-up.
- **Tiles.** 256 × 256 pixel tiles keyed by (world, seed, scale, y, tile x, tile z), in an LRU cache of `BiomeMap`s and their textures. Requests carry a generation number; the UI bumps it on every view change, and workers skip stale requests. Visible tiles render nearest the centre first. While a tile renders, the next coarser cached tile is drawn scaled up in its place.
- **Budget.** `cargo xtask bench` measures 65,536 `biome_at` calls at 158 ms, so a full 256² tile is about 150 ms on one core at any scale of 4 or more, and a 1920 × 1080 window (about 40 tiles) fills in well under a second on 8 cores. If `biomes_in` per tile isn't fast enough, fix it in the library, not the viewer.
- **Screens.** No bundle: an explanation and the exact `spyglass-extract` command, a way to choose the bundles folder, and a re-scan. Otherwise a side panel with the source (seed with version picker, or world save), layers, and locate results; the map fills the rest. A status bar shows the block, chunk and biome under the cursor (with ties) and a "go to" box for `x z` or `x y z`.
- **World saves.** A list from `launcher_saves` plus "open folder". `SaveInspector` picks the bundle; its warnings are shown, and answers are marked inexact when `SaveReport::answerable` is false. The map starts on the save's player, or its spawn. With `generate_structures` off, the structure layer is disabled and says why.
- **Locate from the map.** Right-click a point: nearest structure of a chosen type (`locate_structure` from that point), or nearest biome (`locate_biome`). Hits are listed in the side panel with their certainty, marked on the map, and clicking one centres it.
- **Export.** "Export image" renders the current view through `Session::render` and writes a PNG. "Copy command" puts the equivalent `spyglass render` command on the clipboard, which is also how the CLI–viewer equality is tested.
- **Settings.** eframe's persistence keeps the bundles folder, last source and layers between runs.

Viewer logic that isn't drawing (view transforms, tile keys and scheduling, the export request, command building) lives in plain modules with unit tests, apart from the egui code.

Specs and ADRs that govern this: [ADR 0014](../adr/0014-public-api-world-and-session.md) (the public API), [specs/locate.md](../specs/locate.md) (certainty of starts and biomes), [specs/biome-lookup.md](../specs/biome-lookup.md) (quarts and ties), [specs/cli-json.md](../specs/cli-json.md), [ADR 0008](../adr/0008-game-derived-test-values-stay-local.md) (no committed game-derived images).

## Tasks

### Phase 1: render API in `spyglass`

- [x] ADR: map rendering in the public API (where it lives, the sampling grid, `png`): [0017](../adr/0017-map-rendering-in-the-public-api.md).
- [x] ADR: the biome palette (built-in table plus hashed fallback): [0018](../adr/0018-biome-palette.md).
- [x] `docs/specs/render.md`: the sampling grid and its alignment, scales, the default y (64) and that biomes are a slice at one height rather than at the surface, layer semantics, marker styles, tie marking, highlight dimming, determinism.
- [x] `MapArea` with validation (scale a power of two in range, `min` on the grid, size limits) and `centred_on`.
- [x] `Session::biome_map` over a new `Biomes::biome_grid` (strided, one height); `BiomeMap` with indices and ties.
- [x] Palette and `BiomeMap::colorize`, including highlight dimming and tie marking.
- [x] `Session::map_features`: structure starts, slime chunks and spawn within the area, from the existing queries.
- [x] Marker style table and rasterizer; `Session::render`; `Image::write_png`.
- [x] Tests on fixture bundles (`spyglass-oracle` fixtures): whole view equals tiled view; every drawn marker is a `structures_in` / `slime_chunks` / `spawn_origin` result and vice versa; `biome_map` pixels equal `biome_at` at the sample points; bad areas are errors, not panics.
- [x] Bench lines for one 256² tile at scales 1, 4 and 64, and a full render.

### Phase 2: `spyglass render`

- [ ] `render` command and its arguments, reusing `Common`; `--world` centres on the player by default.
- [ ] `--json` output and its section in `cli-json.md`.
- [ ] Integration test: the command's PNG equals `Session::render`'s for the same request, on a fixture bundle.

### Phase 3: viewer shell

- [ ] ADR: the GUI stack (`eframe` with `wgpu`, `rfd`), with licences added to `deny.toml` as needed and the MSRV checked against eframe's.
- [ ] App skeleton: eframe window, side panel, map area, status bar; settings persistence.
- [ ] Bundle discovery with `bundles_in`; the no-bundle screen with the extract command and a folder chooser.
- [ ] Seed input (`parse_seed`) and version picker.
- [ ] World host thread, scoped workers, request queue with generations and centre-first order.
- [ ] Tile cache (LRU) and texture upload; coarser tiles as placeholders.
- [ ] Pan (drag, keys) and zoom (wheel around the cursor, buttons) across scales 1 to 256; height slider for y.
- [ ] Cursor readout (block, chunk, biome with ties) and "go to".
- [ ] Unit tests for view transforms, tile keys and stale-request dropping.

### Phase 4: layers

- [ ] Structure layer: a checklist of the world's structures (and tags), markers from `map_features` per tile, certain and candidate styles, a tooltip with set, structures and position; markers thinned when zoomed far out.
- [ ] Slime chunk layer, shown only at scales where chunks are visible.
- [ ] Spawn marker, with "platform dependent" shown when it applies.
- [ ] Biome highlight: pick biomes, dim the rest by recolouring cached tiles; tied-cell layer.
- [ ] Check that the on-screen markers sit exactly where `render` draws them at the same view.

### Phase 5: locate and world saves

- [ ] Context menu on the map: nearest structure (type picker) and nearest biome from the clicked point; results in the side panel, on the map, clickable.
- [ ] World saves: launcher list, open folder, inspection off the UI thread, warnings shown, start on player or spawn, `generate_structures` handled.

### Phase 6: export, platforms and close-out

- [ ] "Export image" (PNG via `rfd`) and "Copy command".
- [ ] Test: the viewer's export request and copied command, parsed by the CLI's argument parser, give the same `MapArea` and `Layers`.
- [ ] Windows: run manually through the checklist below.
- [ ] Linux: `cargo xtask linux-check --full` builds the viewer; run it on a Linux desktop (or under Xvfb with a software Vulkan driver) through the checklist.
- [ ] macOS: best effort, not required to complete the plan. Nothing platform-specific is written for it beyond what eframe and `rfd` handle; if a Mac is to hand, run the checklist and note the result.
- [ ] Update architecture.md (viewer no longer a placeholder, render in `spyglass`), the README, and getting-started (running the viewer, `spyglass render`).
- [ ] `cargo xtask check` and `cargo xtask oracle-test`; move this plan and the milestone to `completed/`.

## Verification

- Library tests on fixture bundles for tiling invariance, overlay equality with the query results, and sampling (phase 1). Tests with the real 26.3 bundle are `#[ignore]`d and run by `cargo xtask oracle-test`; images derived from game data are never committed (ADR 0008).
- The CLI integration test and the export-equality test (phases 2 and 6).
- `cargo xtask bench` lines for tiles and full renders, recorded in Notes.
- A manual checklist on each platform: start with no bundles and follow the instructions; open a seed and a save; pan and zoom quickly at every scale without stalls (the UI keeps 60 fps while tiles fill); toggle every layer; locate from the map and compare with `spyglass locate`; export, then run the copied command and compare the files byte for byte.

## Durable outputs

- [x] `docs/specs/render.md` (new)
- [ ] `docs/specs/cli-json.md`: `render`
- [x] ADR: map rendering in the public API ([0017](../adr/0017-map-rendering-in-the-public-api.md))
- [x] ADR: biome palette ([0018](../adr/0018-biome-palette.md))
- [ ] ADR: GUI stack
- [ ] `docs/architecture.md`, `README.md`, `docs/getting-started.md`

## Notes

- 2026-10-25: Phase 1 done. `cargo xtask bench` (release, one thread): a 256² `biome_map` tile is 10.6 ms at scale 1 (64 × 64 quarts, repeated), 158 ms at scale 4 and 164 ms at scale 64, matching the plan's estimate. A 1024² render at scale 16 with structures, slime chunks and spawn is 2.48 s, nearly all biome sampling; overlays are negligible. `Session::render` is single-threaded. The viewer spreads tiles over workers. If `spyglass render` of large images feels slow, the library can render in parallel strips (the grid makes strips independent).
- 2026-10-25: The palette's fallback for unknown ids is a hash of the id, not colours from the bundle's biome JSON: those colours are sparse and mostly alike (ADR 0018). Biome sampling uses a new strided `Biomes::biome_grid`, not `biomes_in`, which fills every quart of a box and would sample 256 times more than needed at scale 64.
- 2026-10-25: macOS support is best effort, not guaranteed: Windows and Linux are verified, macOS only when a Mac is available. The milestone's exit criterion says so.
- 2026-10-25: Biomes are drawn on a slice at one y (default 64, set with a slider), not at the terrain surface. Under mountains the slice can differ from the biome on the peaks above. Surface biomes need heights, which come with [milestone 07](../roadmap/07-terrain-and-validity.md). `docs/specs/render.md` says so, so map users know what they're seeing.
