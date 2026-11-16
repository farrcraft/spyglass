# 30. Nether portal linking

- Status: In progress
- Depends on: [08](completed/08-dimensions-and-versions.md), [17](completed/17-markers-and-saved-seeds.md), [25](completed/25-what-the-save-knows.md)
- Plans: [m30-portal-linking](../plans/m30-portal-linking.md)

## Goal

Show where a Nether portal leads, and where a portal can be built so that it links, or doesn't, to one that exists. Players move and pair portals by trial and error, and a mislink is found only by walking through. Which existing portal a trip lands at is the game's portal search: the entry position scaled by the dimensions' coordinate scales, then the nearest portal within a search square in the other dimension. That needs no blocks, so spyglass can answer it exactly, for portals a user marks on the map and for those a save already has.

## Scope

- **Linking in the library.** For a position in a portal in one dimension, the target in the other: scaled by the dimension types' `coordinate_scale` from the bundle (and so from data packs), and clamped to the world border. Only x and z are scaled and clamped: y is the entity's own, and the search covers every height. Then the portal it lands at: the game's search area around the target, its order (distance first, then how the game breaks ties) and its limits, ported from the game's portal code. A portal is an area of blocks, not a point, so the answer covers every block a player can enter from.
- **Portal markers in the viewer.** A marker kind for a portal, placed in either dimension. Switching to the other dimension shows where it leads: the target, the portal it links to if any marker or save portal is in range, and the search area around the target.
- **Where a portal must go to link.** An overlay, in the source dimension, of the area where a new portal would link to a chosen portal on the other side, and where it would link to another one or to none. For moving a portal: build anywhere in the shaded area and the link stays. With several portals the areas split by nearest, as the game's search does.
- **Portals from a save.** The portals a save already has, from its points-of-interest files (`poi/`), as markers in both dimensions, with each one's link. Portals that don't link back to each other (A leads to B, but B leads to C) are flagged.
- **The command.** `spyglass portal link` from a position or a save's portals, with `--json`.
- **Data.** The user-data spec gains the portal marker kind, as a compatible change to the file's format: older files still load.
- **Untrusted input:** the points-of-interest reader keeps [hardening](completed/10-hardening.md)'s limits, and a fuzz target covers it.

Not in scope:

- Where the game builds a new portal when none is in range: it searches the blocks around the target for room, which needs the world's blocks after features ([decoration order](completed/27-decoration-order.md)). A later milestone could answer it as certain or a candidate, as features are.
- End portals and gateways.
- Writing portals into a save.

## Exit criteria

- [ ] Links from many entry positions, with portals placed at random in both dimensions, match the oracle's real server for 26.2, 26.3 and the 26.4 snapshots, including ties and the edges of the search area.
- [ ] A portal marker placed in one dimension shows its target, link and search area in the other, and the overlay shows where a new portal keeps the link.
- [ ] A real 26.3 save's portals appear with their links, and a mislinked pair is flagged.
- [ ] A new spec covers linking, the [user-data spec](../specs/user-data.md) covers the marker kind, and the user guide describes portal markers, the overlay and the command.
