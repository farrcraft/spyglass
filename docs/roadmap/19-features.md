# 19. Features

- Status: Planned
- Depends on: [18](completed/18-surface-veins-and-carvers.md)
- Plans: none yet

## Goal

Locate and draw the features players look for: dungeons, amethyst geodes, desert wells, fossils and lava lakes. Each answer is certain or a candidate, as structures are. Chunkbase marks dungeons and geodes as often wrong, because they depend on the blocks around them. With [18](completed/18-surface-veins-and-carvers.md)'s blocks, spyglass can say which ones really generate.

## Scope

- **Decoration:** the order the game decorates a chunk in (biome feature steps, then features in each step's order), the per-feature random seeds, and placed features' placement modifiers (count, rarity, in-square, height range, biome, surface-relative, environment scans), all as data with the algorithms in code.
- **The features first:** `monster_room` (dungeons), `amethyst_geode`, `desert_well`, `fossil` (with its diamond variant), `lake` (lava pools), and the Nether's fossils. Each comes with its own validity checks against the blocks: a dungeon's air and solid counts, a geode's exposure, a well's sand.
- **Certainty:** a feature whose checks depend on blocks spyglass can't yet compute, such as other features placed earlier in the same chunk, is a candidate, never a guess. The spec lists which checks those are. Where earlier features are cheap to place exactly, the plan may place them.
- **Queries:** features in an area and the nearest feature, with the same certain and candidate answers and a `--json` form, as `spyglass locate feature` and in the API.
- **The viewer:** feature layers with icons in the shared style, a Locate tab entry, and features as targets and conditions in [16](completed/16-search-within-a-seed.md)'s search and the seed search ("two dungeons within 16 blocks").
- **The oracle:** golden feature positions from the game's own decoration, for the golden seeds, in every supported version.
- **Specs:** a features spec, with per-version notes.

Not in scope:

- Every feature. Trees, ores, flowers and other decoration come only where a target feature's validity needs them.
- The final spawn block, which needs features too. It stays in [Not yet planned](README.md#not-yet-planned) until this milestone shows how far its features reach.
- Loot ([20](20-loot.md)).

## Exit criteria

- [ ] Every feature in scope matches the game's positions in the golden area of each golden seed, for every supported version. Candidates are listed with their reason, and none is the game's answer reported as certain and wrong.
- [ ] `spyglass locate feature minecraft:monster_room` answers from the origin on a real seed, and the viewer draws dungeons at 1:4.
- [ ] A seed search for two dungeons within 16 blocks of each other near spawn returns seeds whose pairs the game confirms (an ignored oracle test).
- [ ] `cargo xtask check` and `cargo xtask oracle-test` pass.
