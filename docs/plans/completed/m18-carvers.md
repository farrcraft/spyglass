# m18. Carvers

- Status: Completed
- Milestone: [18. Surface rules, ore veins, caves and carvers](../../roadmap/18-surface-veins-and-carvers.md)
- Started: 2026-10-30
- Completed: 2026-10-30

## Goal

The caves and ravines that carvers cut after the material rule, exactly as the game makes them, so a chunk's blocks are the game's before features. It meets the milestone's first exit criterion for the `terrain` runs of the [block golden files](m18-block-oracle.md), building on [m18-material-rule](m18-material-rule.md).

## Approach

**What carvers are.** Each biome lists configured carvers (`worldgen/carver/`): `cave`, `cave_extra_underground`, `canyon` and `nether_cave` in vanilla. For a chunk, the game visits every chunk within 8. For each, it takes that chunk's carver biome (the biome at its quart (2, 0, 2) position, as `getBiomeGenerationSettingsForCarver` resolves it) and each carver in order. It seeds a `WorldgenRandom` with `setLargeFeatureSeed(seed + index, x, z)`, and if `isStartChunk` passes, carves into the target chunk. The height providers (`uniform`, `very_biased_to_bottom`, `trapezoid`, …) and float providers in the carvers' data come in with them, in `spyglass-data`.

**Per version** (from the bytecode; the spike confirms):

- **26.2:** carvers write into the chunk's blocks:
  - They replace blocks in the carver's `replaceable` tag.
  - They fill with the aquifer's answer, or lava below `lava_level`.
  - They fix the surface block above a carved grass or mycelium position.
- **26.3:** carvers only mark a `CarvingMask`. `applyCarvingMask` then gives each marked position the aquifer's substance, and the material rule's block where that's solid. The carver data loses `replaceable`, `lava_level` and `debug_settings`.
- **26.4 snapshots:** the material rule itself is ported here, since `ChunkTerrainBuilder.fillColumn` runs it and the carving mask in one pass per column (columns from `NoiseChunk.prepareColumn`, the chunk's upscaled biomes, `#uncarvable`, dirt under an opening evaluated again; see [m18-material-rule](m18-material-rule.md)'s notes). Other differences are found with `version-diff --code` and become switches.

**Where it lives.** `spyglass-blocks`, after the material rule: `Session::chunk_blocks(chunk, Carvers::On)` (the default), and `block_at` and `block_column` with carvers. The CLI's `--no-carvers` becomes meaningful.

**Cost.** Up to 17 × 17 chunks' carvers can reach one chunk, but each is a cheap random check unless it starts there. A chunk's carvers should cost little next to its noise. The bench records it.

## Tasks

- [x] **Spike:** port the cave carver for 26.3 only, and compare one golden chunk's carved positions: the difference between its `terrain` and `no-carvers` runs.
- [x] **Data:** configured carvers, height providers and float providers for all versions: `spyglass_data::providers`, and carvers in `spyglass-blocks` (26.3's and 26.2's forms). Typed loading and fixture tests.
- [x] **26.3:** the cave, canyon and nether cave carvers, the start check and seeding, the carving mask, and applying it through the aquifer and the material rule. The golden `terrain` chunks of 26.3 in all three dimensions.
- [x] **26.4:** `ChunkTerrainBuilder`: the material rule interleaved with the carving mask, the switches, and both snapshots' golden chunks, with and without carvers. The block tests are no longer pinned, and the 26.4 error in `MaterialSettings::load` is gone.
- [x] **26.2:** carving into blocks (replaceable, lava level, surface fix-up), and 26.2's golden chunks.
- [x] **API and CLI:** carvers on by default, `--no-carvers`, and docs.
- [x] **Spec:** `carvers.md`: the start check, the seeding, each carver's shape, the mask, per-version notes. Link it from terrain.md and material.md.
- [x] **Cost:** `cargo xtask bench` gains a chunk with carvers. Record it here: 43 ms a chunk in 26.3 (41 without), 98 ms in 26.2 (59 without), 44 ms in 26.4-snapshot-6 (45 without); see the notes.
- [x] `cargo xtask check` and `cargo xtask oracle-test`.

## Verification

- Every golden `terrain` chunk matches, for every version and dimension.
- The `no-carvers` comparisons still pass, so carving didn't move the material rule.

## Durable outputs

- [x] `docs/specs/carvers.md`.
- [x] terrain.md and material.md: pointers to it.
- [x] cli-json.md: what `--no-carvers` changes.

## Notes

- 2026-10-30: **26.3 matches, spike and all.** The spike went straight to the whole port: both carver types, the start check and seeding, the mask and its application. Every block of 26.3's 960 golden chunks with carvers matches the game except 386 that depend on tied biomes, in all three dimensions; in the Overworld file checked by hand, carvers change 32,256 blocks, and spyglass the same.
  - **26.3 rewrote carvers.** `CaveWorldCarver` is a record with `count`, `thickness`, `weird_thickness_bias` and `start_vertical_radius_multiplier`; the Nether's carver is a `cave`, and `nether_cave` is gone as a type. Configs lose `replaceable`, `lava_level`, `debug_settings` and `yScale`.
  - **The carver biome** is the source's biome at quart y 0, uncached, not the chunk's stored biome.
  - **26.3's `Mth.SIN`** has no exact quarter turns (`density`'s easing table is 26.4's), so carvers have their own.
  - **Providers** moved to `spyglass_data::providers` (specs/providers.md), with every type the codecs register; structures use them too.
  - Tag resolution is now `spyglass_terrain::tag`, for any registry, keeping entry order.
- 2026-10-30: **26.2 matches but for one block.** 26.2's carvers are 26.3's shapes: 26.3 moved the cave count, thickness and vertical scale into data with the same draws and results, so 26.2's configs map onto them. What differs is carving straight into the chunk through a sink: the mask skips positions already carved, `replaceable`, `lava_level`, the Nether carver's lava and cave air, and the surface fix-up per ellipsoid column. Every block of 26.2's golden chunks with carvers matches except tied ones and one Nether block at x −1,000,064, where the game has air and spyglass cave air. The game's own no-carvers chunk and base column have netherrack there, and nothing in 26.2's Nether carving writes plain air. It's named in the test and in carvers.md's open questions.
- 2026-10-30: **Cost.** `chunk_blocks with carvers, bound`: 43 ms a chunk in 26.3, 2 ms over the chunk without carvers, so the start checks over 17 × 17 chunks are cheap, as expected. In 26.2 it's 98 ms, 40 ms over: each carved column asks the aquifer for its whole column (`Terrain::aquifer_column`), and 26.2's family builds a one-column `NoiseChunk` for that. Asking only at carved positions would cut it; left for [m18-layers](../m18-layers.md), which sets its limits from measured cost.
- 2026-10-30: **26.4 matches, both snapshots, with and without carvers.** `ChunkTerrainBuilder` ported: columns from terrain's 26.4 column (solid left undecided), gradients from the chunk's column tops, the rule and the carving per column, pillars on the noise column and icebergs that skip carved positions. Without carvers it matched on the first run. With carvers, two switches were needed, both found from the golden differences:
  - `rounded_trig` (5125 on): 26.4's `Mth.sin` and `Mth.cos` round to the nearest table entry, are symmetric about 0, and have exact quarter turns. Truncating as 26.3 did moved a few hundred ellipsoid edge blocks.
  - `carving_mask_level_bounds` (5126 on): snapshot-6's mask spans the level, snapshot-5's the anchors' context, which in the Nether stops at y 120 rather than 248.
