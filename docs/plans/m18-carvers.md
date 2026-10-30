# m18. Carvers

- Status: Draft
- Milestone: [18. Surface rules, ore veins, caves and carvers](../roadmap/18-surface-veins-and-carvers.md)
- Started: YYYY-MM-DD
- Completed: YYYY-MM-DD

## Goal

The caves and ravines that carvers cut after the material rule, exactly as the game makes them, so a chunk's blocks are the game's before features. It meets the milestone's first exit criterion for the `terrain` runs of the [block golden files](completed/m18-block-oracle.md), building on [m18-material-rule](m18-material-rule.md).

## Approach

**What carvers are.** Each biome lists configured carvers (`worldgen/carver/`): `cave`, `cave_extra_underground`, `canyon` and `nether_cave` in vanilla. For a chunk, the game visits every chunk within 8. For each, it takes that chunk's carver biome (the biome at its quart (2, 0, 2) position, as `getBiomeGenerationSettingsForCarver` resolves it) and each carver in order. It seeds a `WorldgenRandom` with `setLargeFeatureSeed(seed + index, x, z)`, and if `isStartChunk` passes, carves into the target chunk. The height providers (`uniform`, `very_biased_to_bottom`, `trapezoid`, …) and float providers in the carvers' data come in with them, in `spyglass-data`.

**Per version** (from the bytecode; the spike confirms):

- **26.2:** carvers write into the chunk's blocks:
  - They replace blocks in the carver's `replaceable` tag.
  - They fill with the aquifer's answer, or lava below `lava_level`.
  - They fix the surface block above a carved grass or mycelium position.
- **26.3:** carvers only mark a `CarvingMask`. `applyCarvingMask` then gives each marked position the aquifer's substance, and the material rule's block where that's solid. The carver data loses `replaceable`, `lava_level` and `debug_settings`.
- **26.4 snapshots:** the same as 26.3, through `generateCarvingMask`. Other differences are found with `version-diff --code` and become switches.

**Where it lives.** `spyglass-blocks`, after the material rule: `Session::chunk_blocks(chunk, Carvers::On)` (the default), and `block_at` and `block_column` with carvers. The CLI's `--no-carvers` becomes meaningful.

**Cost.** Up to 17 × 17 chunks' carvers can reach one chunk, but each is a cheap random check unless it starts there. A chunk's carvers should cost little next to its noise. The bench records it.

## Tasks

- [ ] **Spike:** port the cave carver for 26.3 only, and compare one golden chunk's carved positions: the difference between its `terrain` and `no-carvers` runs.
- [ ] **Data:** configured carvers, height providers and float providers for all versions. Typed loading and fixture tests.
- [ ] **26.3:** the cave, canyon and nether cave carvers, the start check and seeding, the carving mask, and applying it through the aquifer and the material rule. The golden `terrain` chunks of 26.3 in all three dimensions.
- [ ] **26.4:** the switches, and both snapshots' golden chunks.
- [ ] **26.2:** carving into blocks (replaceable, lava level, surface fix-up), and 26.2's golden chunks.
- [ ] **API and CLI:** carvers on by default, `--no-carvers`, and docs.
- [ ] **Spec:** `carvers.md`: the start check, the seeding, each carver's shape, the mask, per-version notes. Link it from terrain.md and material.md.
- [ ] **Cost:** `cargo xtask bench` gains a chunk with carvers. Record it here.
- [ ] `cargo xtask check` and `cargo xtask oracle-test`.

## Verification

- Every golden `terrain` chunk matches, for every version and dimension.
- The `no-carvers` comparisons still pass, so carving didn't move the material rule.

## Durable outputs

- [ ] `docs/specs/carvers.md`.
- [ ] terrain.md and material.md: pointers to it.
- [ ] cli-json.md: what `--no-carvers` changes.

## Notes
