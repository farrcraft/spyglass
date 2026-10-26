# Map rendering

Status: stable. Covers map images of each of the Default preset's dimensions from the `spyglass` crate's `render` module ([ADR 0017](../adr/0017-map-rendering-in-the-public-api.md)), which the viewer and `spyglass render` both use. Rendering adds no worldgen of its own: every pixel and marker comes from a query the API already answers ([locate.md](locate.md), [biome-lookup.md](biome-lookup.md)).

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

**Biomes are a slice at one height, not the surface,** unless asked for at the surface. The game picks biomes in 3D. Under high terrain, the biome at y 64 can differ from the one at the surface above (a cave biome under a mountain, for example).

**At the surface** (`Session::surface_biome_map`, from a [height map](#heights)), pixel `(i, j)` shows the biome of the quart containing `(x, h, z)`, where `(x, z)` is its corner block and `h` that column's height, kept at most the level's top block (319 in the Overworld). Each pixel is sampled at its own column, at every scale, so at scales 1 and 2 the pixels of one quart can differ. Surface biomes are drawn at scales up to `MAX_TERRAIN_SCALE` (4), since each pixel needs its column's height.

Where a cell is tied ([biome-lookup.md](biome-lookup.md#ties)), the pixel's biome is spyglass's answer, the first of the tie, and the tie is kept with it.

## Heights

`Session::height_map(area)` is the terrain height at each pixel's corner block: `Session::height_at` with `WORLD_SURFACE_WG` ([terrain.md](terrain.md)), one above the highest block that isn't air, so water counts. It also holds the heights one pixel beyond the area's west and north edges, which relief compares against; the first of those may be one pixel past the edge of block coordinates, and is then clamped to `i32::MIN`. Heights are sampled chunk by chunk, each chunk's density volume only as large as the box around its sampled columns, which gives each column the same answer as its own ([terrain.md](terrain.md)).

A chunk's heights cost a few milliseconds, so height maps are limited to scales up to `MAX_TERRAIN_SCALE`, 4; larger scales are an error. For 26.3 (release, one thread), a 256 × 256 tile takes about 1.3 s at scale 1 and 11 s at scale 4.

## Colours

Biome colours come from a built-in table keyed by biome id ([ADR 0018](../adr/0018-biome-palette.md)); structure-set marker colours from a second table keyed by set id. An id missing from a table gets a hashed colour: the 32-bit FNV-1a hash of its UTF-8 bytes, with bytes 0, 1 and 2 (least significant first) as red, green and blue, each mapped to `48 + byte·176 / 255` (integer division). New ids should be added to the tables when a version adds them.

All compositing is integer arithmetic. Blending colour `s` over `d` with alpha `a` (0–255) gives `(s·a + d·(255 − a) + 127) / 255` per channel. Every output pixel is opaque.

## Layers

Drawn in this order, each over the last:

1. **Biomes**, at the area's `y` or [at the surface](#biomes). With the biome layer off: a grey by height if relief is on, `grey = (clamp(h − min_y, 0, height) · 255) / height` for the level's `min_y` and `height`; otherwise a flat background of `#202020`.
   - **Ties**, if on: a tied pixel shows candidate `(gx + gz) mod n` of its `n` tied biomes instead of the first, where `(gx, gz) = min / scale + (i, j)` is the pixel's position on the world-aligned grid. Neighbouring tied pixels alternate, so tied areas show as a checker of their candidates.
   - **Highlight**, if any biomes are chosen: a pixel stays as it is if any of its candidates is chosen (the game may give that biome there). Otherwise it's dimmed: with `grey = (77·r + 150·g + 29·b) / 256`, each channel becomes `(c + 3·grey) / 8`.
   - **Relief**, if on (scales up to 4): each pixel shaded by the [heights](#heights) around it, lit from the north-west. With `h` its height and `h_w`, `h_n` those of the pixels west and north of it, `shade = clamp(floor((2h − h_w − h_n) · 32 / scale), −128, 128)` in units of 1/256; each channel `c` becomes `c + (255 − c) · shade / 256` for a positive shade and `c · (256 + shade) / 256` otherwise, in integer division. Flat ground is unchanged; ground rising to the south-east is lighter, falling darker.
2. **Slime chunks**: each slime chunk's pixels blended with `#40e040` at alpha 128. Only at scales up to 16, where a chunk is at least a pixel; asking for them at a larger scale is an error.
3. **Structures**: a marker at each start `Session::structures_in` finds for the chosen structures and tags, [confirmed](locate.md#confirmed-mode) if `Layers::confirm` is on (it's off by default), at the start's `position` (x and z). The marker's centre pixel is `floor((position − min) / scale)`. A certain start is a 9 × 9 square: a 1-pixel black border around the set's colour. A candidate is the same square with its middle 3 × 3 left showing the map, so it reads as hollow. Markers are drawn in order of set id, then chunk z, then chunk x, so overlaps resolve the same way in every area.
4. **Spawn**: a cross centred on `SpawnOrigin::position`, or on the centre of its chunk when there's no position: a white cross 1 pixel wide reaching 4 pixels from the centre, over a black one 3 pixels wide reaching 5.

Markers have a fixed size in pixels at every scale. A marker can overlap an area while its position is outside it, so an area includes every feature whose centre pixel is within 5 pixels of it; markers near an edge are cut off, and a tile shows the same pixels as the larger view it's part of.

`Session::map_features` returns the features an area would draw (structure starts, slime chunks, the spawn), so front ends and scripts can list or label them with exactly what the image shows.

## Dimensions

A session draws its own dimension: biomes and structures in all three. Some layers exist only in some dimensions, and asking for one elsewhere is an error (`Layers::check`, which `Session::map_features` and `Session::render` apply):

- **Slime chunks** are the Overworld's: slimes spawn in them only there.
- **The spawn** is the Overworld's world spawn. The Nether's and End's spawn origins are chunk (0, 0) by rule ([spawn.md](spawn.md)), not a place anyone spawns, so they aren't drawn.
- **Terrain layers** (relief and surface biomes) aren't drawn in a dimension whose type has `has_ceiling` (the Nether). There, `WORLD_SURFACE_WG` finds the ceiling, about 128 everywhere ([terrain.md](terrain.md)), so relief would be flat and surface biomes a slice at the ceiling. The heights themselves stay available through `Session::height_map`. The End has no ceiling, and its terrain layers draw the islands over the void, where the height is the level's bottom.

## Images

`Image` is 8-bit RGBA, rows top to bottom, alpha always 255. `Image::write_png` writes an 8-bit RGB PNG with no ancillary chunks, so a given spyglass build writes the same bytes for the same image.

## Testing

On fixture bundles, without game data: a view equals itself assembled from tiles at every scale, layers on, terrain layers included at scales 1 to 4 on sloping fixture terrain; each pixel's biome equals `Session::biome_at` at its sample block, and at the surface at its column's height; each height equals `Session::height_at`; relief lightens exactly the pixels higher than a western or northern neighbour on rising ground; the features drawn are exactly those `structures_in`, `slime_chunks` and `spawn_origin` give for the area and its margin, confirmed or not; invalid areas, and terrain layers past scale 4, are errors; on a fixture Nether, slime chunks, the spawn and terrain layers are errors and biomes draw. `spyglass render` writes the same bytes as `Session::render` for the same request. Ignored tests on the real bundle (`cargo xtask oracle-test`) check tiling and sampling there too, terrain layers and confirmed markers included, and `Biomes::biome_grid` against `biome_at`. The heights a map samples are checked against the game by the height golden test, which samples a sparse grid the same way ([terrain.md](terrain.md)).
