# 0074. No third answer for the decoration order at world creation

- Status: Accepted
- Date: 2026-11-14
- See also: [0056](0056-two-certainties-on-its-own-and-whatever-the-neighbours.md), [0073](0073-the-oracle-joins-a-stand-in-player.md)

## Context

[ADR 0056](0056-two-certainties-on-its-own-and-whatever-the-neighbours.md) gives feature and loot answers two certainties, a chunk decorated on its own and an answer whatever its neighbours do, because the order the game decorates chunks in is unknown in general. With all 8 neighbours decorated first, about 1 golden chunk in 700 differs. Searches for dungeons and loot near the spawn ask exactly where a new world is first decorated, and Horizon reported a model of the scheduler that agrees with vanilla about as well as two vanilla runs do ([prior art](../research/prior-art.md)). If the order around the spawn at creation were stable, spyglass could give a third answer there: as the game decorates it when the world is created.

The measurement is in [decoration-order.md](../research/decoration-order.md#measurements): 26.2, 26.3 and 26.4-snapshot-8, 80 seeds in all, two runs each, recorded from the game's own JFR events with a stand-in player joined ([ADR 0073](0073-the-oracle-joins-a-stand-in-player.md)), and each order replayed through the game's own features.

- Startup without a player decorates only 9 chunks. The area around the spawn is decorated by the first join, out to the view distance.
- The join's spawn search lands in a different chunk two runs in three, so the decorated area itself moves.
- Where two runs overlap, about 1 neighbouring pair in 11 is decorated in the opposite order, and about half the chunks have other neighbours decorated first. At 2 processors it's 1 pair in 20: more regular, not stable.
- The order changes a chunk's answer from on its own in 94 of 125,125 decorated chunks (1 in 1,330), almost all dungeons and lava lakes, spread across the area. Two runs' answers differ in 34 of 84,224 shared chunks.

## Decision

We will not give a third answer. Around the spawn at creation, as anywhere, spyglass keeps ADR 0056's two: on its own, and whatever the neighbours do. The order the game decorates in at creation is not reproducible between runs of the same seed on the same machine, so there is no single answer "as the game decorates it" to give, and no scheduler model to build.

## Consequences

- No scheduler model, no new certainty in the API, and nothing for front ends to show. ADR 0056 stands unchanged.
- How far the order at creation moves an answer is now known, with numbers: about 1 chunk in 1,330 differs from on its own, nearly all dungeons and lava lakes. The research doc is the reference.
- The `oracle order` and `oracle replay` commands and their files stay, for measuring again when a version changes the chunk scheduler. They aren't part of `oracle golden` or CI, so a change in the scheduler isn't noticed unless someone reruns them.
- The cost: the residual near the spawn stays unanswered. A user whose world was created on one machine gets "on its own" for a chunk the game, in that run, decorated after a neighbour that changed it, and spyglass can't say which run they had.
- Measured on one Windows machine with 16 hardware threads. Other machines may be more or less regular; the 2-processor runs suggest fewer workers are more regular but still not stable.

## Alternatives considered

- **A third answer from a scheduler model:** the order a model gives can at best agree with the game as well as another run does, and another run disagrees about half the chunks' neighbours. The answer would change in about 1 chunk in 1,330, and two runs of the game already disagree in about half of those.
- **A third answer from the most common recorded order, or "differs in some runs":** needs many recordings per seed, at every version, for every seed a user might ask about. That isn't something a local tool can do, and the answer would still be a probability, not a certainty.
- **A third answer only for startup's 9 chunks, decorated before any player joins:** 9 chunks around the origin chunk is far less than the searches near the spawn ask about, and those 9 are the ones a join then surrounds anyway.
