# Map rendering

Status: draft. Covers map images of the Overworld from the `spyglass` crate's `render` module ([ADR 0017](../adr/0017-map-rendering-in-the-public-api.md)), which the viewer and `spyglass render` both use. Rendering adds no worldgen of its own: every pixel and marker comes from a query the API already answers ([locate.md](locate.md), [biome-lookup.md](biome-lookup.md)).

## Map areas

A `MapArea` is:

- `min`: the block `(x, z)` at the top-left pixel's corner. North (−z) is up, east (+x) is right.
- `size`: width and height in pixels, each 1 to 16,384, at most 2²⁶ pixels in all.
- `scale`: blocks per pixel, a power of two from 1 to 256.
- `y`: the block height biomes are sampled at. The default is 64.

`min` must be a multiple of `scale` on both axes, and the area's last block, `min + size·scale − 1`, must fit in an `i32`. Anything else is an error, not a panic.

`MapArea::centred_on(centre, size, scale, y)` places an area so that block `centre` is at pixel `size / 2` (rounded down): `min = floor((centre − (size / 2)·scale) / scale)·scale`, per axis, in 64-bit arithmetic. Both front ends use it, so a view and the command copied from it cover the same blocks.

## Biomes

Pixel `(i, j)` covers blocks `min + (i, j)·scale` up to the next pixel. It shows the biome of the quart containing its corner block `(min.x + i·scale, y, min.z + j·scale)`, the stored quart biome that `Session::biome_at` returns. The grid is aligned to the world, so two areas at the same scale and height agree on every pixel they share, and a view equals the same view assembled from tiles.

At scales 1 and 2 a quart covers several pixels. From scale 4 up, each pixel is one quart and the quarts between pixels aren't sampled: at scale 64, one quart in 16 along each axis. A biome smaller than the spacing can fall between samples and not show.

**Biomes are a slice at one height, not the surface.** The game picks biomes in 3D. Under high terrain, the biome at y 64 can differ from the one at the surface above (a cave biome under a mountain, for example). Surface biomes need terrain heights, which spyglass doesn't compute yet ([roadmap](../roadmap/README.md)).

Where a cell is tied ([biome-lookup.md](biome-lookup.md#ties)), the pixel's biome is spyglass's answer, the first of the tie, and the tie is kept with it.

## Colours

Biome colours come from a built-in table keyed by biome id ([ADR 0018](../adr/0018-biome-palette.md)); structure-set marker colours from a second table keyed by set id. An id missing from a table gets a hashed colour: the 32-bit FNV-1a hash of its UTF-8 bytes, with bytes 0, 1 and 2 (least significant first) as red, green and blue, each mapped to `48 + byte·176 / 255` (integer division). New ids should be added to the tables when a version adds them.

All compositing is integer arithmetic. Blending colour `s` over `d` with alpha `a` (0–255) gives `(s·a + d·(255 − a) + 127) / 255` per channel. Every output pixel is opaque.

## Layers

Drawn in this order, each over the last:

1. **Biomes**, or, with the biome layer off, a flat background of `#202020`.
   - **Ties**, if on: a tied pixel shows candidate `(gx + gz) mod n` of its `n` tied biomes instead of the first, where `(gx, gz) = min / scale + (i, j)` is the pixel's position on the world-aligned grid. Neighbouring tied pixels alternate, so tied areas show as a checker of their candidates.
   - **Highlight**, if any biomes are chosen: a pixel stays as it is if any of its candidates is chosen (the game may give that biome there). Otherwise it's dimmed: with `grey = (77·r + 150·g + 29·b) / 256`, each channel becomes `(c + 3·grey) / 8`.
2. **Slime chunks**: each slime chunk's pixels blended with `#40e040` at alpha 128. Only at scales up to 16, where a chunk is at least a pixel; asking for them at a larger scale is an error.
3. **Structures**: a marker at each start `Session::structures_in` finds for the chosen structures and tags, at the start's `position` (x and z). The marker's centre pixel is `floor((position − min) / scale)`. A certain start is a 9 × 9 square: a 1-pixel black border around the set's colour. A candidate is the same square with its middle 3 × 3 left showing the map, so it reads as hollow. Markers are drawn in order of set id, then chunk z, then chunk x, so overlaps resolve the same way in every area.
4. **Spawn**: a cross centred on `SpawnOrigin::position`, or on the centre of its chunk when there's no position: a white cross 1 pixel wide reaching 4 pixels from the centre, over a black one 3 pixels wide reaching 5.

Markers have a fixed size in pixels at every scale. A marker can overlap an area while its position is outside it, so an area includes every feature whose centre pixel is within 5 pixels of it; markers near an edge are cut off, and a tile shows the same pixels as the larger view it's part of.

`Session::map_features` returns the features an area would draw (structure starts, slime chunks, the spawn), so front ends and scripts can list or label them with exactly what the image shows.

## Images

`Image` is 8-bit RGBA, rows top to bottom, alpha always 255. `Image::write_png` writes an 8-bit RGB PNG with no ancillary chunks, so a given spyglass build writes the same bytes for the same image.

## Testing

On fixture bundles, without game data: a view equals itself assembled from tiles at every scale, layers on; each pixel's biome equals `Session::biome_at` at its sample block; the features drawn are exactly those `structures_in`, `slime_chunks` and `spawn_origin` give for the area and its margin; invalid areas are errors. `spyglass render` writes the same bytes as `Session::render` for the same request.
