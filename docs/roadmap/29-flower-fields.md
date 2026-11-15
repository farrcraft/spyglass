# 29. Flower fields

- Status: Not started
- Depends on: [19](completed/19-features.md)
- Plans: none yet

## Goal

Find where a chosen flower grows, for a player building a flower farm. In some biomes the flower a grass block grows is decided by a noise of the block's position, so the field of one flower has a shape, and that shape changes with height. A farm's platform can be built at any height, so the answer that matters is not where flowers stand in the generated world but which flower bone meal grows on grass at a chosen block, at any height. spyglass answers it on the map, for a point, and as a search.

## Scope

- **The flower at a block.** For any block and height, the flower that bone-mealing grass there grows: the biome's flower feature, sampled at that position as the game samples it. The flower features' state providers come from the bundle (`noise_provider`, `noise_threshold_provider`, `dual_noise_provider`, `weighted_state_provider` and others in vanilla's flower features); a spec covers each one this needs. A provider that draws at random, rather than by position, answers with its odds instead of one flower.
- **Height is part of the question.** The providers' noises and the biome lookup are both three-dimensional, so the flower at a column can change with its height, and so can the biome (a platform high above a flower forest, or a farm underground). Every answer takes a height. The biome is the one at that block, not at the surface.
- **A flower layer in the viewer.** The flower at each block of the view, at a height the user picks, with a control to move through heights and a key of the flowers shown. Highlighting one flower dims the rest. Where no flower grows (no flower feature, or a random one), the layer says so.
- **The best height for a field.** For an area the user picks, the heights where one flower covers most of it, as a list or a chart: what a builder needs to choose the platform's height.
- **Search.** A constraint on a field of one flower, of at least some size, at a height or anywhere in a range of heights, within a distance: `require 200 minecraft:cornflower field at y 64..120 within 1000 of spawn`, in the search language's own grammar, which the plan settles. It works in a seed search and a search within one seed.
- **The command.** `spyglass flower at`, `spyglass render --flowers` and the search constraint, with `--json`.
- **Checked against the game.** The oracle samples the flower features' providers at many positions and heights, in each biome that has one, for 26.3 and the 26.4 snapshots.

Not in scope:

- The flowers the world generated, before any farm: random patches, which depend on the blocks and on the [decoration order](completed/27-decoration-order.md). A later milestone, if wanted.
- Other things bone meal grows (tall grass, mushrooms, moss), and flowers spreading on their own.
- Searching by colour or dye.

## Exit criteria

- [ ] The flower at a block, at any height, matches the oracle for every vanilla flower feature that picks by position, for 26.3 and the 26.4 snapshots.
- [ ] The viewer's flower layer shows the field at a chosen height, and the best-height list for an area agrees with the layer at the heights it names.
- [ ] A seed search and a search within a seed find fields of a chosen flower and size, and the [search spec](../specs/search.md) covers the constraint.
- [ ] The user guide describes the layer, the command and the constraint, with a flower farm as its example.
