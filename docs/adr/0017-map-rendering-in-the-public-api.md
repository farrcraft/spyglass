# 0017. Map rendering in the public API

- Status: Accepted
- Date: 2026-10-25

## Context

Milestone 05 adds a map: the viewer shows a seed's biomes with structure, slime-chunk and spawn overlays, exports the view as an image, and `spyglass render` makes the same image from scripts. The milestone requires that the CLI's image equals the viewer's for the same view and layers, and that overlays match the public API's answers exactly.

If each front end turned API answers into pixels itself, there would be two rasterizers to keep in step, and the equality could only be checked by comparing their output. It would also break the library-first rule ([architecture.md](../architecture.md#front-ends)): scripts could do less than the GUI.

The viewer renders the map in tiles, in the background, and caches them across pans and zooms. A view rendered in one piece must equal the same view assembled from tiles, or the export would differ from the screen. Zoomed out, a pixel covers many quarts, so which quart a pixel shows has to be a fixed rule, not an accident of where the view starts.

## Decision

We will produce map images in the `spyglass` crate, in a `render` module. Both front ends call `Session::render` (or its parts) and only display or save the result.

- **A world-aligned sampling grid.** A `MapArea` is a block rectangle, a scale (blocks per pixel, a power of two from 1 to 256) and a height. Its corner must be a multiple of the scale. Pixel `(i, j)` shows the biome of the quart containing block `(min.x + i·scale, y, min.z + j·scale)`. Any two areas at one scale and height agree on every pixel they share.
- **Biomes before colours.** `Session::biome_map` returns biome indices and tie sets per pixel. Colouring is a separate step, so the viewer can recolour cached tiles when highlighting changes.
- **Overlays from the existing queries.** Structure markers come from `Session::structures_in`, slime chunks from `Session::slime_chunks`, the spawn from `Session::spawn_origin`. Rendering adds no worldgen logic. A tile includes the markers that overlap it, not only those whose position is inside it, and markers are drawn in one global order, so they line up across tile edges.
- **Deterministic pixels.** Compositing uses integer arithmetic only, so the same request gives the same bytes on every platform.
- **PNG in the library**, with the [`png`](https://crates.io/crates/png) crate, so both front ends write identical files.

The details are in [specs/render.md](../specs/render.md).

## Consequences

- The CLI–viewer equality holds by construction. Its tests only need to show that both front ends build the same `MapArea` and layers.
- The viewer still draws its on-screen markers with egui, for crisp scaling and hover. They use the library's marker positions and style table, but they aren't the exported pixels.
- `spyglass` gains a dependency on `png` (with `flate2`, already in the tree through `spyglass-data`).
- Zoomed out, pixels skip quarts. A small biome between sample points doesn't show, which other map tools share. The spec says so.
- Marker shapes and sizes are part of the image, so changing them changes every render. That's acceptable; renders aren't a stable format.

## Alternatives considered

- **Each front end rasterizes API answers:** two implementations to keep identical, and an equality the code can't guarantee.
- **A separate `spyglass-render` crate:** it would sit above `spyglass` and depend on `Session`, or below it and need the structure and biome crates directly. Rendering is small, and the architecture already puts it in `spyglass`.
- **Sampling each pixel's centre, or averaging quarts:** centre sampling makes the grid depend on the scale in a way that differs between scales below and above 4; averaging (majority of quarts) costs a lookup per quart, up to 4,096 per pixel at scale 256, and shows a biome no single point has.
- **The `image` crate:** it brings many codecs to write one format; `png` is what `image` uses for PNG anyway.
