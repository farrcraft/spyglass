# Map rendering

Status: stable. Covers map images of each of the Default preset's dimensions from the `spyglass` crate's `render` module ([ADR 0017](../adr/0017-map-rendering-in-the-public-api.md)), which the viewer and `spyglass render` both use. Rendering adds no worldgen of its own: every pixel and marker comes from a query the API already answers ([locate.md](locate.md), [biome-lookup.md](biome-lookup.md), [material.md](material.md), [carvers.md](carvers.md)).

## Map areas

A `MapArea` is:

- `min`: the block `(x, z)` at the top-left pixel's corner. North (−z) is up, east (+x) is right.
- `size`: width and height in pixels, each 1 to 16,384, at most 2²⁶ pixels in all.
- `scale`: blocks per pixel, a power of two from 1 to 256.
- `y`: the block height biomes and the [underground layers](#underground) are sampled at. The default is 64.

`min` must be a multiple of `scale` on both axes, and the area's last block, `min + size·scale − 1`, must fit in an `i32`. Anything else is an error, not a panic.

`MapArea::centred_on(centre, size, scale, y)` places an area so that block `centre` is at pixel `size / 2` (rounded down): `min = floor((centre − (size / 2)·scale) / scale)·scale`, per axis, in 64-bit arithmetic. Both front ends use it, so a view and the command copied from it cover the same blocks.

## Biomes

Pixel `(i, j)` covers blocks `min + (i, j)·scale` up to the next pixel. It shows the biome of the quart containing its corner block `(min.x + i·scale, y, min.z + j·scale)`, the stored quart biome that `Session::biome_at` returns. The grid is aligned to the world, so two areas at the same scale and height agree on every pixel they share, and a view equals the same view assembled from tiles.

At scales 1 and 2 a quart covers several pixels. From scale 4 up, each pixel is one quart and the quarts between pixels aren't sampled: at scale 64, one quart in 16 along each axis. A biome smaller than the spacing can fall between samples and not show.

**Biomes are a slice at one height, not the surface,** unless asked for at the surface. The game picks biomes in 3D. Under high terrain, the biome at y 64 can differ from the one at the surface above (a cave biome under a mountain, for example).

**At the surface** (`Session::surface_biome_map`, from a [height map](#heights)), pixel `(i, j)` shows the biome of the quart containing `(x, h, z)`, where `(x, z)` is its corner block and `h` that column's height, kept at most the level's top block (319 in the Overworld). Each pixel is sampled at its own column, at every scale, so at scales 1 and 2 the pixels of one quart can differ. Each pixel needs its column's height, so surface biomes cost what [heights](#heights) do.

Where a cell is tied ([biome-lookup.md](biome-lookup.md#ties)), the pixel's biome is spyglass's answer, the first of the tie, and the tie is kept with it.

## Heights

`Session::height_map(area)` is the terrain height at each pixel's corner block: `Session::height_at` with `WORLD_SURFACE_WG` ([terrain.md](terrain.md)), one above the highest block that isn't air, so water counts. It also holds the heights one pixel beyond the area's west and north edges, which relief compares against; the first of those may be one pixel past the edge of block coordinates, and is then clamped to `i32::MIN`. Heights are sampled chunk by chunk, each chunk's density volume only as large as the box around its sampled columns, which gives each column the same answer as its own ([terrain.md](terrain.md)).

A chunk's heights cost a few milliseconds, and height maps are drawn at every scale. From scale 4 out each column is on its own cell corner and shares no density with its neighbours ([terrain.md](terrain.md#the-column)), and a column costs more as the pixels spread out, until each is in a chunk of its own. For 26.3 (a `dist` build, one thread), a 256 × 256 tile takes about 0.9 s at scale 1 and 5 s at scale 4; on a release build of `spyglass render`, seed 446419313063100416 around 0, 0, a tile took 6.2 s at scale 4, 7.1 s at 8, 10.8 s at 16, 17.7 s at 32 and about 21 s at 64 and 128.

## Underground

The underground layers show a chunk's blocks before features ([material.md](material.md), [carvers.md](carvers.md)) at the area's `y`. Pixel `(i, j)` shows its corner block `(min.x + i·scale, y, min.z + j·scale)`, as biomes do, and depends on nothing else, so a view equals its tiles. Each is sampled at that block alone, not by generating the chunk, which takes about 40 ms (`Session::underground_map`):

- **Ore veins** (`Layers::veins`): the vein block at the position, if any. In 26.2, the noise fill's own vein blocks. From 26.3, the material rule's `ore_vein` rules where the noise fill is solid. The rule is followed at the position alone, up to the first condition that needs more than the position (the column, biome or surface): `noise_threshold`, `vertical_gradient` and `hole` need only the position, and every other condition stops it there. Vanilla's rules have the veins straight after the bedrock floor, before any such condition, so this is the chunk's block. A vein is named by its ore block: copper (`minecraft:copper_ore`, with raw copper blocks and granite as its filler) or iron (`minecraft:deepslate_iron_ore`, with raw iron blocks and tuff). Asking for veins in a dimension without them (no `ore_vein` rule from 26.3; in 26.2, `ore_veins_enabled` off) is an error.
- **Noise caves** (`Layers::noise_caves`): air or a fluid in the noise fill ([terrain.md](terrain.md)) with a solid block somewhere above it in the column, so the caves of the density graph and the aquifer's underground lakes and lava, but not the sky or the sea. Below the surface the material rule leaves the fill's air and fluids as they are, so this is the chunk's air and fluid before carvers there.
- **Carvers** (`Layers::carvers`): the positions in the chunk's carving mask, the union of every carver's ([carvers.md](carvers.md)), by the kind of carver that carved each first in the order the game runs them: caves (`cave`, `cave_extra_underground` and `nether_cave`) or canyons (ravines). Where a start chunk's carver biome is tied, its first candidate's carvers. A carved position is usually air or a fluid in the chunk, but not where the block is `#uncarvable`, or, in 26.2, not in the carver's `replaceable` tag; every block carvers change is carved, or is the dirt under carved grass made the surface again.

`Session::carving_map` gives the carved positions under each pixel at every height, and `Session::underground_map` takes one for the same blocks and scale at any `y`, so a front end that changes the height doesn't carve again. Carving a chunk visits the starts of 17 × 17 chunks around it, and is most of the carvers' cost.

The underground layers are drawn at scales up to `MAX_UNDERGROUND_SCALE` (4); larger scales are an error. For 26.3 (a `dist` build, one thread), a 128 × 128 tile at y 0 takes about 0.2 s at scale 1 with each layer and 0.3 s with all three, and at scale 4 about 1.4 s for veins or noise caves, 1.0 s for carvers and 2.4 s for all three. Relief takes 1.2 s on the same tile at scale 4. With a carving map kept, carvers at another height take under a millisecond. Past scale 4 carving grows quickly, since each pixel is in a chunk of its own: 3.5 s for carvers at scale 8, 15 s at 16.

## Colours

Biome colours come from a built-in table keyed by biome id ([ADR 0018](../adr/0018-biome-palette.md)); structure-set marker colours from a second table keyed by set id. An id missing from a table gets a hashed colour: the 32-bit FNV-1a hash of its UTF-8 bytes, with bytes 0, 1 and 2 (least significant first) as red, green and blue, each mapped to `48 + byte·176 / 255` (integer division). New ids should be added to the tables when a version adds them.

Front ends name an id with `display_name`: the id's path with underscores as spaces and the first letter capitalised, so `minecraft:cherry_grove` is "Cherry grove", followed by the namespace in brackets if it isn't `minecraft`. Bundles have no language files, so these aren't the game's translated names.

All compositing is integer arithmetic. Blending colour `s` over `d` with alpha `a` (0–255) gives `(s·a + d·(255 − a) + 127) / 255` per channel. Every output pixel is opaque.

## Layers

Drawn in this order, each over the last:

1. **Biomes**, at the area's `y` or [at the surface](#biomes). With the biome layer off: a grey by height if relief is on, `grey = (clamp(h − min_y, 0, height) · 255) / height` for the level's `min_y` and `height`; otherwise a flat background of `#808080` if an underground layer is on, and `#202020` if not.
   - **Ties**, if on: a tied pixel shows candidate `(gx + gz) mod n` of its `n` tied biomes instead of the first, where `(gx, gz) = min / scale + (i, j)` is the pixel's position on the world-aligned grid. Neighbouring tied pixels alternate, so tied areas show as a checker of their candidates.
   - **Highlight**, if any biomes are chosen: a pixel stays as it is if any of its candidates is chosen (the game may give that biome there). Otherwise it's dimmed: with `grey = (77·r + 150·g + 29·b) / 256`, each channel becomes `(c + 3·grey) / 8`.
   - **Relief**, if on: each pixel shaded by the [heights](#heights) around it, lit from the north-west. With `h` its height and `h_w`, `h_n` those of the pixels west and north of it, `shade = clamp(floor((2h − h_w − h_n) · 32 / scale), −128, 128)` in units of 1/256; each channel `c` becomes `c + (255 − c) · shade / 256` for a positive shade and `c · (256 + shade) / 256` otherwise, in integer division. Flat ground is unchanged; ground rising to the south-east is lighter, falling darker.
   - **Void**, wherever heights are drawn (relief or surface biomes): a column with no blocks, whose `WORLD_SURFACE_WG` height is the level's lowest y ([terrain.md](terrain.md)), is `#0b0b12` whatever its biome. In the End that's the space between the islands, which the biome source can't show: it gives `the_end` to every column within 1,024 blocks of the origin.
2. **Underground**, at the area's `y` (scales up to 4), each [blended](#colours) over the last:
   - **Ore veins**: ore and raw-ore blocks in the vein's colour at alpha 255, filler at alpha 128. Copper's colour is `#e8743c`, iron's `#b8c4ff`, for a vein named by `minecraft:copper_ore` or `minecraft:deepslate_copper_ore`, and `minecraft:iron_ore` or `minecraft:deepslate_iron_ore`. Any other vein has the hashed colour of its ore block's id.
   - **Noise caves**: `#000000` at alpha 160.
   - **Carvers**: caves `#ffd830`, canyons `#ff40c8`, at alpha 200.
3. **Slime chunks**: each slime chunk's pixels blended with `#40e040` at alpha 128. Only at scales up to 16, where a chunk is at least a pixel; asking for them at a larger scale is an error.
4. **Structures**: a marker at each start `Session::structures_in` finds for the chosen structures and tags, [confirmed](locate.md#confirmed-mode) if `Layers::confirm` is on (it's off by default), at the start's `position` (x and z). The marker's centre pixel is `floor((position − min) / scale)`. A certain start is a 9 × 9 square: a 1-pixel black border around the set's colour. A candidate is the same square with its middle 3 × 3 left showing the map, so it reads as hollow. Markers are drawn in order of set id, then chunk z, then chunk x, so overlaps resolve the same way in every area.
5. **Features**: a marker at each certain or likely answer `Session::features_in` finds for the chosen features ([locate.md](locate.md#features)), at its position's x and z, only at scales up to 2 (`MAX_FEATURE_SCALE`): every chunk the markers could be in is placed, about 30 ms a chunk, and asking for features at a larger scale is an error. A diamond: with `(dx, dy)` a pixel's offset from the centre, black where `|dx| + |dy| = 4`, the feature's colour where it's 2 or 3, and where it's 0 or 1 for a certain answer (a likely candidate is hollow there). Colours by placed feature: dungeons `#9c9c9c` and deep dungeons `#707078`, amethyst geodes `#b070e8`, desert wells `#e8d080`, fossils `#f0ecd8` (upper) and `#c8d8f0` (lower), lava lakes `#ff7020` (underground) and `#ffa030` (surface); any other the hashed colour of its id. Markers are drawn by z, then x, then y, then placed feature.
6. **Loot**: a ring around each loot container holding `Layers::loot`'s item, or that might (`Session::containers_holding`, [locate.md](locate.md#loot)), at its position's x and z, only at scales up to 2 (`MAX_FEATURE_SCALE`), as features are. With `(dx, dy)` a pixel's offset from the centre: gold `#ffc830` where `9 ≤ dx² + dy² ≤ 16`, and black where `17 ≤ dx² + dy² ≤ 25`. A container whose contents certainly hold the item has a whole gold ring; for one that might, the gold is dotted, black where `(dx + dy) mod 3 = 0`. The middle shows what's under it, such as a dungeon's diamond. Rings are drawn by z, then x, then y.
7. **Spawn**: a cross centred on `SpawnOrigin::position`, or on the centre of its chunk when there's no position: a white cross 1 pixel wide reaching 4 pixels from the centre, over a black one 3 pixels wide reaching 5.
8. **Pins**: `Layers::pins`, places a front end gives with a colour each, such as the viewer's markers ([user-data.md](user-data.md#markers)). Each is a disc centred on its block's pixel: with `(dx, dy)` a pixel's offset from the centre, the pin's colour where `dx² + dy² ≤ 9`, and black where `10 ≤ dx² + dy² ≤ 20`. Pins are drawn in the order given, so a later one covers an earlier one. They're drawn in every dimension.

The viewer draws its own icons in place of these markers ([ADR 0044](../adr/0044-the-viewer-draws-its-own-map-icons.md)): at the same centre pixels, in the same set colours, filled for a certain start and hollow for a candidate. Features are diamonds in their colours, filled when certain and outlined when likely. Its Loot section marks every loot container up close as a small brown square (**Containers**), and rings in gold those holding an item: a thick ring where the contents certainly hold it, a thin one where they might. Clicking a container opens its contents, slot by slot. Exports, from the library, the CLI or the viewer, draw the markers above.

Markers have a fixed size in pixels at every scale. A marker can overlap an area while its position is outside it, so an area includes every feature whose centre pixel is within 5 pixels of it; markers near an edge are cut off, and a tile shows the same pixels as the larger view it's part of.

`Session::map_features` returns the features an area would draw (structure starts, features, loot containers, slime chunks, the spawn, pins), and `Session::render_with` draws an area with them, so they're found once, so front ends and scripts can list or label them with exactly what the image shows.

## Dimensions

A session draws its own dimension: biomes and structures in all three. Some layers exist only in some dimensions, and asking for one elsewhere is an error (`Layers::check`, which `Session::map_features` and `Session::render` apply):

- **Slime chunks** are the Overworld's: slimes spawn in them only there.
- **The spawn** is the Overworld's world spawn. The Nether's and End's spawn origins are chunk (0, 0) by rule ([spawn.md](spawn.md)), not a place anyone spawns, so they aren't drawn.
- **Ore veins** are drawn only in a dimension that has them (the Overworld, in vanilla). Noise caves and carvers are drawn in all three: the Nether's open space is under its ceiling, so it's all noise cave.
- **Terrain layers** (relief and surface biomes) aren't drawn in a dimension whose type has `has_ceiling` (the Nether). There, `WORLD_SURFACE_WG` finds the ceiling, about 128 everywhere ([terrain.md](terrain.md)), so relief would be flat and surface biomes a slice at the ceiling. The heights themselves stay available through `Session::height_map`. The End has no ceiling, and its terrain layers draw the islands over the void, where the height is the level's bottom.

## Images

`Image` is 8-bit RGBA, rows top to bottom, alpha always 255. `Image::write_png` writes an 8-bit RGB PNG with no ancillary chunks, so a given spyglass build writes the same bytes for the same image.

## Testing

On fixture bundles, without game data: a view equals itself assembled from tiles at every scale, layers on, terrain layers included at scales 1 to 16 on sloping fixture terrain; each pixel's biome equals `Session::biome_at` at its sample block, and at the surface at its column's height; each height equals `Session::height_at`; relief lightens exactly the pixels higher than a western or northern neighbour on rising ground; columns with no blocks are the void wherever heights are drawn, at scales 1, 4 and 16, and never without heights; the features drawn are exactly those `structures_in`, `slime_chunks` and `spawn_origin` give for the area and its margin, confirmed or not; invalid areas, and underground layers past scale 4, are errors; on a fixture Nether, slime chunks, the spawn and terrain layers are errors and biomes draw. On a fixture with a noise cave, a copper vein rule and carvers: a view equals its tiles with every underground layer on, at scales 1 to 4, over biomes and grey; each pixel's vein, noise cave and carver agree with `Session::chunk_blocks` at its corner block, before and after carvers; each pixel's colour is the specified blend; a carving map gives the same pixels at another height, and is refused for another area; veins in a dimension without them are an error. `spyglass render` writes the same bytes as `Session::render` for the same request. Ignored tests on the real bundle (`cargo xtask oracle-test`) check tiling and sampling there too, terrain layers, underground layers and confirmed markers included, and `Biomes::biome_grid` against `biome_at`. In every supported version, the underground layers' samples (the noise fill's block at a y, veins at a position, and carvings) are checked against whole chunks in `spyglass-blocks`, before and after carvers, in the Overworld and the Nether. The heights a map samples are checked against the game by the height golden test, which samples a sparse grid the same way ([terrain.md](terrain.md)).
