# 25. What a save already knows

- Status: Planned
- Depends on: [04](completed/04-world-saves.md), [20](completed/20-loot.md)
- Plans: none yet

## Goal

For a world save, spyglass uses what the save holds where the world is already generated, and shows where that is. Today it predicts every chunk from the seed, and only counts a save's chunks to warn about older versions ([world-saves.md](../specs/world-saves.md#region-files)). MCToolKit's chunk map and its pruner's per-chunk times suggested this ([prior art](../research/prior-art.md)).

## Scope

- **Generated chunks, on the map.** A viewer layer, and `spyglass render --generated`, shading the chunks a save has in each dimension, with chunks an older version generated marked. The region files' location tables say which chunks exist without reading them; versions come from the scan that already runs.
- **Structures from the save.** A generated chunk stores its structure starts and references. Where a structure's start chunk is generated, its answer comes from the save: certain, even in a world whose data packs or mods spyglass can't predict.
- **Loot from the save.** A generated container that still has its `LootTable` hasn't been opened, so the prediction stands. One without it has been opened, and the answer says so, with the contents the save holds.
- **How long chunks were played** (`InhabitedTime`) as an optional shading, if it costs little once chunks are read anyway.
- **Untrusted input:** the chunk reading keeps [hardening](completed/10-hardening.md)'s limits, and the region fuzz target covers it.

Not in scope:

- Writing to saves, ever.
- Reading blocks from a save in place of generating them.

## Exit criteria

- [ ] The viewer and `spyglass render` show a save's generated chunks, checked against a real 26.3 save.
- [ ] `locate structure --world` gives a save's own structure starts where they're generated, and says that's where the answer came from.
- [ ] Loot answers in a save say when a container has been opened, and give what the save holds.
- [ ] The user guide's [your own worlds](../guide/src/your-worlds.md) page describes all three.
