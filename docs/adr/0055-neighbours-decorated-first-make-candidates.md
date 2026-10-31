# 0055. Neighbours decorated first make candidates, from every feature's write bounds

- Status: Superseded by [0056](0056-two-certainties-on-its-own-and-whatever-the-neighbours.md)
- Date: 2026-10-30

## Context

A chunk is decorated over its 3 × 3 neighbourhood, and its 8 neighbours can be decorated before it or after it. In a running server that depends on how the world was explored: chunks nearer a player are decorated first, so a chunk often has several neighbours decorated already. Their features write into the chunk's neighbourhood and can change what its features find: a dungeon's walls, a lake's edge, a well's sand.

spyglass places a chunk's features on its carved neighbourhood, none decorated ([ADR 0053](0053-the-oracle-decorates-chunks-on-their-own.md)'s model). The oracle's second run decorates all 8 neighbours first. In the golden area that changes 2 to 4 chunks of 2,684 per version: a lava lake declines or moves, a dungeon declines. Under the plain model those answers would be reported certain and be wrong in some worlds. Milestone 19 asks that no answer reported as certain is ever wrong.

There are about 25 leaf feature types in the Overworld's lists in each supported version, nested in selectors (`random_selector`, `sequence` and others). A neighbour runs all of them, in all 11 steps.

## Decision

We will treat a try as a candidate when a neighbour's decoration could write a block it reads. For each neighbour, spyglass computes every placed feature's tries from the seed. Where a modifier reads blocks, the try's position is a bounded range. Each feature type has a sound bound on what it can write, relative to its try, taken from its configuration: a box, from the bytecode. A feature that only ever replaces blocks with ones every target sees the same way is left out, computed from the data. Ores turning stone into ore are the case that matters. Every neighbour try counts as placing. Each target try has a bound on what it reads. Where a neighbour's write bound meets it, the try is a candidate (`Doubt::Neighbours`), and so is everything after it in the chunk.

The oracle's `after_neighbours` checks this: every golden chunk whose calls differ between the two runs must be a candidate in spyglass.

## Consequences

- No answer is reported certain that a neighbour decorated first could change, within the features spyglass bounds.
- Each feature type's bound has to be read from each version's bytecode and kept sound, which is real work for every new version (`cargo xtask version-diff --code` shows which classes changed).
- The bounds are conservative, so some certain answers become candidates. The share is measured on the golden seeds and recorded in the plan; if it's high, a feature can be placed exactly instead of bounded.
- Neighbours' structure pieces are a separate reason, handled with structures.

## Alternatives considered

- **State the model:** answer for a chunk decorated before its neighbours, and say so. Cheap, but in some worlds an answer reported certain would be wrong.
- **Bound only the features spyglass places:** misses the trees, springs and cave decoration that make up most of a neighbour's writes.
- **Place the neighbours exactly:** they depend on their own neighbours' order in turn, so the answer is never fixed.
