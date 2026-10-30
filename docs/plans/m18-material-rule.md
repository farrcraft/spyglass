# m18. The material rule and ore veins

- Status: Draft
- Milestone: [18. Surface rules, ore veins, caves and carvers](../roadmap/18-surface-veins-and-carvers.md)
- Started: YYYY-MM-DD
- Completed: YYYY-MM-DD

## Goal

Every block of a chunk after the noise and the material rule, without carvers, exactly as the game makes it. That covers 26.3's and 26.4's `MaterialSystem` with its ore veins, and 26.2's `SurfaceRules`. Also the library's block queries and their CLI command. It meets the milestone's first exit criterion for the `no-carvers` runs of the [block golden files](completed/m18-block-oracle.md).

## Approach

**Where it lives.** A new crate, `spyglass-blocks`, beside `spyglass-structure`. The material rule needs:

- biomes, for the biome and temperature conditions;
- the terrain's columns and aquifer, for what it fills;
- density functions, for the preliminary surface and noise conditions.

So it sits above `spyglass-biome` and `spyglass-terrain` and below `spyglass`, with the dependencies still flowing one way. Putting it in `spyglass-terrain` would make terrain depend on biomes, which it doesn't today. The crate boundary gets an ADR.

**Blocks.** A block state is an interned id with its properties, as the material rule's data names them (`Name`, `Properties`). A chunk's blocks are a palette and a `u16` per position. No block registry is needed: the material rule only places blocks its data names, and the column only the default block, fluids and air.

**Data.** Everything versioned comes from the bundle ([ADR 0026](../adr/0026-how-game-versions-coexist.md)), loaded through `spyglass-data`'s typed loading ([ADR 0010](../adr/0010-typed-loading-of-worldgen-json.md)):

- **26.3 and 26.4:** `worldgen/material_rule/` and `worldgen/material_condition/` (registries with references), and `noise_settings.material_rule`.
- **26.2:** `noise_settings.surface_rule`, inline.

The algorithms are the code's, one implementation per family:

- **26.3 (`levelgen.material`):**
  - `MaterialSystem.buildSurface`;
  - `MaterialRuleContext`, with its lazy XZ and Y conditions;
  - rules: block, sequence, condition, bandlands, ore vein;
  - conditions: above preliminary surface, biome, hole, noise threshold, not, steep, stone depth, temperature, vertical gradient, water, y.

  Port from the normalized bytecode, class by class, with the spec written as each lands.

- **26.4:** differences as named switches in `spyglass_data::versions`, found with `cargo xtask version-diff 26.3 26.4-snapshot-5 --code`. One is known: 26.4 leaves solid positions undecided until the material rule ([terrain.md](../specs/terrain.md#the-column)), and its rules end with the dimension's stone.
- **26.2:** `SurfaceRules` and `SurfaceSystem`, with the bandlands, eroded badlands pillars and frozen-ocean icebergs that `SurfaceSystem` places outside the rules. Ore veins are already in 26.2's columns (`OreVeinifier`, [terrain.md](../specs/terrain.md#262-terrain)).

**Whole chunks.** The material rule runs per chunk, after the chunk is filled from noise: it reads the chunk's heights (steep, water), its biome manager, and each column's surface depth and stone depth. So `spyglass-blocks` fills a chunk first. That's the noise fill: the column of [terrain.md](../specs/terrain.md#the-column) for all 256 columns, sharing cells and the aquifer as regions already do. Then it applies the rule. Whether the noise fill needs `NoiseChunk`'s exact interpolation path, rather than the column's, is settled by the golden files. The spike in the first task finds out.

**API** ([ADR 0014](../adr/0014-public-api-world-and-session.md)):

- `Session::chunk_blocks(chunk, Carvers::Off)`: a chunk's blocks.
- `Session::block_at([x, y, z])` and `Session::block_column([x, z])`.

The `Carvers` argument is there now so [m18-carvers](m18-carvers.md) doesn't change the signatures. Until then it accepts only `Off`. The CLI gains `spyglass blocks --seed|--world (--at x,y,z | --column x,z | --chunk cx,cz) [--no-carvers] [--json]`, documented in cli-json.md.

## Tasks

- [ ] **ADR:** the `spyglass-blocks` crate: blocks after the noise, the material rule and carvers, and why not in `spyglass-terrain`.
- [ ] **Spike:** fill one golden chunk from noise with today's columns, and compare the `no-carvers` run below the surface depth, where the material rule leaves stone (or deepslate). Decide whether chunks need `NoiseChunk`'s interpolation.
- [ ] **Blocks and chunks:** the state type, the palette, chunk storage, and the noise fill for whole chunks, in all three dimensions and all four versions.
- [ ] **Data:** typed loading of material rules and conditions (26.3 and 26.4) and surface rules (26.2), with errors that name the file and path. Fixture-bundle tests for each rule and condition type.
- [ ] **26.3's material rule:** each condition and rule, the context, and the system's loop, with ore veins. Unit tests on fixtures, then the golden `no-carvers` chunks of 26.3 in all three dimensions.
- [ ] **26.4:** the switches, and the golden chunks of both snapshots.
- [ ] **26.2's surface rules:** the rules, `SurfaceSystem`'s extras, and the golden chunks of 26.2.
- [ ] **API and CLI:** `chunk_blocks`, `block_at`, `block_column`; `spyglass blocks`; cli-json.md; CLI tests on fixtures.
- [ ] **Specs:** a new `material.md` (the rule, its conditions, the context, ore veins, 26.2's surface rules as a section, per-version notes), and terrain.md's pointers to it.
- [ ] **Cost:** `cargo xtask bench` gains a chunk's blocks without carvers. Record the time here.
- [ ] `cargo xtask check` and `cargo xtask oracle-test`.

## Verification

- Every golden `no-carvers` chunk matches, block for block, for every version and dimension (`oracle-test`).
- Height golden files still pass, since the noise fill must not change the columns.
- Fixture tests name each rule and condition, so a failure points at one.

## Durable outputs

- [ ] ADR: the `spyglass-blocks` crate.
- [ ] `docs/specs/material.md`.
- [ ] terrain.md: pointers to it, and the 26.2 ore veins' place.
- [ ] cli-json.md: `blocks`.

## Notes

- 2026-10-29: **From m18-block-oracle's self-check**, before this plan started:
  - **The noise fill:** spyglass's columns already agree with the game's chunks on air, water, lava and solid, at all 73.4 million golden blocks per version, apart from the material rule's own effects. So the spike task is mostly answered: whole chunks can start from today's columns. The spike is now a check of speed, not correctness.
  - **The material rule does more than choose solid blocks:**
    - icebergs, as packed ice and snow blocks over water and air;
    - eroded badlands pillars, as terracotta, red sand and dirt over air;
    - the Nether's `hole` rule, as lava on floors below y 32.

    The port must cover all three.

  - **The game's column and chunk differ far out:** at seed 1's Nether, x 29,999,940 and z 29,999,929, the game's `getBaseColumn` has netherrack from y 5 to 27, where its chunk and spyglass's column have lava. Blocks must follow the chunk, as spyglass's columns already do there. `cargo xtask oracle column` shows a column of both.
