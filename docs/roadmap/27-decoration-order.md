# 27. Decoration order around the spawn

- Status: Planned
- Depends on: [19](completed/19-features.md)
- Plans: none yet

## Goal

Find out whether the order the game decorates chunks in can be known where it matters most, around the spawn when a world is created, and if it can, give a third answer for features and loot: as the game decorates them then. [ADR 0056](../adr/0056-two-certainties-on-its-own-and-whatever-the-neighbours.md) gives two answers, for a chunk decorated before its neighbours and whatever its neighbours do, because the order is unknown in general. About 1 chunk in 700 differs when neighbours go first ([features.md](../specs/features.md#certainty)), and searches for dungeons and loot near the spawn ask exactly there. Horizon reports that a model of the chunk scheduler around a standing player agrees with vanilla as well as two vanilla runs agree with each other ([prior art](../research/prior-art.md)).

This milestone is research first: the implementation is in scope only if the measurement says it's worth it.

## Scope

- **Measuring the game.** The oracle's real server ([ADR 0015](../adr/0015-locate-checks-on-a-real-server.md)) records the order it decorates chunks in, from JFR's `minecraft.ChunkGeneration` events: for new worlds of many seeds, around the spawn at creation, and twice per seed to see how often two runs disagree.
- **What the order changes.** For the chunks whose order varies, how often a feature or container answer changes, against ADR 0056's 1 in 700.
- **A model**, if the order around the spawn at creation turns out stable: the scheduler's order, reproduced, and compared with the recordings.
- **A decision**, in an ADR: a third answer, "as decorated when the world is created", or not, and why.

Not in scope:

- Orders that depend on how a player explores after creation.

## Exit criteria

- [ ] The measurements are recorded in `docs/research/`: how stable the order around the spawn is between runs and seeds, and how many answers it changes.
- [ ] An ADR accepts or rejects a third answer.
- [ ] If accepted, feature and loot answers near the spawn carry it, checked against the recorded orders.
