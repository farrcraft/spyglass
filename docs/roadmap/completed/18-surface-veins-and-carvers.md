# 18. Surface rules, ore veins, caves and carvers

- Status: Completed
- Depends on: [09](09-fast-density.md)
- Plans, in order: [m18-block-oracle](../../plans/completed/m18-block-oracle.md), [m18-material-rule](../../plans/completed/m18-material-rule.md), [m18-carvers](../../plans/completed/m18-carvers.md), [m18-layers](../../plans/completed/m18-layers.md)

## Goal

The blocks of a chunk before features: the material rule (surface rules), ore veins, the noise caves already in the density graph, and the carvers that cut caves and ravines afterwards. Each is exact and checked against the game, and each can be drawn as a map layer at a chosen height. This is what features ([19](../19-features.md)) stand on: dungeons check for air and solid blocks, and wells and fossils check the surface, so they can't be placed exactly without it.

## Scope

- **The material rule:** 26.3's surface rules, as data from the bundle with the algorithm in code, giving each solid position its block. This includes the vein rules, frozen-ocean holes and swamp water that terrain.md lists as coming later.
- **Ore veins** in 26.3 and later, from the material rule's vein rules. 26.2's are already in its columns.
- **Carvers:** the configured carvers that biomes list (caves, canyons, and the Nether's), their per-chunk randomness, and the blocks they remove, including the aquifer's fluids at carved positions.
- **Queries:** a block at a position, and a column of blocks, after the material rule and carvers. Both are in the API, with `spyglass` CLI equivalents.
- **Map layers:** ore veins, caves (noise caves and carvers, kept separate) and ravines, at a height the user picks, with a "surface", "underground" or "bottom" preset, like the biome height. They're drawn at scales up to a limit the plan sets from measured cost.
- **The oracle:** golden block columns after the material rule and after carvers, for the golden seeds and dimensions, in every supported version.
- **Specs:** terrain.md, or new specs beside it, for the material rule, veins and carvers, with per-version notes and switches as [ADR 0026](../../adr/0026-how-game-versions-coexist.md) describes.

Not in scope:

- Features, and blocks placed by structures.
- Lighting, and anything decided after generation.

## Exit criteria

- [x] Every golden block column matches the game after the material rule and after carvers, in each dimension, for every supported version (`cargo xtask oracle-test`). One 26.2 Nether block is an open question ([carvers.md](../../specs/carvers.md#open-questions)).
- [x] The new layers tile exactly: a view equals itself assembled from tiles (the render spec's tiling test, extended).
- [x] The layers' cost per tile at their largest scale is recorded with `cargo xtask bench`, and the viewer stays responsive while they draw.
- [x] `cargo xtask check` passes.
