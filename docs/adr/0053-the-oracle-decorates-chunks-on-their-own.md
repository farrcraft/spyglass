# 0053. The oracle decorates chunks on their own, recording the target features' calls

- Status: Proposed
- Date: 2026-10-30

## Context

Milestone 19 checks where features try to place, and whether they place, against the game. Two things make that hard to get from the game:

- **Decoration doesn't say what it did.** `applyBiomeDecoration` runs every placed feature through its modifiers and calls the feature at each position. Nothing records the positions or the results, and the blocks afterwards don't say which feature put them there, or where a feature looked and declined.
- **A chunk's decoration depends on its neighbours.** Features write into the 8 neighbouring chunks and read them. In a running server, whether a neighbour was decorated first depends on the order chunks were asked for, which a contiguous golden area can't hold fixed.

[ADR 0049](0049-the-oracle-generates-chunks-with-stand-ins.md) already generates chunks through the generator's own steps over stand-ins for the server.

## Decision

We will decorate each golden chunk on its own: it and its 8 neighbours generated fresh through the step before features, none decorated, no structures. Then we repeat `ChunkStatusTasks.generateFeatures` with a real `WorldGenRegion`, built by its own constructor over stand-ins for the server level, chunk cache, chunk holders and server, and call the generator's own `applyBiomeDecoration`.

To see the calls, the harness swaps each target's configured feature in the registry for one that reports every call to `place` (the origin and the result) and delegates to the original. The region reports which placed feature is running. Decoration stops at the first feature past the last step with a target.

## Consequences

- The decoration loop, the modifiers, the features and the region's own block writes, write-radius checks, heightmaps and ticks are all the game's. The harness adds only the stand-ins and the recording.
- The files fix one generation order: neighbours undecorated. Results that a decorated neighbour would change aren't seen; a second run with neighbours decorated first can show them, when placing needs it.
- The stand-ins and the recording wrapper differ between versions (`Feature` is an interface from 26.3, a class in 26.2; constructors and the server level's methods change), so each adapter has its own.
- A feature that reaches a part of the server the stand-ins don't cover fails the run, so it's noticed rather than silently wrong.
- About 20 seconds per seed on 16 cores, since each decorated chunk generates its 9 chunks fresh.

## Alternatives considered

- **A real server generating the area:** the generation order of neighbours would vary between runs, and nothing records the calls.
- **Reading the features back from the blocks** (spawners for dungeons, budding amethyst for geodes): no record of declined tries, and a feature's origin isn't recoverable from what it placed.
- **A copy of the decoration loop in the harness:** it would test spyglass's understanding against itself where the copy is wrong.
