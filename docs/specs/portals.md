# Portals

Status: draft. Covers where a Nether portal sends an entity: the block a trip aims at (`NetherPortalBlock.getPortalDestination`), the existing portal it lands at (`PortalForcer.findClosestPortalPosition`), the positions inside a portal a player can teleport from, and the world border both depend on. Where the game builds a portal when none is found (`PortalForcer.createPortal`) isn't covered: it needs the world's blocks after features.

Ported from the 26.2, 26.3 and 26.4-snapshot-8 bytecode. They agree unless a paragraph says otherwise. The answers are checked against the game's own code on its server ([oracle.md](oracle.md#portal-files), [ADR 0076](../adr/0076-the-oracle-drives-the-game-s-portal-code.md)).

## The target

`NetherPortalBlock.getPortalDestination(level, entity, entryPos)` runs when an entity has been inside a portal long enough ([Entering a portal](#entering-a-portal)):

1. **The other dimension** is the Overworld when `level` is the Nether, and the Nether from any other dimension, the End and a data pack's dimensions included. If that dimension doesn't exist, nothing happens.
2. **The scale** is `DimensionType.getTeleportationScale(from, to)`: from's `coordinate_scale` divided by to's, in `double`. A data pack that changes a dimension type's `coordinate_scale` changes it ([data-packs.md](data-packs.md)).
3. **The exit block** is `to`'s border's `clampToBounds(entity.getX() · scale, entity.getY(), entity.getZ() · scale)`:
   - x is clamped to `[minX, maxX − 9.999999747378752E-6]` and z to `[minZ, maxZ − 9.999999747378752E-6]`, with `to`'s [border](#the-world-border) (the constant is the `float` `1.0E-5f`, widened). The clamp is `Mth.clamp` in 26.2 and 26.3 and `Math.clamp` in 26.4-snapshot-8; they agree for these arguments.
   - `BlockPos.containing` then floors each coordinate.

Two things follow:

- **The entity's own position is scaled, not the portal block's.** `entryPos` isn't used for the target. Out of the Nether, the span of positions a player can stand at in a 2 × 3 portal covers 12 exit blocks along the portal and 14 across it ([Entering a portal](#entering-a-portal)), and where the player stands picks the exit block within that span.
- **y is neither scaled nor clamped.** The exit block's y is the entity's own, floored. The [search](#the-search) covers every height, so a y outside the target dimension's build height still finds portals; it only changes their distance.

On block corners (`x` and `z` whole numbers), the target's x and z are what [`Game::convert`](search.md#dimensions) gives for that column, inside the border.

## The search

`PortalForcer.findClosestPortalPosition(exit, toNether, border)` looks for an existing portal block near the exit block, in the target dimension:

1. **The radius** is 16 when the target dimension is the Nether (`level.dimension() == Level.NETHER`), and 128 otherwise. It's chosen by which dimension it is, not by its scale.
2. **Loading.** `PoiManager.ensureLoadedAndValid(level, exit, radius)` takes every chunk within `floorDiv(radius, 16)` chunks of the exit's chunk (a square, at every section height) and loads, to `ChunkStatus.EMPTY`, each one that has a POI section missing or not valid. Loading a chunk from disk rebuilds those sections from its blocks ([Points of interest](#points-of-interest)). A chunk that was never generated has no blocks and so no portals.
3. **Candidates.** `PoiManager.getInSquare(nether_portal, exit, radius, ANY)` takes the chunks of `ChunkPos.rangeClosed(chunk of exit, floorDiv(radius, 16) + 1)` and, in each, every section from the lowest to the highest, and every `nether_portal` record in it. It keeps a record when `|x − exit.x| ≤ radius` and `|z − exit.z| ≤ radius`, at any height. The outer ring of chunks never holds a candidate: a block within the radius is at most `floorDiv(radius, 16)` chunks from the exit's.
4. **Filters,** in this order:
   - the record's block is inside the target dimension's border: `isWithinBounds(x, z)` with the block's lower corner, `minX ≤ x < maxX` and `minZ ≤ z < maxZ`;
   - the block there has the `horizontal_axis` property, which in vanilla only `nether_portal` has. A record whose block has been replaced is skipped.
5. **The answer** is `Stream.min` by `exit.distSqr(record)` (`dx² + dy² + dz²` in `double`, exact for any block in range), then by the lowest `y`. With no candidate, the game builds a portal (or, from 26.3, does nothing for a spectator).

### Ties

`Stream.min` keeps the first of equal elements, so a tie after distance and y goes to the first candidate in the stream's order:

- **Across chunks:** `rangeClosed` walks z from the lowest chunk to the highest, and within each z, x from lowest to highest. A tie between candidates in different chunks always goes to the one whose chunk comes first.
- **Across sections:** equal y means the same section, so this never decides a tie.
- **Within a section:** a section keeps its records as `Map<PoiType, Set<PoiRecord>>`, and each set is a `java.util.HashSet` keyed by `BlockPos.hashCode` (`(y + 31·z)·31 + x`). Its order follows the hash's bucket, which depends on the set's capacity, and so on how many records it has held since it was loaded or rebuilt, and then on insertion order within a bucket. That isn't known from the portals alone.

Within one portal, a tie can't happen. A portal's blocks fill a box, the square and the border cut it to a smaller box, and the block of a box nearest a whole-number exit is unique: the exit's coordinates clamped to the box's. Distances that differ in exact arithmetic could round to the same `double`, but only for exits more than 2²⁷ blocks (about 134 million) from the portal in y. Even then the blocks are the same portal, and `BlockUtil.getLargestRectangleAround` finds the same portal from any of them.

So each portal has one candidate, and a tie is always between portals. spyglass resolves it as follows:

1. The earliest chunk among the tied candidates, in `rangeClosed`'s order, holds the stream's first.
2. If that chunk holds one portal's candidate, that portal is the answer.
3. Otherwise the answer is tied between the portals in that chunk, which share a section.

A tie between two portals in one section, at equal distance and height, is rare.

### Once found

`BlockUtil.getLargestRectangleAround(record, axis, 21, Y, 21, sameState)` finds the portal around the chosen block, up to 21 × 21, and `getRelativePortalPosition` places the entity in it at the same relative position it had in the portal it left. That decides where in the portal the entity lands, not which portal.

## Entering a portal

A nether portal block has no collision. Its `getEntityInsideCollisionShape` is the default, `Shapes.block()`, so for teleporting the whole block counts, not the thin slab it's drawn as.

- **Being inside.** After each move, `Entity.checkInsideBlocks` takes the entity's box at its new position, deflated by `9.999999747378752E-6` on every side, and visits the blocks it overlaps: for an entity that hasn't moved, `BlockPos.betweenClosed` from the floored minimum corner to the floored maximum. For a moving entity it also visits the blocks swept along the move. A visited portal block calls `entityInside`, which sets the entity as inside the portal, if it can use portals.
- **Teleporting.** Each tick, `PortalProcessor.processPortalTeleportation` adds one to the entity's portal time if it was inside a portal since the last tick, and takes 4 off (down to 0) if not. When the time reaches the portal's transition time, `getPortalDestination` runs with the entity where it is then. For a player the time is a game rule, `players_nether_portal_creative_delay` when invulnerable and `players_nether_portal_default_delay` otherwise (80 ticks by default); for other entities it's 0, so they teleport on the tick after entering.

**A standing player's box** is `x ± h`, `y` to `y + H`, `z ± h`, with `h = 0.3f` (`0.6f / 2` in `float`, then widened: `0.30000001192092896`) and `H = 1.8f` (`1.7999999523162842`) standing, `1.5f` crouching, and `0.6f` swimming or gliding. For a portal along the x axis, with its blocks from `x0` to `x0 + w − 1` and `y0` to `y0 + h_p − 1`, in the column `z0`, and a full frame around it:

- **Along the portal,** `x0 + h ≤ x ≤ x0 + w − h`. A box that overlaps the portal's column by more than the deflation also overlaps the frame's, so the frame's collision keeps it between the frame's sides.
- **Up,** `y0 ≤ y ≤ y0 + h_p − H`, between the frame's bottom and top, for the same reason.
- **Across,** `z − h + d < z0 + 1` and `z + h − d ≥ z0`, with `d = 9.999999747378752E-6`: `z0 − h + d ≤ z < z0 + 1 + h − d`. So the player can stand on either side of the portal, overlapping its column by more than `d`.

For the z axis, swap x and z. A 2 × 3 portal (the smallest) leaves a standing player x in a span of 1.4 and y in 1.2, and z in a span of about 1.6. Times 8 out of the Nether, that's 12 exit blocks along the portal, 14 across and 2 heights. Into the Nether, divided by 8, it's one or two blocks each way.

**Moving entities** teleport from wherever they are when the time runs out. A player who passed through the portal during the last tick's move can be up to that move's length outside the range above, so a flying or falling player can aim beyond it. spyglass answers for the standing range, and says so.

## The world border

Each dimension has its own border, saved data of type `minecraft:world_border` (`WorldBorder.TYPE`), in 26.2 and later. Its fields are `center_x` and `center_z` (doubles, −2.9999984E7 to 2.9999984E7), `size` (default 5.9999968E7), `lerp_time`, `lerp_target`, `damage_per_block`, `safe_zone`, `warning_blocks` and `warning_time`.

With a static border (`lerp_time` 0), `minX = clamp(center_x − size / 2, −m, m)` and `maxX = clamp(center_x + size / 2, −m, m)`, and z likewise, where `m` is the server's `absoluteMaxSize` (`max-world-size` in `server.properties`, 29999984 by default). A border that's moving (`lerp_time` above 0) has bounds that change with time; spyglass uses its `lerp_target` and says so.

A dimension without the saved data has the default border: centre 0, 0, size 5.9999968E7, bounds ±29999984.

In a save it's `dimensions/<namespace>/<path>/data/minecraft/world_border.dat`. A save from before 26.2 has one border in `level.dat`, which the game copies to each dimension when it opens the save, the Nether's centre divided by 8 ([world-saves.md](world-saves.md#world-borders)).

## Points of interest

The game keeps each dimension's points of interest in region files beside its chunks, `poi/r.<x>.<z>.mca`, in the same region format. How spyglass reads them is in [world-saves.md](world-saves.md#points-of-interest). Each chunk is an NBT compound with `DataVersion` and `Sections`, keyed by section y as a string. Each section has `Valid` (a byte) and `Records`, a list of `{pos: [I; x, y, z], type: "minecraft:nether_portal", free_tickets}`.

A section is used as saved only when it's present and `Valid` is 1. Otherwise, when the game loads the chunk ([The search](#the-search), step 2), it rebuilds the section from the chunk's blocks: a section whose palette may hold a POI block is scanned, and each `nether_portal` block in it becomes a record. A section that is missing and holds no portal blocks stays empty.

On the server, placing a portal block adds its record (`PoiManager.add`) and replacing one removes it (`PoiManager.remove`), so a running server's records match its blocks.

**What spyglass does with missing sections.** The game doesn't save empty point-of-interest data: in this machine's saves (2026-11-15), 97 to 100% of full chunks in every version from 1.18 to 26.3 have no `poi/` chunk at all. A missing section is the norm, and in a chunk the game saved since 1.16 it holds no portal, since placing one adds its record and the record is saved with it. So spyglass reads no portals from blocks, and takes a missing section as empty. Portals the records don't list can only be in chunks last saved before 1.16 (data version 2218, or none), whose sections the game rebuilds on load (`ForcePoiRebuild`), and in chunks edited outside the game. A dimension's answer says how many chunks of the first kind it has, since their portals may be missing.

## Where a portal links

`Game::portal_link_map(source, area, portals, border)` answers, for each pixel of a [map area](render.md#map-areas) in `source`, where a Nether portal built at its corner block `(min.x + i·scale, y, min.z + j·scale)` would link, with `y` the area's. This is spyglass's question, not the game's: it's the [search](#the-search) from every exit block a position inside that block aims at, at height `y` ([The target](#the-target)), with `border` the target dimension's.

- **A pixel's answer** is a destination: a portal, a tie between portals, or none (the game builds one). Which block of the portal the search picks doesn't count. When the block's exit blocks don't all agree, the pixel is **mixed** and lists each destination, in order of exit block.
- **Into the Nether** a block aims at one exit block, shared by the 8 × 8 columns around it, so each exit's link is found once. **Out of the Nether** a block aims at 8 × 8 exit blocks of its own, and a block between two portals' areas is mixed.
- **Height matters:** the search counts y in its distance, so the map can change with `y`.
- **The player's position** is anywhere inside the block, not [the standing range](#entering-a-portal) of a portal built there, which also reaches into the frame's column across the portal. Building a portal entirely within pixels of one destination keeps that link for a player standing anywhere in its blocks.

It equals `portal_link` from each pixel's exit blocks, and a test says so. The map is worked out in tiles of 16 × 16 pixels, in parallel, each searching only the portals within the search square of its own exits. `cargo xtask bench` times it among 5,202 portals, one every 160 blocks over ±4,000 in the Overworld and every 20 over ±500 in the Nether (26.3, a desktop machine, 2026-11-16):

| Map                                      | Into the Nether | Out of the Nether |
| ---------------------------------------- | --------------- | ----------------- |
| 1024 × 768 at 1:1, a full view           | 39 ms           | 860 ms            |
| 320 × 240 at 1:4, the viewer's link area | 4 ms            | 84 ms             |

Out of the Nether each pixel is 64 searches, so it costs more. `Game::save_portal_links` on the same 5,202 portals takes 87 ms.

## A save's portals

`SaveReport::portals` reads each dimension's `nether_portal` records ([Points of interest](#points-of-interest)) and its [border](#the-world-border), and `Game::save_portal_links` links every portal both ways. This is spyglass's, not the game's: the game keeps records of blocks, and finds the portal around one only after the search picks it.

- **Grouping.** Records that share a face are one portal. A portal the game lit is a rectangle of 1 to 21 blocks each way, standing in one column. Other shapes (blocks placed by commands or editors) are cut into rectangles: from the lowest block left (by y, then z, then x), a rectangle runs along x or z, whichever row is longer (x when equal), then up while the whole row continues, at most 21 each way. A lit portal starts at its lowest corner, so it's one rectangle. The search takes each rectangle as a portal, and a tie between rectangles of one save portal lands at that portal.
- **Links.** For each portal, every rectangle a player can stand in gives its exit blocks and their links ([Entering a portal](#entering-a-portal)), against every rectangle in the save and the target dimension's border. The outcomes count exit blocks by where they land.
- **Flags:**
  - **unlinked:** every exit leads to no portal, so the game builds one;
  - **split:** exits lead to more than one place, a new portal counting as one;
  - **tied:** some exits tie between portals the game picks by an order spyglass can't know ([Ties](#ties));
  - **mislinked:** every exit leads to one portal B, B's trips come back to this portal's dimension, and no exit of B leads back to this portal. From the End a portal leads to the Nether, whose portals lead to the Overworld, so it's never mislinked;
  - **too low:** no rectangle is tall enough for a standing player.
- **Notes,** by dimension: `poi/` chunks or files that can't be read, chunks last saved before 1.16 (counted from the region files, whose portals may be missing, as [Points of interest](#points-of-interest) says), sections not marked valid that hold portal records (the game rebuilds them from the blocks, and spyglass uses the records), and a border that's moving or can't be read.

The border's `absoluteMaxSize` is the default, 29999984: a single-player save has no `server.properties`.
