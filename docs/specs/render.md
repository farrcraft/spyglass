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

## Flowers

`Session::flower_map(area)` gives the flower bone meal grows at each pixel's corner block, `(min.x + i·scale, y, min.z + j·scale)` with `y` the area's: the block above the grass ([flowers.md](flowers.md)). Each pixel is that block's answer alone, so a view equals its tiles. A pixel's answer is spyglass's biome's (the first of a tie): its flowers by block id, a flower's states together, each with its chance, most likely first. It's **tied** when another biome of the cell's tie would grow other flowers or other chances; nothing there is certain.

`FlowerMap::key` lists the flowers the map shows, by block id, with the pixels each can grow at and those it's certain at, most pixels first.

`Session::flower_heights(area, flower, [min_y, max_y], step)` gives, for every `step`th height from `min_y` to `max_y`, the pixels of the area where the flower is certain and where it can grow, from `flower_map` at that height. So the best heights for a farm and the layer agree at every height they name.

The layer is drawn at every scale: a pixel costs a biome lookup (cached per quart) and the providers' noises. With `cargo xtask bench` on 16 threads, a 128 × 128 tile takes about 20–35 ms at 1:1, where pixels share quarts, and 60–90 ms at 1:16, where none do: about twice a biome tile's cost. `flower_heights` over a 128 × 128 area at 31 heights takes about 2 s. Flower colours come from a built-in table by block id, and any other block has the hashed colour of its id ([below](#colours)).

## Reuse across scales

Every biome, height, surface biome, underground, carving and flower pixel is its corner block's answer alone, as the sections above say. So a pixel at one scale is the same pixel at any scale where its corner block is a pixel corner: a 1:2 map's pixels are every other pixel, on both axes, of the 1:1 maps under it, and a 1:1 map's even pixels are a 1:2 map's. Relief is drawn from a height map's pixels and its edge row, the columns one pixel west and north of the area, and those are pixels too: a coarser map's edge row is a column of the finer map beside it.

`Known::new(area)` holds the pixels already known for an area, and `Known::add(map)` adds those of a map of the same kind at any scale: each pixel of the area whose corner block is one of the map's pixel corners, a height map's edge row included. Maps are of the same kind when they're from the same world and:

| Map                                 | Shares pixels with maps                         |
| ----------------------------------- | ----------------------------------------------- |
| `BiomeMap` from `biome_map`         | sampled at the same `y`                         |
| `BiomeMap` from `surface_biome_map` | at the surface                                  |
| `HeightMap`                         | at any `y`                                      |
| `CarvingMap`                        | at any `y`                                      |
| `UndergroundMap`                    | at the same `y`, with the same layers asked for |
| `FlowerMap`                         | at the same `y`                                 |

A map of another kind is an error, and so is a map of another world.

Each map call has a `…_with` form that takes a `Known` for its area and computes only the pixels it lacks: `biome_map_with`, `height_map_with`, `surface_biome_map_with`, `carving_map_with`, `underground_map_with` and `flower_map_with`, on `Session` and on `Stopping`. The map is built the same way whatever was known, so it equals the one computed directly, ties, a vein's ore and the edge row included. With every pixel known, nothing is computed: no biome is looked up, no column sampled and no chunk carved. Where a quart covers several pixels (biomes at scales 1 and 2), it's known if any of them is. Heights and the underground probes are still sampled a band of rows at a time, the rows lacking the same columns together, so a column's answer is its own ([heights](#heights)).

**What the viewer reuses.** A tile whose finer tiles are cached takes their pixels: the four under it and, with heights, those west and north of them for its edge row. Covered whole, it computes nothing, and it's queued before everything else, ahead of the coarse stand-ins; covered in part, it computes the rest. The workers' carving maps are assembled from those of the four tiles under a tile the same way. Copying a whole tile from finer ones takes about 6 ms of work with every layer on (mostly flowers, whose answers are hashed again), against 0.3 to 3 s to compute it, so it's a job, at the front of the queue, rather than work on the UI thread: the workers do a view's worth in a few frames, where the UI thread could afford a tile or two a frame.

**Past a layer's limit.** Features and loot containers are kept by region ([layers](#layers)), and one zoom step past their limit, at 1:4, the viewer still draws those it found closer in, from the regions it has, and computes nothing new. Each region it has is outlined where it meets one it doesn't, and those are hatched, so an empty region reads as "nothing here" and a hatched one as "not looked at"; the layer's control says it's paused and that what's drawn was found earlier. Not further: at 1:8, dungeons pile into one blob and the hatch covers most of the map.

The other way, a tile started from the quarter of its parent that it shares, saves too little to be worth it, and the viewer doesn't. On 26.3, three seeds, a 128 × 128 tile at 1:1 from 1:2, 1:2 from 1:4 and 1:4 from 1:8 saved under 12% for biomes, under 10% for flowers and under 5% for heights, surface biomes, carvings and the underground probes, some slower: biomes and flowers are looked up per quart and cached, and heights and probes are sampled a chunk at a time, so a chunk with three columns in four still to compute costs what it did with four.

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
3. **Flowers**, at the area's `y`, if on ([above](#flowers)), [blended](#colours) over the last. A pixel with no flower is dimmed as a highlight dims. Otherwise, with `alpha(c) = 255` for a flower certain there, and `64 + ⌊159 · c⌋` for one with chance `c` otherwise:
   - with no flower chosen, its most likely flower, at that flower's alpha;
   - with a flower chosen (`Layers::flower`, a block id some bone meal feature can place), that flower at its alpha where its chance is above 0, and the pixel dimmed where it's 0.

   Its colours: allium `#b55fd9`, azure bluet `#d6e4ee`, blue orchid `#2fa8e0`, closed eyeblossom `#8a6d8f`, cornflower `#4664e0`, dandelion `#f5d03b`, lily of the valley `#f2f6e6`, orange tulip `#f08a2a`, oxeye daisy `#e8e0a8`, pink petals `#f4a6c8`, pink tulip `#f090c0`, poppy `#d8352a`, red tulip `#e0412f`, short grass `#6fa64a`, tall grass `#5c9a3c`, white tulip `#f4f4f4`, wildflowers `#f2e04a`. With the biome layer and relief off, the base is `#808080`, as for the underground layers.

4. **Generated chunks**, for a session with a save (`Session::with_save`, [world-saves.md](world-saves.md#region-files)): each chunk the save's region files list, blended over its pixels as slime chunks are. A finished chunk (status `full`) is `#ff30ff` at alpha 112; one generated partway is the same at alpha 48; one an older version saved (or with no data version) is `#ffa020`, and one that can't be read `#ff2020`, at alpha 112 if finished or unreadable and 48 if not. Only at scales up to 16, as slime chunks; asking for them at a larger scale, or without a save, is an error. With `Layers::inhabited`, a readable chunk's alpha is instead `min(32 + 16 · ⌊log2(m + 1)⌋, 224)`, `m` its `InhabitedTime` in whole minutes (ticks / 1200): faint where players passed through, about 112 after an hour, 176 after ten.
5. **Slime chunks**: each slime chunk's pixels blended with `#40e040` at alpha 128. Only at scales up to 16, where a chunk is at least a pixel; asking for them at a larger scale is an error.
6. **Structures**: a marker at each start `Session::structures_in` finds for the chosen structures and tags, [confirmed](locate.md#confirmed-mode) if `Layers::confirm` is on (it's off by default), at the start's `position` (x and z). The marker's centre pixel is `floor((position − min) / scale)`. A certain start is a 9 × 9 square: a 1-pixel black border around the set's colour. A candidate is the same square with its middle 3 × 3 left showing the map, so it reads as hollow. Markers are drawn in order of set id, then chunk z, then chunk x, so overlaps resolve the same way in every area.
7. **Features**: a marker at each certain or likely answer `Session::features_in` finds for the chosen features ([locate.md](locate.md#features)), at its position's x and z, only at scales up to 2 (`MAX_FEATURE_SCALE`): every chunk the markers could be in is placed, about 30 ms a chunk, and asking for features at a larger scale is an error. A diamond: with `(dx, dy)` a pixel's offset from the centre, black where `|dx| + |dy| = 4`, the feature's colour where it's 2 or 3, and where it's 0 or 1 for a certain answer (a likely candidate is hollow there). Colours by placed feature: dungeons `#9c9c9c` and deep dungeons `#707078`, amethyst geodes `#b070e8`, desert wells `#e8d080`, fossils `#f0ecd8` (upper) and `#c8d8f0` (lower), lava lakes `#ff7020` (underground) and `#ffa030` (surface); any other the hashed colour of its id. Markers are drawn by z, then x, then y, then placed feature.
8. **Loot**: a ring around each loot container holding `Layers::loot`'s item, or that might (`Session::containers_holding`, [locate.md](locate.md#loot)), at its position's x and z, only at scales up to 2 (`MAX_FEATURE_SCALE`), as features are. With `(dx, dy)` a pixel's offset from the centre: gold `#ffc830` where `9 ≤ dx² + dy² ≤ 16`, and black where `17 ≤ dx² + dy² ≤ 25`. A container whose contents certainly hold the item has a whole gold ring; for one that might, the gold is dotted, black where `(dx + dy) mod 3 = 0`. The middle shows what's under it, such as a dungeon's diamond. Rings are drawn by z, then x, then y.
9. **Spawn**: a cross centred on `SpawnOrigin::position`, or on the centre of its chunk when there's no position: a white cross 1 pixel wide reaching 4 pixels from the centre, over a black one 3 pixels wide reaching 5.
10. **Pins**: `Layers::pins`, places a front end gives with a colour each, such as the viewer's markers ([user-data.md](user-data.md#markers)). Each is a disc centred on its block's pixel: with `(dx, dy)` a pixel's offset from the centre, the pin's colour where `dx² + dy² ≤ 9`, and black where `10 ≤ dx² + dy² ≤ 20`. Pins are drawn in the order given, so a later one covers an earlier one. They're drawn in every dimension.

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

## Stopping

A front end that computes the map in the background can stop a call it no longer needs ([ADR 0078](../adr/0078-map-calls-can-be-stopped.md)). `Session::stopping(&stop)` gives a view of the session whose calls check a `Stop` (raised from another thread with `Stop::stop`) and return `Error::Stopped` soon after it's raised, with no partial answer. While the stop isn't raised, each gives the session's own answer. The calls, and where each checks:

| Call                                  | Checks the stop                                                                  |
| ------------------------------------- | -------------------------------------------------------------------------------- |
| `biome_map`                           | once, before its first lookup (a tile takes well under a tenth of a second)      |
| `height_map`                          | before each band of rows with a height to compute, the rows in one row of chunks |
| `surface_biome_map`, `flower_map`     | before each row with a pixel to compute                                          |
| `carving_map`                         | before each chunk it carves                                                      |
| `underground_map`                     | before each band of rows, and as `carving_map` when it carves                    |
| `map_features`                        | before each candidate structure start, and as below for features and loot        |
| `structures_in`                       | before each candidate structure start                                            |
| `features_in`                         | before each chunk it places, and before each neighbour that chunk carves         |
| `containers_in`, `containers_holding` | before each chunk it places, and before each neighbour that chunk carves         |

The `…_with` forms check where their own call does. With every pixel known, they never check, since they compute nothing ([reuse](#reuse-across-scales)). Heights and the underground probes are computed a band at a time. The terrain samples each chunk's columns together, whatever else it's asked for, so the answers are those of the whole map at once. What a world loads on first use (the jigsaw template pools that features and containers read structure pieces from) isn't interrupted: it happens once per world and later calls share it.

On the 26.3 Overworld in a release build, with the pools loaded, the slowest tiles stopped 11 to 56 ms after their stop was raised: heights at 1:64, every underground layer at 1:4, features (dungeons and geodes) at 1:2, and every container in 17 × 17 chunks.

## Testing

On fixture bundles, without game data: a view equals itself assembled from tiles at every scale, layers on, terrain layers included at scales 1 to 16 on sloping fixture terrain; each pixel's biome equals `Session::biome_at` at its sample block, and at the surface at its column's height; each height equals `Session::height_at`; relief lightens exactly the pixels higher than a western or northern neighbour on rising ground; columns with no blocks are the void wherever heights are drawn, at scales 1, 4 and 16, and never without heights; the features drawn are exactly those `structures_in`, `slime_chunks` and `spawn_origin` give for the area and its margin, confirmed or not; invalid areas, and underground layers past scale 4, are errors; on a fixture Nether, slime chunks, the spawn and terrain layers are errors and biomes draw. On a fixture with a noise cave, a copper vein rule and carvers: a view equals its tiles with every underground layer on, at scales 1 to 4, over biomes and grey; each pixel's vein, noise cave and carver agree with `Session::chunk_blocks` at its corner block, before and after carvers; each pixel's colour is the specified blend; a carving map gives the same pixels at another height, and is refused for another area; veins in a dimension without them are an error. `spyglass render` writes the same bytes as `Session::render` for the same request. Ignored tests on the real bundle (`cargo xtask oracle-test`) check tiling and sampling there too, terrain layers, underground layers and confirmed markers included, and `Biomes::biome_grid` against `biome_at`. In every supported version, the underground layers' samples (the noise fill's block at a y, veins at a position, and carvings) are checked against whole chunks in `spyglass-blocks`, before and after carvers, in the Overworld and the Nether. The heights a map samples are checked against the game by the height golden test, which samples a sparse grid the same way ([terrain.md](terrain.md)). Every call made through `Session::stopping` gives the session's answer while its stop isn't raised, and `Error::Stopped` when it is, on a fixture at scales 1, 4 and 16; an ignored test on the real bundle checks the slowest tiles stop within half a second of the stop being raised mid-call. Every map made `…_with` pixels known from maps at another scale equals the map made directly, on fixtures at several seeds and scales 1 to 16 (1 to 4 underground), from a finer area with its edge, the west part of one and a coarser one, ties and veins included; with every pixel known it's made with its stop raised, so nothing was computed. An ignored test does the same on the real bundle, flowers included.
