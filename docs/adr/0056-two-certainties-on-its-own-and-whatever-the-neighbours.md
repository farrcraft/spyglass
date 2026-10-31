# 0056. Two certainties: on its own, and whatever the neighbours do

- Status: Proposed
- Date: 2026-10-30
- Supersedes: [0055](0055-neighbours-decorated-first-make-candidates.md)

## Context

[ADR 0055](0055-neighbours-decorated-first-make-candidates.md) made a try a candidate whenever a neighbour decorated first could write a block it reads, from sound bounds on every feature's writes. Built and measured on the golden seeds, that leaves no answer certain: none of 26.3's 37,842 golden calls.

The reason is structural. Many of a neighbour's placed features try many times per chunk: dungeons, glow lichen, pointed dripstone, cave vegetation, sculk. Each try draws what its blocks decide, and a neighbour's blocks depend on its own neighbours, in an order nobody knows. So after a feature's first try, its later tries could be anywhere in the neighbour's chunk, at any height, and their writes reach 2 to 16 blocks across the border. Every target in the chunk is within reach of one.

Meanwhile, decorating each chunk on its own (the oracle's model, [ADR 0053](0053-the-oracle-decorates-chunks-on-their-own.md)) gives 37,437 certain answers that match the game, and the oracle's second run, all 8 neighbours decorated first, changes only 2 to 4 of 2,684 golden chunks per version.

## Decision

We will give each placing answer two certainties:

- **On its own:** the answer for the chunk decorated before its neighbours, certain unless something in the chunk spyglass can't know could change it: a tie, a feature it doesn't place, a template's shape updates. This is what's checked against the oracle's `tries` and what front ends show by default, saying what it means.
- **Whatever the neighbours do:** whether the answer also holds with any neighbours decorated first, from ADR 0055's write bounds. It's exact, and today almost never true; it grows as more features are placed exactly.

Both propagate inside the chunk: a candidate's writes, and its own placed feature's later tries, put what reads them in the same doubt.

## Consequences

- Most answers are certain on their own, with a stated model: right in a world explored so that the chunk was decorated before its neighbours, and wrong for about 1 chunk in 700 when its neighbours went first (the oracle's measurement).
- "Holds whatever the neighbours do" is sound, so it can be relied on where it's true. The oracle's `after_neighbours` checks it: every call that differs there is flagged.
- Front ends have to explain the first certainty. Search and the viewer show answers certain on their own, and say where neighbours could change them.
- The write bounds stay, and need keeping up per version ([ADR 0055](0055-neighbours-decorated-first-make-candidates.md)'s cost).

## Alternatives considered

- **ADR 0055 alone:** sound, but leaves nothing certain.
- **State the model only:** the same answers, but nothing tells the reader which ones neighbours could change.
- **Place neighbours' features exactly:** they depend on their own neighbours in turn, and it would take porting most of the Overworld's features before any answer held whatever the neighbours do.
