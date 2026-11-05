# 0062. The oracle places structures when decorating, for containers

- Status: Proposed
- Date: 2026-11-05
- See also: [0053](0053-the-oracle-decorates-chunks-on-their-own.md), [0049](0049-the-oracle-generates-chunks-with-stand-ins.md)

## Context

Milestone 20 checks each structure's loot containers against the game: where each one is, its table and its loot seed ([loot.md](../specs/loot.md#containers)). A container's seed is a draw from the random that places it, and structures place their pieces during decoration, before each step's features, from a random seeded per structure and chunk. So the game's containers only exist in decorated chunks with structures in them.

[ADR 0053](0053-the-oracle-decorates-chunks-on-their-own.md) decorates chunks on their own with no structures: no starts, no references, a structure manager that has none. Placing pieces needs more of the game than features do. Starts come from `createStructures` on every chunk within 8 of the one decorated, and references from `createReferences`. Pieces load templates, and some spawn mobs that ask the level for its difficulty, dimension and structures, equip items and make sounds.

## Decision

We will add a run, `oracle containers`, that decorates chunks on their own as ADR 0053 does, with structures. Starts are made by the generator's own `createStructures`, and references by its own `createReferences`, on fresh proto chunks. Decoration reads them through a real `StructureManager`, made without its constructor, whose level is a stand-in that hands out those chunks and fails on anything else. The stand-in server level gains what pieces' mobs ask for, none of which draws from the chunk's random. Each chunk decorated gets its starts made again for itself alone, so it sees them as if it were the first chunk to place them. For each golden seed it decorates every chunk that the nearest start of each structure reaches, and records each container with a loot table and what placed it.

The terrain stays as ADR 0053 has it: generated without structures, so without their terrain adaptation.

## Consequences

- Starts, references, the per-structure random, piece placement, templates and their processors are all the game's. The harness adds only stand-ins and the recording.
- A piece can settle its state in the first chunk that places it (an ocean ruin its height, a desert pyramid its floor), so a start shared between chunks gives answers that depend on which chunk went first. Each chunk decorated gets its starts made again, for itself alone, as if it were the first; references are made once. Templates aren't safe to share between threads either (a palette caches its filtered blocks in a plain map), so each thread has its own template manager.
- Fortresses' and end cities' piece generators keep state in static fields, so where one can generate (the Nether, the End) starts are made one at a time, and those runs are slower.
- Without terrain adaptation, blocks near jigsaw structures that adapt terrain differ from a real world's. A container whose answer depends on such blocks (one placed after a village's tree, or a mineshaft corridor beside a village) may differ from the same seed's world, while still matching what spyglass computes from the same terrain.
- Feature golden files keep ADR 0053's setup, so their answers don't change.
- About 30 seconds per seed on 16 cores in 26.3, all three dimensions, most of it the Overworld's starts within 72 chunks of the origin and mineshafts' many chunks; 26.2 is slower.

## Alternatives considered

- **A real dedicated server, reading the containers back from its saves:** the generation order of neighbours varies, nothing says which structure placed a container, and it takes minutes per seed.
- **Feature golden files with structures:** every feature answer would change for chunks near structures, and the files test feature placement, which structures would blur.
- **Terrain with structures' terrain adaptation:** it's what a real world has, but block golden files and spyglass's terrain have none yet, so containers depending on it couldn't be compared like for like. It can follow once terrain adaptation is ported.
