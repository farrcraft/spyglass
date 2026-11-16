# m30. Nether portal linking

- Status: In progress
- Milestone: [30. Nether portal linking](../roadmap/30-portal-linking.md)
- Started: 2026-11-15
- Completed: not yet

## Goal

Answer where a Nether portal leads: the block a trip aims at, the existing portal it lands at, and where a portal can be built so it links to a chosen one. Answer it for portals the user marks and for those a save already has.

This plan covers all four of the milestone's exit criteria:

- links from many entry positions match the game's own running server, ties and the search area's edges included;
- portal markers in the viewer, with the target, link and search area in the other dimension, and the overlay of where a new portal keeps a link;
- a real 26.3 save's portals with their links, and a mislinked pair flagged;
- a new spec, the user-data spec's marker kind, and the user guide.

It covers every supported version: 26.2, 26.3 and the 26.4 snapshots. The portal code is the same in 26.2, 26.3 and 26.4-snapshot-8, apart from a spectator check (26.3) and the border's clamp function (26.4).

## What the game does

[portals.md](../specs/portals.md) has it all, from the 26.2, 26.3 and 26.4-snapshot-8 bytecode. What shapes the work:

- **The target** scales the entity's own position, not the portal block's, and clamps x and z to the target dimension's border. y is neither scaled nor clamped.
- **The other dimension** is the Overworld from the Nether, and the Nether from anywhere else, **the End included**. `Game::convert` refuses End links, so `portal_target` doesn't use it.
- **The search** takes portal POIs within 16 blocks (into the Nether) or 128 (otherwise) in x and z, at any height, inside the border and still portal blocks. The nearest by `distSqr` wins, then the lowest y, then stream order.
- **Ties** after that resolve across chunks (`rangeClosed` walks z, then x). Within one section the order is a `HashSet`'s and isn't known. A tie between blocks of one portal is no tie for the link.
- **Entering:** the portal block's inside shape is the whole block, and the frame confines the player along the portal and up. A standing player in a 2 × 3 portal spans 12 × 14 × 2 exit blocks out of the Nether, and one or two each way into it.
- **The border** is each dimension's own `world_border` saved data, in 26.2 too, at `dimensions/<ns>/<path>/data/minecraft/world_border.dat`.
- **Points of interest:** a section missing or not `Valid` is rebuilt from the chunk's blocks when the search loads the chunk. A missing section counts as well as an invalid one.

## Approach

Five parts, each building on the one before: the link in the library, the oracle, portals from a save, the overlay, and the front ends with the guide.

### Linking in the library

- **Where:** a `portals` module in `spyglass`. It needs the game's coordinate scales and dimension heights (`Game`), not a seed or a generator. The POI and world-border readers go in `spyglass-world`, beside `region.rs`. No new crate.
- **Types:**
  - `Portal`: a dimension, an axis, and its blocks. It's either a rectangle (a marker: a lower corner, an axis, a width and a height, 2 × 3 by default) or a set of POI blocks (from a save, grouped into rectangles by axis and adjacency).
  - `Border`: a centre and a size, the vanilla default unless a save gives one.
- **The target:** `Game::portal_target(from, [x, y, z]: [f64; 3], border) -> PortalTarget` (the dimension and the exit block), [the spec's target](../specs/portals.md#the-target) in `f64`, with the border's `9.999999747378752E-6`. The target dimension is the Nether from the End too. It isn't `Game::convert`, which floors whole block columns and refuses the End. The two agree on block corners between the Overworld and the Nether, and a test says so.
- **The search:** `portal_link(to, exit, portals, border) -> Link`. It filters candidates to the square, the border and the target dimension, then takes the minimum by `distSqr` and then lowest y.
  - A tie that remains goes by [the spec's tie rules](../specs/portals.md#ties): one portal's blocks only is certain; otherwise the earliest chunk in `rangeClosed`'s order decides; within that chunk, more than one portal stays tied, as tied biomes are.
  - `Link` is `Lands(Landing)`, `Tied(Vec<Landing>)` or `NewPortal` (the game would build one). A `Landing` is a portal's index in the slice searched and the block of it the search picks.
  - Each portal has one candidate, its block nearest the exit, so the search is one pass over the portals, not their blocks.
- **From a portal, not a point:** `portal_links(portal, portals, border) -> PortalLinks`, over [the spec's standing range](../specs/portals.md#entering-a-portal): inside the frame along the portal and up, and overlapping its column by more than `9.999999747378752E-6` across it.
  - `PortalLinks` maps that entry range, split where the exit block changes, to each piece's exit block and its `Link`. Pieces with the same link merge for display.
  - Into the Nether a portal has one to eight exit blocks (one or two each way). Out of it, a 2 × 3 portal has 12 × 14 × 2 = 336, so `portal_links` computes each exit column's candidates once and reuses them for both heights.
  - The answer says it's for a standing player: one moving through the portal can aim up to a tick's movement beyond it.

### The oracle

- **On a running server.** The harness starts the stock server in its own JVM and reaches it as `Join` does ([ADR 0073](../adr/0073-the-oracle-joins-a-stand-in-player.md)), with the EULA rule of [ADR 0033](../adr/0033-the-oracle-accepts-the-eula-only-when-asked.md). Everything below is the game's code:
  - it places portal blocks with `level.setBlock` in force-loaded chunks, which adds their POIs as play does;
  - it sets each dimension's border where a case needs one;
  - it summons a stand-in entity at each entry position and calls `NetherPortalBlock.getPortalDestination`.

  A case that finds no portal makes the game build one. The harness records `none` and then removes what was built, with `setBlock` to air, which removes its POIs too. A new ADR records this, since it's the first oracle command that drives gameplay code rather than generation.

- **`cargo xtask oracle portals [--seed S]… [--random N] [--world SAVE]…`**, not part of `golden`, as `locate` and `order` aren't. For each seed it writes `oracle-out/<version>/portals/<seed>.json`, with the portals it placed and, for each case, the entry dimension and position, the exit block and the portal block linked (or `none`). The cases are:
  - **Random layouts:** portals at random in both dimensions, and entry positions inside each portal, its edges included.
  - **Constructed edges:**
    - candidates exactly at `±radius` and one past it;
    - ties of distance broken by y;
    - ties of distance and y across chunks and within one section;
    - exits clamped by a small border, and a candidate just outside it;
    - a portal block removed after its POI was written;
    - both scales of a data pack's dimension type (with `--pack`).
- **The comparison** is an ignored test, listed in `xtask/src/oracle_test.rs`:
  - every certain answer must match the game;
  - every tied answer must contain the game's;
  - each kind of constructed case must occur, or the test fails.

### Portals from a save

- **Reading POIs:** `spyglass_world::poi::read(dir) -> PoiIndex`. It reads `nether_portal` records by chunk under [world-saves.md](../specs/world-saves.md#limits)'s limits and `region.rs`'s rules for unreadable chunks, and keeps each section's `Valid`.
- **Sections that aren't valid** are filled from the chunk's blocks, as the game would on load: a section whose block palette has no `nether_portal` is skipped, and otherwise its paletted container is read for them. If that turns out too costly or too rare to justify, the dimension's answer notes that those sections' portals may be missing. The spec records whichever is chosen.
- **The border:** each dimension's `world_border` saved data, or the default when there's none.
- **The API:** `Session::with_save` (or `SaveReport`) gives `save_portals(dimension) -> Vec<Portal>`, grouped into portals. `save_portal_links(save) -> Vec<SavePortalLink>` links each portal both ways and flags:
  - **mislinked:** A leads to B, but B doesn't lead back to A;
  - **split:** different parts of A lead to different portals;
  - **unlinked:** A has no portal in range, so the game would build one.

### Where a portal must go to link

- **`portal_link_map(source, area, y, portals, border) -> LinkMap`:** for each pixel of an area in the source dimension, at height `y`, which portal on the other side a portal block there would link to, or `None`, or a tie.
  - The work is done on the target side. Each exit block's link at `y` is computed once for the area's image under the scale, then mapped back.
  - So a source area of 1:1 pixels into the Nether costs one link per 64 pixels.
- **Shading:** one colour per destination portal, a hatch for "none", and the tied checker. With a chosen destination portal, every other region is dimmed. That's the "build anywhere in the shaded area and the link stays" view.
- **Where it's drawn:** the viewer draws the map, not `render`, since it depends on the user's markers, not on the seed. [render.md](../specs/render.md) still gets the colours if `render` grows a `--portals` option later.
- **One height or all?** The overlay is drawn at the viewer's own height control, as the flower layer is. The link can change with y, since `distSqr` counts dy.

### Front ends and the guide

- **User data:** markers gain an optional `kind`, `pin` by default, or `portal`. A portal marker also has `axis` (`x` or `z`), `width` and `height`, with defaults, and needs `y`. Older viewers keep these as unknown fields and show the marker as a pin. The format stays 1, since older files still load and nothing they hold changes meaning.
- **Viewer:**
  - **Placing:** "Add portal here" in the map's right-click menu, and a kind and axis control in the markers panel.
  - **In the other dimension:** each portal marker's exit range, the search square around it, and a line to the portal it links to, or a "new portal" note when none is in range.
  - **Save portals:** shown as portal icons in both dimensions, with mislinked, split and unlinked ones flagged, and the reason given on hover.
  - **The overlay:** a "Link area" action on a portal marker or save portal draws the link map in the other dimension.
- **CLI:**
  - `spyglass portal link <dimension> X Y Z [--axis x|z] [--size WxH] [--portal DIMENSION X Y Z [--axis] [--size]]… [--world SAVE]`: the exit range from a portal at that position, the search square, and the portal each part lands at, against the portals given and the save's.
  - `spyglass portal link --world SAVE`, with no position: every save portal's links, with the flags.
  - Both take `--json`, in [cli-json.md](../specs/cli-json.md)'s style. No seed is needed, only a version (and a save's packs, for scales a pack changes).
- **Guide:** a portals page, with an example of moving a base's portal without breaking its link and finding a mislinked pair in a save. It also adds the marker kind to [viewer.md](../guide/src/viewer.md), the command to [command.md](../guide/src/command.md), and save portals to [your-worlds.md](../guide/src/your-worlds.md).

## Tasks

- [x] Write `docs/specs/portals.md` from the 26.3 bytecode, and check 26.2 and 26.4-snapshot-8 against it (compared with `javap` directly, since no code maps to it yet for `version-diff --code`). It covers:
  - the target: the scale, the border clamp, the entity's position, y;
  - the search: the radius by dimension, `rangeClosed`'s order, the square, the filters, the order and the stream's ties;
  - the entry range a player can teleport from;
  - the world border's data and default.
- [x] `portal_target` and `portal_link` in `spyglass::portals`, with unit tests: a corner agrees with `Game::convert`, the End leads to the Nether, the border clamp, the square's edges, y breaking ties, ties across chunks resolved, and between portals in one section left tied. (A tie within one portal can't happen: [the spec](../specs/portals.md#ties) says why.)
- [x] `portal_links` over a portal's entry range (`Portal::standing_range`), with `PortalLinks::outcomes` and `only` for split and single links.
- [x] Oracle: the `portals` command on a running server, its cases and file, and the comparison test. Add an ADR for driving gameplay code and cleaning up built portals.
- [x] `spyglass_world::poi` reader and the `world_border` reader, with tests on hand-made files, and a `poi` fuzz target under `fuzz/` ([ADR 0036](../adr/0036-fuzzing-with-cargo-fuzz.md)).
- [x] Sections that aren't `Valid`: read portals from the chunk's blocks, or note that they may be missing. Record which in the spec. (Noted, not read: [the spec](../specs/portals.md#points-of-interest) says why.)
- [x] `save_portals` and `save_portal_links`, with the mislinked, split and unlinked flags (`SaveReport::portals`, `Game::save_portal_links`), tested on a save built in the test.
- [ ] Check them on a real 26.3 save with a mislinked pair built on purpose, against `oracle portals --world`.
- [x] `portal_link_map`, with a test that it agrees with `portal_link` at every pixel.
- [x] User data: the marker `kind`, `axis`, `width` and `height`, read and written leniently, and a test that a file with portal markers round-trips through an older reader's unknown-field path. Update [user-data.md](../specs/user-data.md).
- [x] CLI: `portal link`, from a position and from a save, with `--json`. Update [cli-json.md](../specs/cli-json.md).
- [x] Viewer: placing portal markers, the target, square and link in the other dimension, save portals and their flags, and the Link area overlay.
- [x] Benchmarks: a full-view link map and a large save's `save_portal_links` added to `cargo xtask bench`, with their times in the spec.
- [x] User guide: the portals page, and [viewer.md](../guide/src/viewer.md), [command.md](../guide/src/command.md) and [your-worlds.md](../guide/src/your-worlds.md).
- [x] Update the milestone's scope (y isn't clamped) and its exit criteria (26.2 too).

## Verification

- **Oracle:** `cargo xtask oracle portals` matches spyglass's answers for 26.2, 26.3 and 26.4-snapshot-8 (`cargo xtask oracle-test`):
  - every certain link equals the game's;
  - every tie contains it;
  - the test fails if a kind of constructed case is missing.
- **Save:** on a real 26.3 save, the portals spyglass reads are the POIs the server loads, and their links match `oracle portals --world` on a copy of the save. A pair built to mislink is flagged.
- **Link map:** it agrees with `portal_link` at every pixel. In the viewer, building a portal marker inside the shaded area keeps the link, and building one just outside breaks it.
- **Untrusted input:** the `poi` fuzz target runs clean for a while (`cargo xtask fuzz poi`), and oversized or truncated chunks are unreadable, not errors.
- **By eye:** markers, the square, the link line and the overlay in the viewer, checked with the `run-viewer` skill.
- `cargo xtask check` passes.

## Durable outputs

- [x] `docs/specs/portals.md`: the target, the search, its ties, the entry range and the border. Add it to the [specs index](../specs/README.md).
- [ ] [world-saves.md](../specs/world-saves.md): `poi/` files, sections that aren't valid, the world border's data, and their limits.
- [x] [user-data.md](../specs/user-data.md): the portal marker kind.
- [x] [cli-json.md](../specs/cli-json.md): `portal link`.
- [x] [oracle.md](../specs/oracle.md): the `portals` command and its files.
- [x] An ADR ([0076](../adr/0076-the-oracle-drives-the-game-s-portal-code.md)): the oracle drives the game's portal code on a running server and removes the portals it makes it build.
- [x] The user guide's portals page.

## Notes

Newest first.

- 2026-11-16: Benchmarks are in `cargo xtask bench` (`portal_link_map` both ways, a full view and the viewer's link area, and `save_portal_links`), among 5,202 synthetic portals, with times in [the spec](../specs/portals.md#where-a-portal-links). The first run found the link map filtering portals for the whole area, which out of the Nether keeps nearly all of them: 16.8 s for a full view. It now filters per 16 × 16-pixel tile: 0.86 s, and 84 ms for the viewer's link area.
- 2026-11-16: The viewer's portals and the guide's [portals page](../guide/src/portals.md) are done. Checked by eye with `run-viewer` on seed 12345 and this machine's 26.3 save: a portal marker placed from the map's menu, its link area in the Overworld (green over its search square, red elsewhere), an Overworld portal added inside it and the link line both ways, and the save's 11 portals with save portal #10 drawn as unlinked. How it's built:
  - Everything is in `app/map/portal_layer.rs`: portal markers and save portals are one list of groups, linked with `Game::portal_links` and `outcomes_by`, worked out again only when the portals change. The save's portals and flags are read on a thread when a save opens.
  - Portals are footprints under the markers, so a portal marker is still a pin for editing, and its footprint and links come with it. The flags are the save's own (`save_portal_links`), among its portals only, since markers aren't real.
  - The link area is a texture at most 320 pixels a side, coarser than the view when needed, worked out on a thread for the view and redrawn when it's done. Its notice over the map explains the shades and hides it.
- 2026-11-16: `spyglass portal link` is done, from a position and from a save, with `--json` ([cli-json.md](../specs/cli-json.md#portal-link)) and a section in the guide's [command.md](../guide/src/command.md#portals). What changed from the draft:
  - The position is `DIMENSION x,y,z`, as other commands take blocks, and `--portal` is one value, `DIMENSION:x,y,z[:AXIS][:WxH]`, since clap can't give a repeated option options of its own.
  - Its JSON header has no `seed` or `dimension`, since links need neither, and its `warnings` are the save's portal notes, not its worldgen warnings, which don't touch links.
  - `PortalLinks::outcomes_by` groups outcomes by owner (a `--portal` or a save portal), and `SaveLinkOutcome` became `LinkOutcome`.
  - On this machine's 26.3 save, it lists the 11 portals and the one unlinked, in about a second.
- 2026-11-16: Portal markers in user data are done ([the spec](../specs/user-data.md#portal-markers)): `Marker::kind` is `MarkerKind::Pin` or `Portal { axis, width, height }`, and `Marker::portal` gives the `spyglass::Portal`. A pin writes no `kind`, so files without portals are written as before, and a kind this viewer doesn't know stays in the unknown fields, shown as a pin, and is written back. A portal without a `y` is read the same way, and reported.
- 2026-11-16: `Game::portal_link_map` is done ([the spec](../specs/portals.md#where-a-portal-links)), with a test that every pixel equals `portal_link` from its block's exits, in both directions and at two scales. What changed from the draft:
  - A pixel's answer is a `Destination`: a portal, tied portals or a new portal, not the block of it the search picks. Otherwise one block between two portals gave four answers.
  - It's a `Game` method taking a `MapArea`, whose `y` is the portal's height, as the flower map's is.
  - A pixel's block is the player's position, not a standing range. Out of the Nether a block aims at 8 × 8 exits, and a block whose exits disagree is **mixed**, with each destination.
  - A 1024 × 768 view takes about 50 ms into the Nether and 0.9 s out of it (64 searches a pixel, nothing shared). The benchmarks task can speed that up if the viewer needs it.
- 2026-11-16: `SaveReport::portals` and `Game::save_portal_links` are done ([the spec](../specs/portals.md#a-saves-portals)), with tests on the fixture bundle and a save built in the test, and an ignored test over this machine's saves. What changed from the draft:
  - A save's records are grouped by shared faces and cut into rectangles, since a record doesn't say its axis and blocks placed by commands needn't be one. A lit portal is one rectangle.
  - `Game::portal_links` searches only the portals near its exits, so linking a save is one pass over its portals per portal, not over all of them for every exit.
  - Flags: `tied` and `too-low` beside the plan's three. A portal leading out of the End is never mislinked, since the Nether's portals lead to the Overworld.
  - `Game::build_height` gives each dimension's `min_y` and `height`, for the `poi/` sections the game reads.
  - On this machine, one 26.3 save has 11 portals, read and linked in about a second. One Nether portal is unlinked: its exit is about 190 blocks from the nearest Overworld portal. No save has a mislinked pair; that needs one built on purpose.

- 2026-11-15: Portals aren't read from blocks. The game saves no empty point-of-interest data, so 97 to 100% of full chunks in this machine's saves, from 1.18 to 26.3, have no `poi/` chunk, and a missing section is the norm. Only chunks last saved before 1.16 (rebuilt on load) can hold portals the records don't list; `save_portals` counts them in a warning instead.
- 2026-11-15: `spyglass_world::poi::read` and `Save::world_border` are done, with tests on hand-made files, a `poi` fuzz target (2 minutes clean) and an ignored test over this machine's saves: 88 `poi/` folders, 26,906 chunks and 107,818 portal records, all readable. From the bytecode:
  - The game reads a POI section leniently: a record its codec rejects is skipped and the rest kept, and only section keys inside the dimension's height, as decimal text, are read.
  - `ForcePoiRebuild` (1.16) rebuilds every section of an older chunk, and a chunk without `DataVersion` counts as 1945, so none of those sections is valid.
  - A save from before 26.2 keeps one border in `level.dat`. The game copies it to the three vanilla dimensions, the Nether's centre divided by 8 (`LevelDatToSavedDataFileFix`). Older saves store 6E7, the old default size.

- 2026-11-15: The oracle's `portals` command, its harness (`spyglass.oracle.Portals`), the comparison test and [ADR 0076](../adr/0076-the-oracle-drives-the-game-s-portal-code.md) are done. spyglass matches the game for 26.2, 26.3 and 26.4-snapshot-8 (70 layouts each, about 20 s a run), and for 400 random layouts on 26.3 (17,614 answers). What changed from the draft:
  - The harness records the exit block and the block found through a `PortalForcer` subclass put in each level by reflection, since `getPortalDestination` returns only where the entity lands.
  - Searches (an exit given, `findClosestPortalPosition`) and trips (a position, `getPortalDestination`) are separate case kinds, so the search's edges don't build portals.
  - The game adds a block's record in a later server task, so a layout is set up, answered and undone in three tasks.
  - One file, `portals/portals.json`, not one per seed: the Overworld is a void superflat and the seed doesn't matter. `--pack` and `--world` are left for later; the data-pack scale case with them.
- 2026-11-15: `spyglass::portals` has the border, `Portal`, `Game::portal_target`, `portal_link` and `Game::portal_links`, with 16 tests. The exit block is a whole block, so a portal's nearest block is unique, and ties within one portal (which the draft expected between a portal's columns) can't happen. Portal markers are rectangles of 1 to 21 blocks each way, checked by `Portal::new` (`Error::InvalidPortal`). A save's portals will need grouping into rectangles.
- 2026-11-15: Wrote [portals.md](../specs/portals.md) from the bytecode. What changed from the draft plan:
  - A portal in the End leads to the Nether, so the target can't use `Game::convert`.
  - The portal block's inside shape is the whole block, and the frame confines a standing player along the portal and up. A 2 × 3 portal spans 12 × 14 × 2 exit blocks out of the Nether, not about 17.
  - `PoiRecord` hashes by `BlockPos`, so section order is deterministic but depends on the set's history. A tie between blocks of one portal is no tie for the link, which leaves only ties between portals in one section.
  - The border is per dimension in 26.2 as well, in `data/minecraft/world_border.dat` under each dimension's folder.
  - A missing POI section is rebuilt from blocks just as an invalid one is.
- 2026-11-15: Read the 26.3 portal code, and compared `findClosestPortalPosition` in 26.2 and 26.4-snapshot-8. Three things the milestone didn't say:
  - The scaled position is the entity's, not the portal block's, so out of the Nether one portal block spans 8 target blocks.
  - y is neither scaled nor clamped.
  - Ties after distance and y go to stream order, which within a section is a `HashSet`'s, so some ties can't be broken from the portals alone.
